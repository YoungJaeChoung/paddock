//! macOS: `ps`로 터미널 앞쪽 프로세스를 찾고, libc 조회로 셸의 현재 폴더와 앞쪽 프로세스의 명령줄·환경·시작 시각을 읽는다.
//!
//! macOS에는 `/proc`이 없다. 앞쪽 프로세스 찾기는 core의 `mac_processes` 규칙을 따라 `ps`를 터미널 수와 상관없이 두 번만 실행한다.
//! 나머지는 프로세스를 더 띄우지 않고 커널 조회로 읽는다:
//! - 현재 폴더·시작 시각: `proc_pidinfo`(`PROC_PIDVNODEPATHINFO`·`PROC_PIDTBSDINFO`)
//! - 명령줄·환경: `sysctl(KERN_PROCARGS2)`. `ps`의 명령줄은 인자 사이 공백과 인자 안 공백을 구별하지 못해, 읽을 수 있으면 이쪽을 쓴다.
//!
//! 다른 사용자로 실행된 프로그램(sudo 등)은 `KERN_PROCARGS2`가 거부된다. 그때 명령줄은 `ps` 것을 쓰고 환경(계정)은 모름으로 둔다.
use std::collections::BTreeMap;
use std::process::Command;

use paddock_core::agent_account::{AccountEnv, Runtime, parse_environ};
use paddock_core::mac_processes::{MacForeground, read_mac_foregrounds};

use super::{ProbeTarget, Reading};

pub fn read(targets: &[ProbeTarget]) -> BTreeMap<u32, Reading> {
    let shell_pids: Vec<u32> = targets.iter().map(|target| target.shell_pid).collect();
    let mut run = |args: &[&str]| {
        Command::new("ps")
            .args(args)
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    };
    let foregrounds = read_mac_foregrounds(&shell_pids, &mut run);
    targets
        .iter()
        .filter_map(|target| {
            let foreground = foregrounds.get(&target.shell_pid)?.as_ref()?;
            Some((target.id, reading_of(target.shell_pid, foreground)))
        })
        .collect()
}

fn reading_of(
    shell_pid: u32,
    foreground: &MacForeground,
) -> Reading {
    let args = process_args(foreground.pid);
    let argv = args.as_ref().map(|args| args.argv.clone()).filter(|argv| !argv.is_empty());
    let env: Option<AccountEnv> = args.map(|args| parse_environ(&args.environ, '\0'));
    let started_at = started_at_of(foreground.pid);
    Reading {
        cwd: cwd_of(shell_pid),
        argv: argv.unwrap_or_else(|| foreground.argv.clone()),
        env,
        runtime: Runtime::Native,
        distribution: String::new(),
        pid: Some(foreground.pid),
        started_at,
        start_mark: started_at,
        session_record: None,
    }
}

/// 프로세스의 현재 폴더. 읽지 못하면 None이다.
fn cwd_of(pid: u32) -> Option<String> {
    // SAFETY: `proc_vnodepathinfo`는 정수·바이트 배열뿐인 구조체라 0으로 채운 값이 유효하다.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: 버퍼는 `size` 바이트의 살아 있는 구조체이고, 커널은 그 크기까지만 쓴다.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    (written == size)
        .then(|| {
            // 경로는 NUL로 끝나는 `c_char` 1024칸이다.
            let bytes: Vec<u8> = info.pvi_cdir.vip_path.iter().flatten().map(|byte| *byte as u8).collect();
            let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).into_owned()
        })
        .filter(|path| !path.is_empty())
}

/// 프로세스가 시작된 시각(ms, 1970년 기준). 읽지 못하면 None이다.
fn started_at_of(pid: u32) -> Option<u64> {
    // SAFETY: `proc_bsdinfo`는 정수·바이트 배열뿐인 구조체라 0으로 채운 값이 유효하다.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: 버퍼는 `size` 바이트의 살아 있는 구조체이고, 커널은 그 크기까지만 쓴다.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (written == size && info.pbi_start_tvsec > 0).then(|| info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000)
}

/// 프로세스의 명령줄과 환경 원문.
#[derive(Debug, PartialEq, Eq)]
struct ProcessArgs {
    argv: Vec<String>,
    /// 환경 변수 `이름=값`을 NUL로 이은 글.
    environ: String,
}

/// `sysctl(KERN_PROCARGS2)`로 프로세스의 명령줄과 환경을 읽는다. 다른 사용자의 프로세스 등 거부되면 None이다.
fn process_args(pid: u32) -> Option<ProcessArgs> {
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    // SAFETY: 크기만 묻는 호출이라 출력 버퍼는 null이고, `name`·`size`는 호출 동안 살아 있다.
    let sized = unsafe { libc::sysctl(name.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) };
    let mut result = None;
    if sized == 0 && size > 0 {
        // 두 호출 사이에 환경이 커질 수 있어 여유를 둔다.
        let mut buffer = vec![0u8; size + size / 2 + 1024];
        let mut capacity: libc::size_t = buffer.len();
        // SAFETY: 버퍼는 `capacity` 바이트이고 커널은 그 크기까지만 쓰며 실제 쓴 길이를 `capacity`에 돌려준다.
        let read = unsafe {
            libc::sysctl(name.as_mut_ptr(), 3, buffer.as_mut_ptr().cast(), &mut capacity, std::ptr::null_mut(), 0)
        };
        if read == 0 {
            result = parse_process_args(&buffer[..capacity.min(buffer.len())]);
        }
    }
    result
}

