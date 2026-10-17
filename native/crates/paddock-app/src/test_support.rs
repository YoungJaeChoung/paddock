//! 시험 보조: 임시 홈 폴더, 시험용 데몬 실행 파일 위치, 시험이 띄운 데몬 정리.
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use paddock_protocol::daemon_info::read_info;

static LOCK: Mutex<()> = Mutex::new(());

/// 시험 하나 동안 홈 폴더를 새 임시 폴더로 돌린다.
///
/// 홈 폴더 아래를 읽고 쓰는 시험(데몬을 띄우는 시험 포함)은 환경 변수를 공유하므로 한 번에 하나씩 돌고,
/// 버릴 때 이전 환경으로 되돌린다. 띄운 데몬은 이 환경을 물려받아 임시 폴더 안에서만 일한다.
pub struct TempHome {
    pub dir: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl TempHome {
    pub fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("temp dir");
        let mut home = Self { dir, saved: Vec::new(), _guard: guard };
        let path = home.dir.path().to_path_buf();
        home.set("HOME", Some(path.as_os_str()));
        home.set("USERPROFILE", Some(path.as_os_str()));
        home.set("XDG_RUNTIME_DIR", None);
        home
    }

    fn set(
        &mut self,
        key: &'static str,
        value: Option<&std::ffi::OsStr>,
    ) {
        self.saved.push((key, std::env::var_os(key)));
        // SAFETY: 환경 변수를 건드리는 시험은 모두 위의 잠금을 쥐고 있다.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            // SAFETY: 아직 잠금을 쥐고 있다.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

/// 시험 대상 데몬 실행 파일.
///
/// 시험 실행 파일 옆에 복사된 것(다른 기계에서 돌리는 교차 빌드 시험), 없으면 `target/<프로필>/`의 것을 쓴다.
/// 어느 쪽에도 없으면 한 번 빌드한다.
pub fn daemon_exe() -> PathBuf {
    let name = format!("paddock-daemon{}", std::env::consts::EXE_SUFFIX);
    let test_exe = std::env::current_exe().expect("test exe path");
    let deps_dir = test_exe.parent().expect("test exe folder");
    let candidates = [deps_dir.join(&name), deps_dir.parent().map(|dir| dir.join(&name)).unwrap_or_default()];
    let found = |candidates: &[PathBuf]| candidates.iter().find(|path| path.is_file()).cloned();
    match found(&candidates) {
        Some(path) => path,
        None => {
            let status = std::process::Command::new(env!("CARGO"))
                .args(["build", "-p", "paddock-daemon"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("run cargo build");
            assert!(status.success(), "building paddock-daemon failed");
            found(&candidates).expect("paddock-daemon was built")
        }
    }
}

/// 버릴 때 `daemon.json`의 데몬을 끝낸다. `TempHome`보다 나중에 만들어야 홈 폴더가 남아 있을 때 정리된다.
pub struct DaemonCleanup;

impl Drop for DaemonCleanup {
    fn drop(&mut self) {
        // 시험이 실패했으면 원인을 찾을 수 있도록 데몬과 앱의 기록 파일을 출력에 남긴다.
        if std::thread::panicking() {
            for name in ["daemon.log", "app.log"] {
                let path = paddock_protocol::daemon_info::native_dir().join("logs").join(name);
                eprintln!("--- {name} ---\n{}", std::fs::read_to_string(path).unwrap_or_default());
            }
        }
        if let Some(info) = read_info() {
            kill_process(info.pid);
        }
    }
}

/// 프로세스를 강제로 끝내고 사라질 때까지 기다린다. 이미 없으면 아무 일도 하지 않는다.
///
/// 다음 시험이 같은 이름의 파이프나 잠금을 쓰므로, 끝난 것이 확인될 때까지 기다린다.
pub fn kill_process(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    #[cfg(windows)]
    // SAFETY: 핸들은 이 블록 안에서만 쓰고 닫는다.
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
    let end = Instant::now() + Duration::from_secs(5);
    while process_is_alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    #[cfg(windows)]
    let alive = paddock_protocol::windows_security::process_is_alive(pid);
    alive
}
