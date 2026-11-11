//! 데몬을 앱과 분리된 프로세스로 띄운다. 앱 창을 닫아도 데몬과 그 안의 터미널이 살아남게 하기 위해서다.
use std::io;
use std::path::{Path, PathBuf};

/// 앱이 분리 실행에 실패(부모 작업 개체가 분리를 허락하지 않음)했음을 데몬에 알리는 실행 인자.
/// 데몬이 `daemon.json`에 그대로 기록한다.
pub const BREAKAWAY_DENIED_ARG: &str = "--breakaway-denied";

/// 띄운 데몬 프로세스.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchedDaemon {
    pub pid: u32,
    /// 부모 작업 개체에서 분리하려다 거부당해, 분리 없이 띄웠는가. true면 부모 작업 개체가 끝날 때 데몬도 끝난다.
    pub breakaway_denied: bool,
}

/// 앱 실행 파일 옆의 데몬 실행 파일 경로. 앱과 데몬은 한 폴더에 함께 배포된다.
pub fn sibling_daemon_exe() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!("paddock-daemon{}", std::env::consts::EXE_SUFFIX)))
}

/// 데몬을 분리해 띄우고 기다리지 않고 돌아온다. 데몬이 준비됐는지는 `daemon.json`으로 확인한다.
///
/// 호출자의 핸들(출력 파이프 등)을 데몬이 물려받지 않는다 — 물려받으면 호출한 쪽이 그 파이프가 닫히기를
/// 데몬이 끝날 때까지 기다리게 된다. Windows는 부모 작업 개체에서 분리를 시도하고, 거부당하면(접근 거부)
/// 분리 없이 다시 띄우면서 `--breakaway-denied`로 데몬에 알린다. Unix는 새 프로세스 그룹으로 띄운다.
pub fn launch_daemon(exe: &Path) -> io::Result<LaunchedDaemon> {
    #[cfg(windows)]
    let launched = windows::launch(exe);
    #[cfg(unix)]
    let launched = unix::launch(exe);
    launched
}

/// 두 경로의 파일 이름이 같은가(폴더는 보지 않는다). 지워진 실행 파일의 `(deleted)` 표시는 무시한다.
fn same_file_name(
    image: &Path,
    exe: &Path,
) -> bool {
    let name = |path: &Path| {
        path.file_name().map(|name| name.to_string_lossy().trim_end_matches(" (deleted)").to_ascii_lowercase())
    };
    name(image).is_some() && name(image) == name(exe)
}

