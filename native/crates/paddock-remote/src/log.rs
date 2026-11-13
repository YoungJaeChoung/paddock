//! 게이트웨이 기록 파일(`~/.paddock/native/logs/remote.log`, 소유자 전용 0600).
//!
//! 기록은 `Event`로만 쓴다. 이벤트의 필드는 기기 이름·로그인·횟수·거부 이유처럼 남겨도 되는 값뿐이라,
//! 입력·출력·기기 토큰·페어링 코드·쿠키는 기록 함수에 넘길 길이 없다. 자유 글(기기 이름·로그인)은 제어 문자를 빼고 길이를 줄여 쓴다.
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, Once};

use paddock_protocol::daemon_info::{ensure_private_dir, native_dir};

static LOGS_DIR_READY: Once = Once::new();

/// 줄이 섞이지 않고 크기 확인과 돌리기가 겹치지 않도록 기록 쓰기를 한 번에 하나씩 하게 하는 잠금.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 기록 파일 크기 규칙.
pub struct LogRule;

impl LogRule {
    /// 이 크기를 넘으면 `remote.log`를 `remote.log.1`로 돌리고(옛 `.1`은 버린다) 새 파일에 쓴다. 디스크는 이 크기의 두 배 안에서 멈춘다.
    pub const MAX_BYTES: u64 = 1024 * 1024;
}

/// 기록할 수 있는 사건.
pub enum Event<'a> {
    Started {
        port: u16,
        host: &'a str,
        login: &'a str,
    },
    Stopped {
        reason: &'a str,
    },
    DaemonLinked {
        daemon_pid: u32,
        compatible: bool,
    },
    PairingOpened,
    /// 기기 목록 파일을 쓰지 못했다. `error_kind`는 오류 종류 이름이다.
    DeviceSaveFailed {
        error_kind: &'a str,
    },
    PairingSucceeded {
        device: &'a str,
    },
    PairingFailed {
        n_failures: u32,
        closed: bool,
    },
    PairingRejectedNotOpen,
    DeviceConnected {
        device: &'a str,
        login: &'a str,
    },
    DeviceDisconnected {
        device: &'a str,
    },
    DeviceRevoked {
        device: &'a str,
    },
    /// 게이트가 요청을 거부했다. `reason`은 거부 이유 이름이다.
    Denied {
        route: &'a str,
        reason: &'a str,
    },
    /// 거부 기록이 한도를 넘어 건너뛴 건수.
    DenialsSuppressed {
        n_events: u32,
    },
    ControlRefused,
}

impl Event<'_> {
    /// 기록 한 줄의 글.
    pub fn text(&self) -> String {
        match self {
            Event::Started { port, host, login } => {
                format!("gateway started (port {port}, host {}, login {})", clean(host), clean(login))
            }
            Event::Stopped { reason } => format!("gateway stopped: {}", clean(reason)),
            Event::DaemonLinked { daemon_pid, compatible } => {
                format!("daemon linked (pid {daemon_pid}, protocol compatible: {compatible})")
            }
            Event::PairingOpened => "pairing opened".to_string(),
            Event::PairingSucceeded { device } => format!("pairing succeeded: device {}", clean(device)),
            Event::PairingFailed { n_failures, closed } => {
                format!("pairing failed (failures {n_failures}, window closed: {closed})")
            }
            Event::PairingRejectedNotOpen => "pairing rejected: no open window".to_string(),
            Event::DeviceConnected { device, login } => {
                format!("device connected: {} (login {})", clean(device), clean(login))
            }
            Event::DeviceDisconnected { device } => format!("device disconnected: {}", clean(device)),
            Event::DeviceRevoked { device } => format!("device revoked: {}", clean(device)),
            Event::Denied { route, reason } => format!("denied {route}: {reason}"),
            Event::DenialsSuppressed { n_events } => format!("{n_events} more rejected requests not logged"),
            Event::DeviceSaveFailed { error_kind } => format!("device list could not be saved: {}", clean(error_kind)),
            Event::ControlRefused => "control connection refused: other user".to_string(),
        }
    }
}

/// 자유 글에서 제어 문자를 빼고 80자로 줄인다. 한 줄 기록에 줄바꿈이나 가짜 줄이 끼지 않게 한다.
fn clean(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_control()).take(80).collect()
}

/// 기록 파일에 한 줄을 덧붙인다. 파일을 쓰지 못해도 게이트웨이는 계속 일하므로 오류는 버린다.
pub fn write(event: &Event<'_>) {
    let dir = native_dir().join("logs");
    LOGS_DIR_READY.call_once(|| {
        let _ = ensure_private_dir(&dir);
    });
    let line = format!("{} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), event.text());
    append(&dir, &line, LogRule::MAX_BYTES);
}

/// `dir/remote.log`에 완성된 한 줄을 한 번의 쓰기로 덧붙인다. 파일이 `max_bytes` 이상이면 먼저 `remote.log.1`로 돌린다.
fn append(
    dir: &Path,
    line: &str,
    max_bytes: u64,
) {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = dir.join("remote.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() >= max_bytes) {
        let _ = std::fs::rename(&path, dir.join("remote.log.1"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_text_cannot_add_lines_or_grow_without_bound() {
        let event = Event::DeviceConnected { device: "evil\nfake line", login: &"x".repeat(500) };
        let text = event.text();
        assert!(!text.contains('\n'));
        assert!(text.len() < 200, "{}", text.len());
    }

    #[test]
    fn test_file_is_rotated_once_it_passes_the_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..100 {
            append(dir.path(), &format!("line {index:03}\n"), 200);
        }
        let current = std::fs::metadata(dir.path().join("remote.log")).unwrap().len();
        let rotated = std::fs::metadata(dir.path().join("remote.log.1")).unwrap().len();
        assert!(current <= 200 + 9 && rotated <= 200 + 9, "{current} {rotated}");
        let text = std::fs::read_to_string(dir.path().join("remote.log")).unwrap();
        assert!(text.contains("line 099"), "newest line is in the current file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.path().join("remote.log")).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_concurrent_lines_stay_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let threads: Vec<_> = (0..8)
            .map(|id| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        append(&path, &format!("thread-{id} {}\n", "x".repeat(300)), u64::MAX);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let text = std::fs::read_to_string(dir.path().join("remote.log")).unwrap();
        assert_eq!(text.lines().count(), 8 * 200);
        assert!(text.lines().all(|line| line.len() == 309 && line.ends_with(&"x".repeat(300))), "a line was torn");
    }
}
