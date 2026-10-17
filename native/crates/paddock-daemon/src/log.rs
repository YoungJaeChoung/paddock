//! 데몬 기록 파일(`~/.paddock/native/logs/daemon.log`). 터미널에 붙고 멈춘 사건과 기동 환경만 남긴다.
//!
//! 입력한 글자·터미널 출력·인증 토큰은 어떤 경우에도 쓰지 않는다 — 기록 파일은 나중에 사용자가 공유할 수 있는 진단 자료다.
use std::io::Write;
use std::sync::Once;

use paddock_protocol::daemon_info::{ensure_private_dir, native_dir};

static LOGS_DIR_READY: Once = Once::new();

/// 기록 파일에 한 줄을 덧붙인다. 파일을 쓰지 못해도 데몬은 계속 일하므로 오류는 버린다.
pub fn event(message: &str) {
    let dir = native_dir().join("logs");
    LOGS_DIR_READY.call_once(|| {
        let _ = ensure_private_dir(&dir);
    });
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(dir.join("daemon.log")) {
        let _ = writeln!(file, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), message);
    }
}
