//! 게이트웨이 기록 파일(`~/.paddock/native/logs/remote.log`, 소유자 전용 0600).
//!
//! 기록은 `Event`로만 쓴다. 이벤트의 필드는 기기 이름·로그인·횟수·거부 이유처럼 남겨도 되는 값뿐이라,
//! 입력·출력·기기 토큰·페어링 코드·쿠키는 기록 함수에 넘길 길이 없다. 자유 글(기기 이름·로그인)은 제어 문자를 빼고 길이를 줄여 쓴다.
use std::io::Write;
use std::sync::Once;

use paddock_protocol::daemon_info::{ensure_private_dir, native_dir};

static LOGS_DIR_READY: Once = Once::new();

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
        n_denials: u32,
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
            Event::DenialsSuppressed { n_denials } => format!("denied: {n_denials} more requests not logged"),
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
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(dir.join("remote.log")) {
        let _ = writeln!(file, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), event.text());
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
}
