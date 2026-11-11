//! 터미널 위 오른쪽에 띄우는 안내 띠의 문구. 어느 안내를 보일지는 `Shared`의 상태 표시로 정한다.
use std::sync::atomic::Ordering;

use crate::terminal::grid::Shared;

/// 재생 뒤 대체 화면일 때 띄우는 안내.
const REPLAY_NOTICE_TEXT: &str = "Screen may differ \u{2014} press Ctrl+L";
const DISCONNECTED_TEXT: &str = "Disconnected from the terminal daemon";
const DETACHED_TEXT: &str = "This terminal was opened in another Paddock window";

/// 지금 띄울 안내 문구. 안내가 필요 없으면 None이다.
///
/// 한 번에 하나만 보이며, 연결이 끊긴 것이 가장 중요하고 셸 종료, 다른 창이 이어받음, 재생 뒤 화면 차이 순이다.
///
/// Examples
/// --------
/// | 상태                       | 결과                                         |
/// |----------------------------|----------------------------------------------|
/// | 연결 끊김, 셸은 살아 있었음 | `Disconnected from the terminal daemon`      |
/// | 셸 종료, 종료 코드 3        | `Shell exited with code 3`                   |
/// | 셸 종료, 코드를 모름        | `Shell exited (exit code unknown)`           |
/// | 다른 창이 이어받음          | `This terminal was opened in another Paddock window` |
pub fn banner_text(shared: &Shared) -> Option<String> {
    let exited = shared.exited.load(Ordering::Relaxed);
    if shared.disconnected.load(Ordering::Relaxed) && !exited {
        Some(DISCONNECTED_TEXT.to_string())
    } else if exited {
        let code = shared.exit_code.lock().unwrap().clone();
        Some(match code.as_deref() {
            Some(code) if code != "unknown" => format!("Shell exited with code {code}"),
            _ => "Shell exited (exit code unknown)".to_string(),
        })
    } else if shared.detached.load(Ordering::Relaxed) {
        Some(DETACHED_TEXT.to_string())
    } else if shared.replay_notice.load(Ordering::Relaxed) {
        Some(REPLAY_NOTICE_TEXT.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banner_priority_and_exit_code_wording() {
        let shared = Shared::default();
        assert_eq!(banner_text(&shared), None);
        shared.replay_notice.store(true, Ordering::Relaxed);
        assert_eq!(banner_text(&shared).as_deref(), Some("Screen may differ \u{2014} press Ctrl+L"));
        shared.detached.store(true, Ordering::Relaxed);
        assert_eq!(banner_text(&shared).as_deref(), Some("This terminal was opened in another Paddock window"));
        shared.exited.store(true, Ordering::Relaxed);
        *shared.exit_code.lock().unwrap() = Some("3".to_string());
        assert_eq!(banner_text(&shared).as_deref(), Some("Shell exited with code 3"));
        *shared.exit_code.lock().unwrap() = Some("unknown".to_string());
        assert_eq!(banner_text(&shared).as_deref(), Some("Shell exited (exit code unknown)"));
        // 셸이 끝나 연결도 끊겼으면 종료 안내가 먼저다. 셸이 살아 있는데 끊겼을 때만 연결 끊김이다.
        shared.disconnected.store(true, Ordering::Relaxed);
        assert_eq!(banner_text(&shared).as_deref(), Some("Shell exited (exit code unknown)"));
        shared.exited.store(false, Ordering::Relaxed);
        assert_eq!(banner_text(&shared).as_deref(), Some("Disconnected from the terminal daemon"));
    }
}
