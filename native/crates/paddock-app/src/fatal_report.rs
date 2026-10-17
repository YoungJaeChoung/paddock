//! 앱이 복구할 수 없는 오류(panic)로 멈출 때 그 이유를 문장으로 남긴다.
//!
//! 앱은 Windows에서 콘솔 없이 실행돼 오류 문장이 어디에도 보이지 않는다. 셰이더 파일이 없거나 그래픽 장치를 만들지 못하는 시작 실패가
//! 대표적이다 — 창이 뜨기 전에 프로세스가 말없이 사라지면 사용자는 원인을 알 수 없다. 그래서 오류를 앱 기록 파일에 남기고,
//! 주 스레드가 멈추는 경우에는 Windows 오류 창으로도 보인다.
use std::panic::{self, PanicHookInfo};

use crate::app_log;

/// 기록 파일에 남기는 오류 문장의 최대 글자 수. 긴 오류 사슬이 기록 파일을 채우지 않게 한다.
const MAX_SENTENCE_CHARS: usize = 600;

/// panic이 나면 오류 문장을 앱 기록 파일에 남기고, 주 스레드의 panic이면 `show_dialog`일 때 오류 창도 띄우도록 훅을 건다.
///
/// 원래 훅(표준 오류 출력)은 그대로 이어서 부른다. 창을 띄우지 않는 실행(자동 점검의 화면 밖 실행)은 `show_dialog`를 끈다 —
/// 사용자 화면에 오류 창이 뜨지 않게 하기 위해서다.
pub fn install(show_dialog: bool) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let sentence =
            sentence_of(&panic_message(info), info.location().map(|location| (location.file(), location.line())));
        app_log::event(&format!("fatal: {sentence}"));
        if show_dialog && std::thread::current().name() == Some("main") {
            show_error_box(&sentence);
        }
        previous(info);
    }));
}

/// panic이 들고 온 메시지 글. 문자열이 아닌 값으로 panic하면 알 수 없다는 문장이다.
fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown error".to_string()
    }
}

/// 사용자에게 보이는 오류 문장: 메시지 뒤에 위치를 붙이고 `MAX_SENTENCE_CHARS`에서 자른다.
///
/// Examples
/// --------
/// | message                      | location          | 결과                                          |
/// |------------------------------|-------------------|-----------------------------------------------|
/// | `Failed to open window`      | (`main.rs`, 90)   | `Paddock stopped: Failed to open window (main.rs:90)` |
/// | `boom`                       | None              | `Paddock stopped: boom`                       |
/// | 1000자 메시지                | None              | 앞 600자 뒤에 `…`                             |
pub fn sentence_of(
    message: &str,
    location: Option<(&str, u32)>,
) -> String {
    let mut sentence = format!("Paddock stopped: {}", message.trim());
    if sentence.chars().count() > MAX_SENTENCE_CHARS {
        sentence = sentence.chars().take(MAX_SENTENCE_CHARS).collect::<String>() + "…";
    }
    if let Some((file, line)) = location {
        sentence.push_str(&format!(" ({file}:{line})"));
    }
    sentence
}

/// Windows 오류 창을 띄운다. 창을 닫을 때까지 이 스레드가 멈춰 있다. 다른 운영체제에서는 아무것도 하지 않는다(콘솔이 있다).
fn show_error_box(sentence: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

        let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (text, caption) = (wide(sentence), wide("Paddock"));
        // SAFETY: 두 버퍼는 NUL로 끝나고 호출이 끝날 때까지 살아 있다.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = sentence;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sentence_names_the_message_and_the_place() {
        assert_eq!(
            sentence_of("Failed to open window", Some(("main.rs", 90))),
            "Paddock stopped: Failed to open window (main.rs:90)"
        );
        assert_eq!(sentence_of("boom\n", None), "Paddock stopped: boom");
    }

    #[test]
    fn test_long_message_is_cut() {
        let sentence = sentence_of(&"x".repeat(1000), None);
        assert!(sentence.ends_with('…'));
        assert!(sentence.chars().count() <= MAX_SENTENCE_CHARS + 1);
    }
}
