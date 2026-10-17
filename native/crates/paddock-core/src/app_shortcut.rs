//! 앱이 터미널보다 먼저 가로채는 단축키: 분할, 새 터미널, 탭 이동, 사이드바 접기.
//!
//! 이 키들은 셸·프로그램에 보내면 안 된다(예: Ctrl+Shift+C가 Ctrl+C로 나가 실행 중인 프로그램을 끊는다).
//! 키 이름은 창 도구가 준 값 그대로다. 글자 키는 Shift가 눌려도 누른 키 자체의 이름(`shift`는 참)이다.
//! 그러나 Windows는 Shift가 눌린 숫자·기호 키를 바뀐 글자로 넘기고 `shift`를 거짓으로 비운다
//! (Ctrl+Shift+`는 `~`, Ctrl+Shift+5는 `%`). 그래서 두 형태를 모두 받는다.
//!
//! 이 단축키 중 Ctrl+Shift+C·`Ctrl+\`·Ctrl+←/→와 Ctrl+B는 기존 앱에 있는 키이고, Ctrl+Shift+T·W는 기존 앱에는 없는
//! 네이티브 추가 키다(터미널 앱의 관례이고 Ctrl+Shift 조합이라 셸의 키와 겹치지 않는다).
use crate::key_encoding::{KeyInput, encode_key};

/// 앱 단축키가 하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppShortcut {
    /// 선택한 터미널 옆 칸에 새 터미널을 연다.
    SplitRight,
    /// 선택한 터미널 아래 칸에 새 터미널을 연다.
    SplitDown,
    /// 지금 보는 자리(작업 폴더·묶음)에 새 터미널을 탭으로 연다.
    NewTerminalHere,
    /// 활성 탭(터미널 또는 파일 탭)을 닫는다. 평 Ctrl+W는 셸의 단어 지우기라 쓰지 않는다.
    CloseActiveTab,
    PreviousTab,
    NextTab,
    FirstTab,
    LastTab,
    ToggleSidebar,
}

/// 키 누름이 앱 단축키이면 그 일을, 아니면 None을 돌려준다. Alt가 눌린 키는 단축키가 아니다.
///
/// Examples
/// --------
/// | 키                    | 결과                       |
/// |-----------------------|----------------------------|
/// | Ctrl+Shift+C          | `SplitRight`               |
/// | Ctrl+\ · Ctrl+Shift+5 | `SplitRight`               |
/// | Ctrl+Shift+`          | `SplitDown`                |
/// | `%`·`~`(Ctrl, shift 거짓) | Windows가 Shift+5·Shift+`를 넘기는 형태. 위 둘과 같다 |
/// | Ctrl+`                | `NewTerminalHere`          |
/// | Ctrl+←/→/↑/↓          | 앞·뒤·처음·마지막 탭       |
/// | Ctrl+B                | `ToggleSidebar`            |
/// | Ctrl+Shift+T          | `NewTerminalHere`(네이티브 추가 키) |
/// | Ctrl+Shift+W          | `CloseActiveTab`(네이티브 추가 키) |
/// | Ctrl+C                | `None`                     |
pub fn app_shortcut(input: &KeyInput<'_>) -> Option<AppShortcut> {
    if input.control && !input.alt {
        match (input.key, input.shift) {
            ("c" | "5", true) | ("%", _) | ("\\", false) => Some(AppShortcut::SplitRight),
            ("`" | "~", true) | ("~", false) => Some(AppShortcut::SplitDown),
            ("`", false) | ("t", true) => Some(AppShortcut::NewTerminalHere),
            ("w", true) => Some(AppShortcut::CloseActiveTab),
            ("left", false) => Some(AppShortcut::PreviousTab),
            ("right", false) => Some(AppShortcut::NextTab),
            ("up", false) => Some(AppShortcut::FirstTab),
            ("down", false) => Some(AppShortcut::LastTab),
            ("b", false) => Some(AppShortcut::ToggleSidebar),
            _ => None,
        }
    } else {
        None
    }
}