/// `KERN_PROCARGS2` 원문을 명령줄과 환경으로 나눈다. 모양이 맞지 않으면 None이다.
///
/// 원문은 `argc`(4바이트) → 실행 파일 경로 → NUL 여백 → 인자 `argc`개(각각 NUL로 끝) → 환경 변수들(각각 NUL로 끝) 순서다.
///
/// Examples
/// --------
/// | 원문(NUL은 `\0`)                                         | 결과                                         |
/// |----------------------------------------------------------|----------------------------------------------|
/// | `2` `/bin/sh\0\0\0` `sh\0-c\0` `HOME=/h\0A=b\0`          | argv `["sh", "-c"]`, environ `HOME=/h\0A=b`  |
/// | `argc`보다 짧은 원문                                     | None                                         |
fn parse_process_args(bytes: &[u8]) -> Option<ProcessArgs> {
    let n_args = usize::try_from(i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?)).ok()?;
    let mut rest = bytes.get(4..)?;
    // 실행 파일 경로를 건너뛴다.
    let path_end = rest.iter().position(|byte| *byte == 0)?;
    rest = &rest[path_end..];
    // 경로 뒤 NUL 여백을 건너뛴다.
    let start = rest.iter().position(|byte| *byte != 0)?;
    rest = &rest[start..];
    let mut argv = Vec::with_capacity(n_args);
    for _ in 0..n_args {
        let end = rest.iter().position(|byte| *byte == 0)?;
        argv.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = &rest[end + 1..];
    }
    let environ = String::from_utf8_lossy(rest).trim_end_matches('\0').to_string();
    Some(ProcessArgs { argv, environ })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// 프로세스가 `sleep`에 들어가 명령줄을 읽을 수 있을 때까지 기다린다(`exec` 직후에는 아직 이전 이미지일 수 있다).
    fn wait_for_args(pid: u32) -> ProcessArgs {
        let mut found = None;
        for _ in 0..100 {
            found =
                process_args(pid).filter(|args| args.argv.first().is_some_and(|program| program.ends_with("sleep")));
            if found.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        found.expect("the child's arguments are readable")
    }

    #[test]
    fn test_procargs2_is_split_into_argv_and_environment() {
        let mut bytes = 2i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/bin/sh\0\0\0sh\0-c\0HOME=/h\0A=b\0");
        let args = parse_process_args(&bytes).expect("well formed");
        assert_eq!(args.argv, vec!["sh", "-c"]);
        assert_eq!(args.environ, "HOME=/h\0A=b");
        assert!(parse_process_args(&bytes[..6]).is_none());
        assert!(parse_process_args(&[]).is_none());
    }

    #[test]
    fn test_the_real_process_args_keep_spaces_inside_an_argument_and_the_account_environment() {
        // 운영체제 기본 프로그램(`/bin/sleep` 등)은 보호되어 환경을 읽을 수 없다. 사본을 띄우면 일반 프로그램처럼 읽힌다.
        // (`sleep`은 보호 표시가 붙은 채 복사해도 실행되지만, bash 같은 일부는 다시 서명해야 한다.)
        let dir = tempfile::tempdir().expect("temp dir");
        let copy = dir.path().join("sleep");
        std::fs::copy("/bin/sleep", &copy).expect("copy sleep");
        let mut child = Command::new(&copy)
            .arg("30")
            .env("CODEX_HOME", "/tmp/codex home")
            .env("CLAUDE_CONFIG_DIR", "/tmp/claude")
            .stdout(Stdio::null())
            .spawn()
            .expect("sleep starts");
        let args = wait_for_args(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(&args.argv[1..], ["30"]);
        let env = parse_environ(&args.environ, '\0');
        assert_eq!(env.codex_home.as_deref(), Some("/tmp/codex home"));
        assert_eq!(env.claude_config_dir.as_deref(), Some("/tmp/claude"));
    }

    #[test]
    fn test_the_folder_and_start_time_of_a_real_process_are_read() {
        let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let folder = std::fs::canonicalize(std::env::temp_dir()).expect("temp dir");
        let mut child = Command::new("sleep").arg("30").current_dir(&folder).spawn().expect("sleep starts");
        let cwd = cwd_of(child.id());
        let started = started_at_of(child.id()).expect("a start time");
        let _ = child.kill();
        let _ = child.wait();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        assert_eq!(cwd.as_deref(), folder.to_str());
        assert!(started + 1000 >= before && started <= now + 1000, "started {started}, window {before}..{now}");
    }

    #[test]
    fn test_the_probe_reads_this_process_like_a_shell_without_a_terminal() {
        // 이 시험 프로세스를 "셸"로 본다. 제어 터미널이 없으면 앞쪽 그룹이 없어 프로세스 자신이 앞쪽이다.
        let target = ProbeTarget { id: 1, shell_pid: std::process::id(), wsl: None };
        let readings = read(&[target]);
        let reading = readings.get(&1).expect("own process is readable");
        assert!(!reading.argv.is_empty());
        assert_eq!(
            reading.cwd.as_deref(),
            std::env::current_dir()
                .ok()
                .and_then(|dir| std::fs::canonicalize(dir).ok())
                .as_deref()
                .and_then(|dir| dir.to_str())
        );
        assert!(reading.env.is_some());
        assert!(reading.started_at.is_some() && reading.started_at == reading.start_mark);
        assert!(reading.pid.is_some());
    }

    #[test]
    fn test_a_missing_process_is_left_out() {
        let target = ProbeTarget { id: 7, shell_pid: u32::MAX - 1, wsl: None };
        assert!(read(&[target]).is_empty());
    }
}
