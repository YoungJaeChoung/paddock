//! `~/.paddock/native/daemon.json`: 실행 중인 데몬이 남기는 접속 정보. 앱은 이 파일로 데몬을 찾고 인증한다.
//!
//! 파일과 그 폴더는 현재 사용자만 읽고 쓴다(Unix 0600/0700, Windows 소유자 한 항목만 둔 DACL).
use std::io;
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

/// 데몬 접속 정보. 데몬만 쓴다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub endpoint: String,
    pub token: String,
    pub fingerprint: String,
    pub started: String,
    /// 앱이 데몬을 부모 작업 개체에서 분리하려다 거부당했는가. true면 부모 작업 개체가 끝날 때 데몬도 끝난다.
    #[serde(default)]
    pub breakaway_denied: bool,
}

impl DaemonInfo {
    /// 이 데몬을 다른 데몬과 구별하는 글자(pid와 시작 시각). 터미널 번호는 데몬마다 1부터 매겨지므로,
    /// 번호는 이 글자와 함께 있어야 어느 터미널인지 정해진다.
    pub fn identity(&self) -> String {
        format!("{}@{}", self.pid, self.started)
    }

    /// 이 정보를 쓴 데몬 프로세스가 아직 살아 있는가.
    pub fn is_running(&self) -> bool {
        pid_alive(self.pid)
    }
}

/// 네이티브 앱의 데이터 폴더(`~/.paddock/native`).
pub fn native_dir() -> PathBuf {
    std::env::home_dir().expect("a home directory is required").join(".paddock").join("native")
}

/// `daemon.json`의 경로.
pub fn info_path() -> PathBuf {
    native_dir().join("daemon.json")
}

/// 현재 사용자 전용 폴더를 만든다. 이미 있어도 권한을 다시 좁힌다.
///
/// `path`가 `native_dir()` 아래이면 그 사이의 폴더(`native_dir()` 포함)도 모두 전용으로 맞춘다.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let native = native_dir();
    let mut dirs = vec![path.to_path_buf()];
    if path.starts_with(&native) {
        dirs = path.ancestors().take_while(|ancestor| ancestor.starts_with(&native)).map(Path::to_path_buf).collect();
    }
    let mut result = Ok(());
    for dir in dirs {
        if result.is_ok() {
            result = restrict_to_owner(&dir, true);
        }
    }
    result
}

/// 접속 정보를 읽는다. 파일이 없거나 깨졌으면 None이다.
pub fn read_info() -> Option<DaemonInfo> {
    let text = std::fs::read_to_string(info_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// 접속 정보를 쓴다. 임시 파일에 다 쓴 뒤 바꿔치워, 읽는 쪽이 쓰다 만 파일을 보지 않는다.
pub fn write_info(info: &DaemonInfo) -> io::Result<()> {
    ensure_private_dir(&native_dir())?;
    let path = info_path();
    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(info).expect("daemon info serializes to JSON");
    write_owner_only(&temp, text.as_bytes())?;
    std::fs::rename(&temp, &path)
}

/// 접속 정보 파일이 `pid` 데몬의 것일 때만 지운다. 이미 없거나 다른 데몬의 것이면 건드리지 않는다.
///
/// 교체된 옛 데몬이 늦게 끝나면서 새 데몬이 쓴 파일을 지우지 않도록, 데몬은 자기 pid로만 지운다.
///
/// Examples
/// --------
/// | 파일의 pid | 인자 `pid` | 결과           |
/// |------------|------------|----------------|
/// | 없음       | 7          | 아무 일 없음   |
/// | 7          | 7          | 파일을 지움    |
/// | 8          | 7          | 파일을 그대로 둠 |
pub fn remove_info_owned_by(pid: u32) {
    if read_info().is_some_and(|info| info.pid == pid) {
        let _ = std::fs::remove_file(info_path());
    }
}

/// 접속 정보 파일이 `info`와 같은 데몬(`identity()` 일치)의 것일 때만 지운다.
///
/// pid만 보는 `remove_info_owned_by`와 달리 시작 시각까지 대조한다. 죽은 데몬의 pid를 다른 프로세스가 물려받았는지
/// 확인하는 동안 새 데몬이 같은 pid로 파일을 새로 썼다면 그 파일을 지우지 않는다.
pub fn remove_info_of(info: &DaemonInfo) {
    if read_info().is_some_and(|current| current.identity() == info.identity()) {
        let _ = std::fs::remove_file(info_path());
    }
}

/// 새 인증 토큰(무작위 32바이트의 16진수 64자).
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 소유자만 읽고 쓰는 파일을 쓴다. 임시 파일에 다 쓴 뒤 이름을 바꿔치워, 쓰는 도중 프로세스가 죽어도 쓰다 만 파일이 남지 않는다.
///
/// 폴더는 호출자가 `ensure_private_dir`로 먼저 만든다.
pub fn write_private_file(
    path: &Path,
    contents: &[u8],
) -> io::Result<()> {
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(".tmp");
    let temp = path.with_file_name(temp_name);
    write_owner_only(&temp, contents)?;
    std::fs::rename(&temp, path)
}

pub(crate) fn write_owner_only(
    path: &Path,
    contents: &[u8],
) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        file.write_all(contents)?;
    }
    #[cfg(windows)]
    {
        std::fs::write(path, contents)?;
    }
    restrict_to_owner(path, false)
}

pub(crate) fn restrict_to_owner(
    path: &Path,
    is_dir: bool,
) -> io::Result<()> {
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    };
    #[cfg(windows)]
    let result = crate::windows_security::restrict_to_current_user(path, is_dir);
    result
}

