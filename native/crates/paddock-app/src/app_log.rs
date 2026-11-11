//! 앱 기록 파일(`~/.paddock/native/logs/app.log`). 데몬 실행·연결·지문 불일치·교체·재접속 시간만 남긴다.
//!
//! 입력한 글자·터미널 출력·인증 토큰은 어떤 경우에도 쓰지 않는다 — 기록 파일은 나중에 사용자가 공유할 수 있는 진단 자료다.
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use paddock_protocol::daemon_info::{ensure_private_dir, native_dir};

/// 기록 파일에 한 줄을 덧붙인다. 파일을 쓰지 못해도 앱은 계속 일하므로 오류는 버린다.
pub fn event(message: &str) {
    let dir = native_dir().join("logs");
    // 폴더가 없을 때만 전용 권한으로 만든다.
    let file = open_log(&dir).or_else(|_| ensure_private_dir(&dir).and_then(|()| open_log(&dir)));
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), message);
    }
}

fn open_log(dir: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(dir.join("app.log"))
}
