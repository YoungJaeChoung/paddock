//! Paddock 앱과 터미널 데몬 사이의 연결 계약: 프레임, 메시지,
//! `daemon.json`, 단일 실행 잠금, 엔드포인트 이름, 데몬 지문, 상대 사용자 확인.
//!
//! 이 크레이트의 운영체제 호출은 연결 계약에 필요한 것(파일 권한, 엔드포인트 생성, 상대 자격)에 한정한다.
//! 그래서 앱과 데몬이 같은 규칙을 쓰게 된다.

pub mod daemon_info;
pub mod daemon_lock;
pub mod endpoint;
pub mod fingerprint;
pub mod frame;
pub mod message;
pub mod peer;
pub mod shared_config;
#[cfg(windows)]
pub mod windows_security;

#[cfg(test)]
pub(crate) mod test_home {
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    /// 시험 하나 동안 홈 폴더를 새 임시 폴더로 돌린다.
    ///
    /// 홈 폴더 아래를 읽고 쓰는 시험은 환경 변수를 공유하므로 한 번에 하나씩 돌고,
    /// 버릴 때 이전 환경으로 되돌린다.
    pub struct TempHome {
        pub dir: tempfile::TempDir,
        saved: Vec<(&'static str, Option<OsString>)>,
        _guard: MutexGuard<'static, ()>,
    }

    impl TempHome {
        /// 새 임시 폴더를 홈으로 삼는다. 다른 `TempHome`이 살아 있으면 그것이 버려질 때까지 기다린다.
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

        /// 환경 변수 하나를 바꾼다(`None`이면 지운다). 버릴 때 원래 값으로 돌아간다.
        pub fn set(
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
}