/// 데몬 프로세스 `pid`를 강제로 끝낸다. 멈춰서 종료 요청에 답하지 못하는 데몬에 쓴다.
///
/// 그 번호의 프로세스가 `exe`와 같은 이름의 실행 파일일 때만 끝낸다. 다른 프로그램이 번호를 물려받았으면 건드리지 않고 오류다.
pub fn terminate_daemon(
    pid: u32,
    exe: &Path,
) -> io::Result<()> {
    #[cfg(windows)]
    let result = windows::terminate(pid, exe);
    #[cfg(unix)]
    let result = unix::terminate(pid, exe);
    result
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, OpenProcess,
        PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
        STARTUPINFOW, TerminateProcess,
    };

    use super::{BREAKAWAY_DENIED_ARG, LaunchedDaemon};

    pub fn launch(exe: &Path) -> std::io::Result<LaunchedDaemon> {
        // 콘솔을 아예 물려받지 않는 `DETACHED_PROCESS`로 창이 생기지 않으므로 `CREATE_NO_WINDOW`는 쓰지 않는다
        // (둘을 함께 주면 `CREATE_NO_WINDOW`는 무시된다). `CREATE_NEW_PROCESS_GROUP`은 앱을 연 콘솔에서
        // Ctrl+C가 오면 데몬까지 끝나는 일을 막는다.
        let base_flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        match create_process(exe, &[], base_flags | CREATE_BREAKAWAY_FROM_JOB) {
            Ok(pid) => Ok(LaunchedDaemon { pid, breakaway_denied: false }),
            Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                create_process(exe, &[BREAKAWAY_DENIED_ARG], base_flags)
                    .map(|pid| LaunchedDaemon { pid, breakaway_denied: true })
            }
            Err(error) => Err(error),
        }
    }

    /// `CreateProcessW`를 직접 불러 핸들 상속을 끈다(`bInheritHandles = FALSE`). Rust 표준 라이브러리의
    /// 실행은 상속 가능한 핸들(호출자의 출력 파이프 등)을 자식에게 넘길 수 있다.
    fn create_process(
        exe: &Path,
        args: &[&str],
        flags: u32,
    ) -> std::io::Result<u32> {
        let mut command_line = format!("\"{}\"", exe.display());
        for arg in args {
            command_line.push(' ');
            command_line.push_str(arg);
        }
        let mut wide: Vec<u16> = OsString::from(command_line).encode_wide().chain(Some(0)).collect();
        // SAFETY: 구조체는 0으로 채워 쓰고, 명령줄 버퍼는 호출이 끝날 때까지 살아 있다.
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let created = CreateProcessW(
                null(),
                wide.as_mut_ptr(),
                null(),
                null(),
                0,
                flags,
                null(),
                null(),
                &startup,
                &mut info,
            );
            if created == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                CloseHandle(info.hThread);
                CloseHandle(info.hProcess);
                Ok(info.dwProcessId)
            }
        }
    }

    /// `pid` 프로세스가 `exe`와 같은 이름의 실행 파일이면 끝낸다.
    pub fn terminate(
        pid: u32,
        exe: &Path,
    ) -> std::io::Result<()> {
        // SAFETY: 핸들은 이 함수 안에서만 쓰고 닫는다. 이름 버퍼 길이는 호출이 읽고 쓰는 만큼 넘긴다.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                Err(std::io::Error::last_os_error())
            } else {
                let mut buffer = [0u16; 1024];
                let mut n_units = buffer.len() as u32;
                let named = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut n_units);
                let image = String::from_utf16_lossy(&buffer[..n_units as usize]);
                let outcome = if named == 0 {
                    Err(std::io::Error::last_os_error())
                } else if !super::same_file_name(Path::new(&image), exe) {
                    Err(std::io::Error::other("The process is not the terminal daemon"))
                } else if TerminateProcess(handle, 1) == 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                };
                CloseHandle(handle);
                outcome
            }
        }
    }
}

#[cfg(unix)]
mod unix {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::LaunchedDaemon;

    /// `pid` 프로세스가 `exe`와 같은 이름의 실행 파일이면 끝낸다.
    pub fn terminate(
        pid: u32,
        exe: &Path,
    ) -> std::io::Result<()> {
        let image = std::fs::read_link(format!("/proc/{pid}/exe"));
        match image {
            Ok(image) if super::same_file_name(&image, exe) => {
                // SAFETY: 프로세스 번호와 신호만 넘기며 메모리를 건드리지 않는다.
                if unsafe { libc::kill(pid as i32, libc::SIGKILL) } == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            }
            Ok(_) => Err(std::io::Error::other("The process is not the terminal daemon")),
            Err(error) => Err(error),
        }
    }