/// 키 누름이 터미널(PTY)에 보낼 바이트. 앱 단축키는 보내지 않으므로 None이다.
///
/// 터미널 보기의 키 처리는 `encode_key`가 아니라 이 함수만 불러, 단축키가 어떤 경로로도 PTY에 닿지 않게 한다.
pub fn terminal_key_bytes(
    input: &KeyInput<'_>,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if app_shortcut(input).is_some() { None } else { encode_key(input, app_cursor) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(
        key: &'static str,
        control: bool,
        shift: bool,
    ) -> KeyInput<'static> {
        KeyInput { key, key_char: None, control, shift, alt: false }
    }

    #[test]
    fn test_app_shortcuts_are_recognised_by_key_and_modifiers() {
        let table = [
            (press("c", true, true), AppShortcut::SplitRight),
            (press("\\", true, false), AppShortcut::SplitRight),
            (press("5", true, true), AppShortcut::SplitRight),
            (press("`", true, true), AppShortcut::SplitDown),
            (press("`", true, false), AppShortcut::NewTerminalHere),
            (press("t", true, true), AppShortcut::NewTerminalHere),
            (press("w", true, true), AppShortcut::CloseActiveTab),
            (press("left", true, false), AppShortcut::PreviousTab),
            (press("right", true, false), AppShortcut::NextTab),
            (press("up", true, false), AppShortcut::FirstTab),
            (press("down", true, false), AppShortcut::LastTab),
            (press("b", true, false), AppShortcut::ToggleSidebar),
        ];
        for (input, expected) in table {
            assert_eq!(app_shortcut(&input), Some(expected), "{input:?}");
        }
    }

    /// GPUI Windows가 Shift가 눌린 숫자·기호 키를 넘기는 형태: 키 이름이 바뀐 글자(미국 배열 기준)로 오고 `shift`는 거짓이다.
    /// (`gpui-pre-windows` keyboard.rs의 `get_keystroke_key`: VK_OEM_3 → `~`, VK_5 → `%`, VK_OEM_5 → `|`)
    /// 글자 키(c·t·w·b)는 바뀌지 않아 이름 그대로 `shift`가 참이다.
    #[test]
    fn test_gpui_windows_delivers_shifted_digit_and_oem_keys_as_the_shifted_character_with_shift_cleared() {
        let converted =
            [(press("~", true, false), AppShortcut::SplitDown), (press("%", true, false), AppShortcut::SplitRight)];
        for (input, expected) in converted {
            assert_eq!(app_shortcut(&input), Some(expected), "{input:?}");
        }
        // 변환된 글자라도 Ctrl이 없으면 그냥 글자다.
        assert_eq!(app_shortcut(&press("~", false, false)), None);
        assert_eq!(app_shortcut(&press("%", false, false)), None);
        // Ctrl+Shift+\는 `|`로 오며 분할 키가 아니다(Ctrl+\만 분할이다).
        assert_eq!(app_shortcut(&press("|", true, false)), None);
    }

    #[test]
    fn test_keys_that_are_not_app_shortcuts_stay_with_the_terminal() {
        // Shift·Alt가 더해진 방향키와 Shift 없는 Ctrl+C·글자 키는 셸·프로그램의 것이다.
        let table = [
            press("c", true, false),
            press("left", true, true),
            press("b", false, false),
            press("b", true, true),
            press("5", true, false),
            KeyInput { key: "left", key_char: None, control: true, shift: false, alt: true },
            KeyInput { key: "b", key_char: None, control: true, shift: false, alt: true },
        ];
        for input in table {
            assert_eq!(app_shortcut(&input), None, "{input:?}");
        }
    }

    #[test]
    fn test_c_d51_closing_the_active_tab_uses_ctrl_shift_w_only_so_plain_ctrl_w_still_deletes_a_word_in_the_shell() {
        // 파일 탭이 활성이어도 터미널이 활성이어도 같은 키가 활성 탭을 닫고, 그 키는 PTY로 가지 않는다.
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("w", true, true), app_cursor), None);
            assert_eq!(terminal_key_bytes(&press("w", true, false), app_cursor), Some(vec![0x17]));
        }
        assert_eq!(app_shortcut(&press("w", true, false)), None);
    }

    #[test]
    fn test_c_d38_split_shortcuts_never_reach_the_pty() {
        let shortcuts = [
            press("c", true, true),
            press("\\", true, false),
            press("5", true, true),
            press("`", true, true),
            press("`", true, false),
            press("left", true, false),
            press("right", true, false),
            press("up", true, false),
            press("down", true, false),
            press("b", true, false),
        ];
        for input in shortcuts {
            for app_cursor in [false, true] {
                assert_eq!(terminal_key_bytes(&input, app_cursor), None, "{input:?} app_cursor={app_cursor}");
            }
        }
        // 막지 않는 키는 그대로 인코딩된다: Ctrl+C는 0x03이고 일반 방향키는 이스케이프 열이다.
        assert_eq!(terminal_key_bytes(&press("c", true, false), false), Some(vec![0x03]));
        assert_eq!(terminal_key_bytes(&press("left", false, false), false), Some(b"\x1b[D".to_vec()));
        // 수정 키를 더한 방향키(Ctrl+Shift+←)는 막지 않는다.
        assert_eq!(terminal_key_bytes(&press("left", true, true), false), Some(b"\x1b[D".to_vec()));
    }
}
