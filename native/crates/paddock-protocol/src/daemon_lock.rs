//! 데몬 단일 실행 잠금(`~/.paddock/native/daemon.lock`).
//!
//! 데몬은 기동 순서(살아 있는 데몬 확인 → 옛 소켓 덮어쓰기 → 리스너 생성 → `daemon.json` 쓰기) 전체를 이 잠금 안에서 하고,
//! 끝날 때까지 쥔다. 동시에 뜬 두 데몬 중 하나만 잠금을 얻으므로, 살아 있는 데몬의 소켓을 다른 데몬이 덮어쓰는 일이 없다.
//! 잠금은 프로세스가 죽으면 운영체제가 풀어 주어 낡은 잠금 파일이 남지 않는다.
use std::fs::File;
use std::io;

use crate::daemon_info::{ensure_private_dir, native_dir, write_owner_only};

/// 쥐고 있는 동안 다른 데몬이 잠금을 얻지 못하게 하는 값. 버리면 풀린다.
#[derive(Debug)]
pub struct DaemonLock {
    _file: File,
}

impl DaemonLock {
    /// 잠금을 얻으면 `Some`, 다른 데몬이 쥐고 있으면 `None`이다. 기다리지 않는다.
    ///
    /// Examples
    /// --------
    /// | 상태                 | 결과   |
    /// |----------------------|--------|
    /// | 잠금을 쥔 쪽이 없음  | `Some` |
    /// | 다른 쪽이 쥐고 있음  | `None` |
    pub fn try_acquire() -> io::Result<Option<DaemonLock>> {
        ensure_private_dir(&native_dir())?;
        let path = native_dir().join("daemon.lock");
        let usable = if path.exists() { Ok(true) } else { create_lock_file(&path) };
        usable
            .and_then(|usable| if usable { open_exclusive(&path) } else { Ok(None) })
            .map(|opened| opened.map(|file| DaemonLock { _file: file }))
    }
}

/// 잠금 파일을 만들어 소유자 전용으로 맞춘다. 다른 데몬이 같은 순간에 이 파일을 만들고 쥐고 있어 건드릴 수 없으면 `false`다.
///
/// 파일이 아직 없을 때 두 데몬이 동시에 뜨면 한쪽이 만든 파일을 쥐는 동안 다른 쪽의 권한 설정이 공유 위반으로 실패한다.
/// 그 데몬이 잠금을 쥔 쪽이므로 이쪽은 "잠금을 못 얻음"으로 물러난다.
fn create_lock_file(path: &std::path::Path) -> io::Result<bool> {
    match write_owner_only(path, b"") {
        Ok(()) => Ok(true),
        Err(error) if is_sharing_violation(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// 다른 프로세스가 이 파일을 열고 있어 공유할 수 없을 때의 오류인가. Windows에만 있는 오류다.
fn is_sharing_violation(error: &io::Error) -> bool {
    /// 다른 프로세스가 이미 이 파일을 열고 있어 공유할 수 없을 때의 오류 번호.
    const ERROR_SHARING_VIOLATION: i32 = 32;
    cfg!(windows) && error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
}

#[cfg(unix)]
fn open_exclusive(path: &std::path::Path) -> io::Result<Option<File>> {
    use std::os::fd::AsRawFd;
    let file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    // 파일 서술자가 닫히거나 프로세스가 죽으면 풀리는 권고 잠금이다.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if locked {
        Ok(Some(file))
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(error) }
    }
}

#[cfg(windows)]
fn open_exclusive(path: &std::path::Path) -> io::Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    // 공유 모드 0으로 열면 파일을 연 동안 다른 누구도 열 수 없다. 프로세스가 죽으면 핸들이 닫혀 풀린다.
    match std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if is_sharing_violation(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::TempHome;

    #[test]
    fn test_lock_is_exclusive_until_dropped() {
        let _home = TempHome::new();
        let first = DaemonLock::try_acquire().unwrap();
        assert!(first.is_some());
        assert!(DaemonLock::try_acquire().unwrap().is_none(), "second holder must be refused");
        drop(first);
        assert!(DaemonLock::try_acquire().unwrap().is_some(), "lock must be free after the holder is dropped");
    }

    /// 다른 데몬이 만든 직후의 잠금 파일을 쥐고 있을 때, 이쪽의 파일 만들기는 오류가 아니라 "못 얻음"이다.
    #[cfg(windows)]
    #[test]
    fn test_creating_a_lock_file_held_by_another_daemon_is_not_an_error() {
        use std::os::windows::fs::OpenOptionsExt;
        let _home = TempHome::new();
        ensure_private_dir(&native_dir()).unwrap();
        let path = native_dir().join("daemon.lock");
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(!create_lock_file(&path).unwrap());
    }

    #[test]
    fn test_creating_a_lock_file_succeeds_when_nobody_holds_it() {
        let _home = TempHome::new();
        ensure_private_dir(&native_dir()).unwrap();
        assert!(create_lock_file(&native_dir().join("daemon.lock")).unwrap());
    }
}