/// 프로세스가 살아 있는가. pid 0과 `i32` 범위 밖의 값은 프로세스를 가리키지 않으므로 살아 있지 않은 것으로 본다.
///
/// Unix에서 `kill(0, 0)`은 자기 프로세스 그룹 전체를, 음수는 다른 그룹을 가리켜 엉뚱하게 성공하기 때문이다.
fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    let alive = pid != 0 && i32::try_from(pid).is_ok_and(|pid| unsafe { libc::kill(pid, 0) == 0 });
    #[cfg(windows)]
    let alive = crate::windows_security::process_is_alive(pid);
    alive
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::TempHome;

    fn sample() -> DaemonInfo {
        DaemonInfo {
            pid: 4242,
            endpoint: "endpoint".into(),
            token: "token".into(),
            fingerprint: "fp".into(),
            started: "2026-10-15T00:00:00Z".into(),
            breakaway_denied: true,
        }
    }

    #[test]
    fn test_daemon_info_roundtrip_and_remove() {
        let _home = TempHome::new();
        assert_eq!(read_info(), None);
        write_info(&sample()).unwrap();
        assert_eq!(read_info(), Some(sample()));
        remove_info_owned_by(sample().pid);
        assert_eq!(read_info(), None);
        // 이미 없는 파일을 지우려 해도 오류가 아니다.
        remove_info_owned_by(sample().pid);
    }

    #[test]
    fn test_c_t4a_private_file_replaces_the_old_content_and_leaves_no_temporary_file() {
        let home = TempHome::new();
        let folder = home.dir.path().join(".paddock").join("native").join("private");
        ensure_private_dir(&folder).unwrap();
        let file = folder.join("a.json");
        write_private_file(&file, b"first").unwrap();
        write_private_file(&file, b"second").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "the temporary file was renamed away");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn test_remove_info_leaves_another_daemons_file() {
        let _home = TempHome::new();
        write_info(&sample()).unwrap();
        // 교체된 옛 데몬(다른 pid)이 늦게 끝나도 새 데몬의 접속 정보는 남는다.
        remove_info_owned_by(sample().pid + 1);
        assert_eq!(read_info(), Some(sample()));
    }

    #[test]
    fn test_remove_info_of_leaves_a_restarted_daemons_file_with_the_same_pid() {
        let _home = TempHome::new();
        let old = sample();
        let mut restarted = sample();
        restarted.started = "2026-10-16T00:00:00Z".into();
        write_info(&restarted).unwrap();
        remove_info_of(&old);
        assert_eq!(read_info(), Some(restarted.clone()));
        remove_info_of(&restarted);
        assert_eq!(read_info(), None);
    }

    #[test]
    fn test_identity_differs_when_a_new_daemon_reuses_the_pid_or_start_time() {
        let first = sample();
        let mut restarted = sample();
        restarted.started = "2026-10-16T00:00:00Z".into();
        let mut other_pid = sample();
        other_pid.pid += 1;
        assert_eq!(first.identity(), sample().identity());
        assert_ne!(first.identity(), restarted.identity());
        assert_ne!(first.identity(), other_pid.identity());
    }

    #[test]
    fn test_pid_zero_and_out_of_range_are_not_alive() {
        assert!(!pid_alive(0));
        assert!(!pid_alive(u32::MAX));
        assert!(!pid_alive(i32::MAX as u32 + 1));
    }

    #[test]
    fn test_new_token_is_64_hex_chars_and_unique() {
        let token = new_token();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(token, new_token());
    }

    #[test]
    fn test_is_running_follows_the_process() {
        let mut info = sample();
        info.pid = std::process::id();
        assert!(info.is_running());
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd").args(["/C", "exit"]).spawn().unwrap()
        } else {
            std::process::Command::new("true").spawn().unwrap()
        };
        info.pid = child.id();
        child.wait().unwrap();
        assert!(!info.is_running());
    }

    #[test]
    fn test_broken_daemon_info_reads_as_none() {
        let _home = TempHome::new();
        ensure_private_dir(&native_dir()).unwrap();
        std::fs::write(info_path(), "{broken").unwrap();
        assert_eq!(read_info(), None);
    }

    #[test]
    fn test_breakaway_denied_defaults_to_false() {
        let _home = TempHome::new();
        ensure_private_dir(&native_dir()).unwrap();
        std::fs::write(info_path(), r#"{"pid":1,"endpoint":"e","token":"t","fingerprint":"f","started":"s"}"#).unwrap();
        assert!(!read_info().unwrap().breakaway_denied);
    }

    #[test]
    fn test_c_t4a_native_dir_permissions() {
        let home = TempHome::new();
        write_info(&sample()).unwrap();
        let dir = native_dir();
        // 모든 파일은 홈 폴더 아래에 있다.
        assert!(dir.starts_with(home.dir.path()));
        assert!(info_path().starts_with(&dir));
        let nested = dir.join("logs");
        ensure_private_dir(&nested).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&nested), 0o700);
            assert_eq!(mode(&info_path()), 0o600);
        }
        #[cfg(windows)]
        {
            let sid = crate::windows_security::current_user_sid().unwrap();
            for path in [dir.clone(), nested.clone(), info_path()] {
                let sddl = crate::windows_security::object_sddl(&path.to_string_lossy()).unwrap();
                // 상속 항목이 없는 보호된 DACL에 현재 사용자 한 항목만 있다.
                assert!(sddl.contains("D:P"), "{sddl}");
                assert_eq!(sddl.matches("(A;").count(), 1, "{sddl}");
                assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
                assert!(sddl.contains(&sid), "{sddl}");
            }
        }
    }
}