    pub fn launch(exe: &Path) -> std::io::Result<LaunchedDaemon> {
        let mut command = Command::new(exe);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
        let mut child = command.spawn()?;
        let pid = child.id();
        // 데몬이 끝난 뒤 좀비로 남지 않도록 거둔다. 기다림은 이 스레드가 맡아 호출자는 막히지 않는다.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(LaunchedDaemon { pid, breakaway_denied: false })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_support::{DaemonCleanup, TempHome, daemon_exe, kill_process};

    /// 부모 시험이 자식에게 "임시 홈을 쥐고 있다"고 알리는 환경 변수 이름.
    const PARENT_HOME_MARK: &str = "PADDOCK_TEST_PARENT_HOLDS_HOME";

    /// `test_c_d18a_...`가 자식 프로세스로 다시 실행하는 시험. 데몬을 띄우고 pid를 출력한 뒤 끝난다.
    ///
    /// 홈 폴더는 부모 시험이 쥔 임시 홈을 물려받는다 — 자식이 자기 임시 홈을 만들면 끝나며 폴더를 지우는데,
    /// 그때 아직 사는 데몬이 기록 파일을 쓰며 지워진 폴더를 되살린다. 단독 실행으로 실제 폴더를 건드리지 않게
    /// 부모가 넘기는 표지가 없으면 멈춘다.
    #[test]
    #[ignore = "run by test_c_d18a_launch_returns_without_waiting in a child process"]
    fn launch_in_child() {
        assert!(
            std::env::var_os(PARENT_HOME_MARK).is_some(),
            "run only through test_c_d18a_launch_returns_without_waiting"
        );
        let launched = launch_daemon(&daemon_exe()).unwrap();
        println!("LAUNCHED {}", launched.pid);
    }

    #[test]
    fn test_c_d18a_launch_returns_without_waiting() {
        // 자식이 띄우는 데몬은 사용자 SID에서 정해지는 같은 파이프 이름을 쓴다. 자식이 사는 동안 같은 프로세스의 다른
        // 시험이 데몬을 띄우지 못하도록, 자식을 띄우기 전에 임시 홈(프로세스 안 잠금)을 쥐고 데몬이 끝난 뒤에 놓는다.
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        // 호출자의 표준 출력이 파이프인 채로 데몬을 띄운다. 데몬이 그 파이프를 물려받으면
        // 호출자가 끝나도 파이프가 닫히지 않아 읽는 쪽이 데몬이 끝날 때까지 기다리게 된다.
        let started = Instant::now();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon_launch::tests::launch_in_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env(PARENT_HOME_MARK, "1")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = String::new();
        child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
        let elapsed = started.elapsed();
        child.wait().unwrap();

        let pid: u32 = output
            .lines()
            .find_map(|line| line.split("LAUNCHED ").nth(1))
            .unwrap_or_else(|| panic!("no pid in child output: {output}"))
            .trim()
            .parse()
            .unwrap();
        kill_process(pid);
        assert!(elapsed < Duration::from_secs(1), "launch held the caller's pipe for {elapsed:?}");
    }

    /// 분리를 허락하지 않는 작업 개체(`JOB_OBJECT_LIMIT_BREAKAWAY_OK` 없음) 안에서 데몬을 띄운다.
    /// 부모가 작업 개체에 넣은 뒤 "go" 줄을 보내면 띄우고, 부모가 입력을 닫을 때까지 살아 있어 임시 홈 폴더를 지키고 있다.
    #[cfg(windows)]
    #[test]
    #[ignore = "run by test_c_d18b_launch_retries_without_breakaway_in_a_job_that_forbids_it in a child process"]
    fn launch_in_job_child() {
        use std::io::{BufRead, Write};
        let mut go = String::new();
        std::io::stdin().lock().read_line(&mut go).unwrap();
        let home = TempHome::new();
        let launched = launch_daemon(&daemon_exe()).unwrap();
        println!("LAUNCHED {} {}", launched.pid, launched.breakaway_denied);
        println!("HOME {}", home.dir.path().display());
        std::io::stdout().flush().unwrap();
        // 부모가 확인을 마치고 입력을 닫을 때까지 기다린다.
        let mut rest = Vec::new();
        std::io::stdin().lock().read_to_end(&mut rest).unwrap();
    }

    /// 닫으면 안의 프로세스를 모두 끝내는 작업 개체. 분리 허용 플래그는 주지 않는다.
    #[cfg(windows)]
    struct JobWithoutBreakaway(windows_sys::Win32::Foundation::HANDLE);

    #[cfg(windows)]
    impl JobWithoutBreakaway {
        fn new() -> Self {
            use windows_sys::Win32::System::JobObjects::{
                CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject,
            };
            // SAFETY: 구조체는 0으로 채워 쓰고, 정보 크기를 구조체 크기로 넘긴다.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                assert!(!job.is_null(), "CreateJobObjectW failed");
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                assert!(set != 0, "SetInformationJobObject failed");
                Self(job)
            }
        }

        fn assign(
            &self,
            child: &std::process::Child,
        ) {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
            // SAFETY: 자식 핸들은 `child`가 쥐고 있는 동안 유효하다.
            let assigned = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) };
            assert!(assigned != 0, "AssignProcessToJobObject failed");
        }
    }

    #[cfg(windows)]
    impl Drop for JobWithoutBreakaway {
        fn drop(&mut self) {
            // SAFETY: 핸들은 이 값이 만들어 한 번만 닫는다.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_c_d18b_launch_retries_without_breakaway_in_a_job_that_forbids_it() {
        use std::io::{BufRead, BufReader, Write};
        // 자식 시험의 데몬이 쓰는 파이프 이름을 이 프로세스의 다른 시험이 다투지 못하도록 자식이 사는 동안 임시 홈 잠금을 쥔다.
        let _lock = TempHome::new();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon_launch::tests::launch_in_job_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let job = JobWithoutBreakaway::new();
        job.assign(&child);
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"go\n").unwrap();
        stdin.flush().unwrap();

        let mut launched = None;
        let mut home = None;
        // 자식이 끝나며 남기는 출력이 닫힌 파이프로 가지 않도록, 읽는 쪽은 자식이 끝난 뒤에 버린다.
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        for line in output.by_ref() {
            let line = line.unwrap();
            // 시험 이름이 같은 줄에 먼저 찍히므로 줄 앞이 아니라 표지 뒤를 읽는다.
            if let Some(rest) = line.split("LAUNCHED ").nth(1) {
                let mut parts = rest.split_whitespace();
                let pid: u32 = parts.next().unwrap().parse().unwrap();
                launched = Some((pid, parts.next() == Some("true")));
            } else if let Some(path) = line.split("HOME ").nth(1) {
                home = Some(PathBuf::from(path));
            }
            if launched.is_some() && home.is_some() {
                break;
            }
        }
        let (pid, reported_denied) = launched.unwrap_or_else(|| panic!("child did not launch the daemon"));
        let info_path = home.unwrap().join(".paddock").join("native").join("daemon.json");

        // 데몬이 `--breakaway-denied`를 받아 떴다면 자기 `daemon.json`에 그렇게 적는다.
        let end = Instant::now() + Duration::from_secs(10);
        let mut recorded = None;
        while recorded.is_none() && Instant::now() < end {
            recorded = std::fs::read_to_string(&info_path)
                .ok()
                .and_then(|text| serde_json::from_str::<paddock_protocol::daemon_info::DaemonInfo>(&text).ok())
                .filter(|info| info.pid == pid);
            if recorded.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        kill_process(pid);
        drop(stdin);
        child.wait().unwrap();
        drop(output);
        let recorded = recorded.unwrap_or_else(|| panic!("daemon {pid} did not write daemon.json"));
        assert!(reported_denied, "launch must report that it fell back to no breakaway");
        assert!(recorded.breakaway_denied, "daemon must be started with --breakaway-denied");
        drop(job);
    }

    #[test]
    fn test_same_file_name_ignores_folder_case_and_deleted_mark() {
        assert!(same_file_name(Path::new("/a/b/paddock-daemon"), Path::new("/x/paddock-daemon")));
        assert!(same_file_name(Path::new("x/Paddock-Daemon.exe"), Path::new("paddock-daemon.exe")));
        assert!(same_file_name(Path::new("/a/paddock-daemon (deleted)"), Path::new("/a/paddock-daemon")));
        assert!(!same_file_name(Path::new("/a/other"), Path::new("/a/paddock-daemon")));
    }

    #[test]
    fn test_terminate_refuses_a_process_that_is_not_the_daemon() {
        // 이 시험 프로세스 자신은 데몬 실행 파일이 아니므로 끝내지 않고 오류다.
        let error = terminate_daemon(std::process::id(), Path::new("paddock-daemon")).expect_err("must refuse");
        assert_eq!(error.to_string(), "The process is not the terminal daemon");
    }

    #[test]
    fn test_sibling_daemon_exe_is_beside_the_app() {
        let exe = sibling_daemon_exe().unwrap();
        assert_eq!(exe.parent(), std::env::current_exe().unwrap().parent());
        assert!(exe.file_name().unwrap().to_string_lossy().starts_with("paddock-daemon"));
    }
}
