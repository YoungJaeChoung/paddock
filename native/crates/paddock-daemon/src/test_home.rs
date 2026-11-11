//! 시험 보조: 홈 폴더를 새 임시 폴더로 돌린다.
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

/// 시험 하나 동안 홈 폴더를 새 임시 폴더로 돌린다.
///
/// 홈 폴더 아래를 읽고 쓰는 시험은 환경 변수를 공유하므로 한 번에 하나씩 돌고, 버릴 때 이전 환경으로 되돌린다.
pub struct TempHome {
    _dir: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl TempHome {
    pub fn new() -> Self {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("temp dir");
        let mut home = Self { _dir: dir, saved: Vec::new(), _guard: guard };
        let path = home._dir.path().to_path_buf();
        for key in ["HOME", "USERPROFILE"] {
            home.saved.push((key, std::env::var_os(key)));
            // SAFETY: 환경 변수를 건드리는 시험은 모두 위의 잠금을 쥐고 있다.
            unsafe { std::env::set_var(key, path.as_os_str()) };
        }
        home
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
