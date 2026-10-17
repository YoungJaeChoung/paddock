//! Windows 보안 도우미: 사용자 SID, 개체의 DACL 읽기·좁히기, 프로세스 소유자, 작업 개체 소속.
use std::io;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT, SE_KERNEL_OBJECT,
    SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::JobObjects::{
    IsProcessInJob, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// `GetExitCodeProcess`가 아직 끝나지 않은 프로세스에 돌려주는 값.
const STILL_ACTIVE: u32 = 259;

/// 널로 끝나는 UTF-16 문자열 포인터를 `String`으로 복사한다.
fn pwstr_to_string(p: *const u16) -> String {
    unsafe {
        let mut n_chars = 0;
        while *p.add(n_chars) != 0 {
            n_chars += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n_chars))
    }
}

/// 널로 끝나는 UTF-16 버퍼. 윈도우 API에 넘기는 문자열이다.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// 토큰의 사용자 SID 문자열.
fn token_user_sid(token: HANDLE) -> io::Result<String> {
    unsafe {
        let mut need = 0u32;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut need);
        // TOKEN_USER는 포인터를 담아 8바이트 정렬이 필요하므로, 바이트가 아니라 u64 버퍼에 받는다.
        let mut buf = vec![0u64; (need as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), need, &mut need) == 0 {
            Err(io::Error::last_os_error())
        } else {
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
                Err(io::Error::last_os_error())
            } else {
                let out = pwstr_to_string(s);
                LocalFree(s.cast());
                Ok(out)
            }
        }
    }
}

/// 이 프로세스를 실행한 사용자의 SID 문자열(`S-1-5-21-…`).
pub fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            Err(io::Error::last_os_error())
        } else {
            let sid = token_user_sid(token);
            CloseHandle(token);
            sid
        }
    }
}

/// 프로세스를 실행한 사용자의 SID. 프로세스를 열 수 없으면(권한 없음·이미 끝남) 그 오류다.
pub fn process_user_sid(pid: u32) -> io::Result<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            Err(io::Error::last_os_error())
        } else {
            let mut token = null_mut();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
            let opened_error = io::Error::last_os_error();
            CloseHandle(process);
            if opened == 0 {
                Err(opened_error)
            } else {
                let sid = token_user_sid(token);
                CloseHandle(token);
                sid
            }
        }
    }
}

/// 프로세스가 아직 실행 중인가.
pub fn process_is_alive(pid: u32) -> bool {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        let mut alive = false;
        if !process.is_null() {
            let mut code = 0u32;
            alive = GetExitCodeProcess(process, &mut code) != 0 && code == STILL_ACTIVE;
            CloseHandle(process);
        }
        alive
    }
}

/// 파이프·파일·폴더를 READ_CONTROL로 열어 GetSecurityInfo로 소유자·DACL을 SDDL 문자열로 읽는다.
pub fn object_sddl(path: &str) -> io::Result<String> {
    unsafe {
        let w = wide(path);
        // 폴더도 열 수 있도록 BACKUP_SEMANTICS를 준다(파이프·파일에는 영향이 없다).
        let h = CreateFileW(
            w.as_ptr(),
            READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            let mut sd = null_mut();
            let rc = GetSecurityInfo(
                h,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            );
            CloseHandle(h);
            if rc != 0 {
                Err(io::Error::from_raw_os_error(rc as i32))
            } else {
                let mut s = null_mut();
                let mut n_chars = 0u32;
                let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    sd,
                    1,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut s,
                    &mut n_chars,
                );
                let out = if ok == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    let text = pwstr_to_string(s);
                    LocalFree(s.cast());
                    Ok(text)
                };
                LocalFree(sd);
                out
            }
        }
    }
}

/// 파일·폴더의 DACL을 현재 사용자 한 항목만 둔 보호된 DACL로 바꾼다(상위 폴더의 권한을 물려받지 않는다).
/// 폴더는 그 안에 새로 만드는 파일·폴더가 같은 권한을 물려받는다.
pub fn restrict_to_current_user(
    path: &Path,
    is_dir: bool,
) -> io::Result<()> {
    let inherit = if is_dir { "OICI" } else { "" };
    let sid = current_user_sid()?;
    let sddl = wide(&format!("D:P(A;{inherit};FA;;;{sid})"));
    let mut path_wide = wide(&path.to_string_lossy());
    unsafe {
        let mut sd = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut sd, null_mut()) == 0 {
            Err(io::Error::last_os_error())
        } else {
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = null_mut();
            let got = GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted);
            let result = if got == 0 || present == 0 {
                Err(io::Error::last_os_error())
            } else {
                let rc = SetNamedSecurityInfoW(
                    path_wide.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    dacl,
                    null_mut(),
                );
                if rc == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(rc as i32)) }
            };
            LocalFree(sd);
            result
        }
    }
}

/// 이 프로세스가 속한 작업 개체의 제한 플래그. 작업 개체에 속하지 않으면 None.
pub fn job_limit_flags() -> Option<u32> {
    unsafe {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        let ok = QueryInformationJobObject(
            null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        );
        if ok != 0 { Some(info.BasicLimitInformation.LimitFlags) } else { None }
    }
}

/// 이 프로세스가 어떤 작업 개체에든 속해 있는가.
pub fn in_job() -> bool {
    unsafe {
        let mut r = 0;
        IsProcessInJob(GetCurrentProcess(), null_mut(), &mut r);
        r != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_user_sid_looks_like_a_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
    }

    #[test]
    fn test_process_user_sid_of_self_matches_current_user() {
        assert_eq!(process_user_sid(std::process::id()).unwrap(), current_user_sid().unwrap());
    }

    #[test]
    fn test_job_queries_do_not_fail() {
        let _ = in_job();
        let _ = job_limit_flags();
    }
}
