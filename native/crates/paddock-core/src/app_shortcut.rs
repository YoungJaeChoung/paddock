//! 앱이 터미널보다 먼저 가로채는 단축키: 분할, 새 터미널, 탭 이동, 사이드바 접기, 폴더 검색, 터미널 안 찾기.
//!
//! 이 키들은 셸·프로그램에 보내면 안 된다(예: Ctrl+Shift+C가 Ctrl+C로 나가 실행 중인 프로그램을 끊는다).
//! 키 이름은 창 도구가 준 값 그대로다. 글자 키는 Shift가 눌려도 누른 키 자체의 이름(`shift`는 참)이다.
//! 그러나 Windows는 Shift가 눌린 숫자·기호 키를 바뀐 글자로 넘기고 `shift`를 거짓으로 비운다
//! (Ctrl+Shift+`는 `~`, Ctrl+Shift+5는 `%`). 그래서 두 형태를 모두 받는다.
//!
//! 이 단축키 중 Ctrl+Shift+C·`Ctrl+\`·Ctrl+←/→·Ctrl+B와 폴더 검색의 Ctrl+Shift+F는 기존 앱에 있는 키이고, Ctrl+Shift+T·W는 기존 앱에는 없는
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
    /// 폴더 검색을 열거나 닫는다(Ctrl+Shift+F).
    SearchInFolder,
    /// 선택한 터미널 안의 글자를 찾는 띠를 연다(Ctrl+Shift+H). 평 Ctrl+F·Ctrl+H는 셸의 키라 쓰지 않는다.
    FindInTerminal,
    /// 파일 빠른 열기를 연다(Ctrl+Shift+P). Ctrl+Shift라 셸의 키와 겹치지 않고, 터미널이 활성이어도 앱이 받는다. 평 Ctrl+P는 셸(`0x10`)의 키다.
    QuickOpen,
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
/// | Ctrl+Shift+F          | `SearchInFolder`           |
/// | Ctrl+Shift+H          | `FindInTerminal`(네이티브 추가 키) |
/// | Ctrl+Shift+P          | `QuickOpen`(네이티브 추가 키)      |
/// | Ctrl+P                | `None`(셸의 `0x10`)                |
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
            ("f", true) => Some(AppShortcut::SearchInFolder),
            ("h", true) => Some(AppShortcut::FindInTerminal),
            ("p", true) => Some(AppShortcut::QuickOpen),
            _ => None,
        }
    } else {
        None
    }
}

/// 파일 탭이 활성일 때만 앱이 가져가는 단축키. 터미널이 활성이면 같은 키가 셸로 간다(Ctrl+S는 `0x13`, Ctrl+G는 `0x07`).
/// 그래서 `app_shortcut`에 넣지 않고 따로 둔다 — `terminal_key_bytes`가 `app_shortcut`인 키를 PTY에서 막기 때문이다.
/// 찾기(Ctrl+F)·바꾸기(Ctrl+H)는 편집기 부품이 편집기가 포커스일 때만 받으므로 여기에 없다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileShortcut {
    /// 지금 파일을 저장한다(Ctrl+S).
    Save,
    /// 줄 번호를 물어 그 줄로 간다(Ctrl+G).
    GoToLine,
    /// 고른 줄에 검토 메모를 남긴다(Ctrl+Alt+M). Alt가 든 앱 키는 이것 하나뿐이다.
    AddReviewNote,
}

/// 키 누름이 파일 탭용 단축키이면 그 일을, 아니면 None을 돌려준다. Shift가 더해진 키는 아니고, Alt는 Ctrl+Alt+M(검토 메모)에서만 허용한다.
///
/// Examples
/// --------
/// | 키       | 결과               |
/// |----------|--------------------|
/// | Ctrl+S   | `Save`             |
/// | Ctrl+G   | `GoToLine`         |
/// | Ctrl+Alt+M | `AddReviewNote`  |
/// | Ctrl+Alt+S | `None`           |
/// | Ctrl+Shift+S | `None`         |
/// | Ctrl+F   | `None`(편집기 부품이 받음) |
pub fn file_shortcut(input: &KeyInput<'_>) -> Option<FileShortcut> {
    match (input.control && !input.shift, input.alt, input.key) {
        (true, false, "s") => Some(FileShortcut::Save),
        (true, false, "g") => Some(FileShortcut::GoToLine),
        // AltGr(= Ctrl+Alt)로 글자를 만드는 배열에서 Ctrl+Alt+M은 글자 입력이다: 글자가 들어 있으면 단축키로 보지 않는다.
        (true, true, "m") if input.key_char.is_none_or(str::is_empty) => Some(FileShortcut::AddReviewNote),
        _ => None,
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
    fn test_c_d38_search_shortcut_never_reaches_the_pty() {
        // Ctrl+Shift+F는 앱의 폴더 검색이다. Windows가 Shift 눌린 글자 키를 넘기는 이름은 `f` 그대로(`shift` 참)다.
        assert_eq!(app_shortcut(&press("f", true, true)), Some(AppShortcut::SearchInFolder));
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("f", true, true), app_cursor), None);
        }
        // Shift 없는 Ctrl+F는 셸·프로그램의 키(오른쪽으로 한 글자)다.
        assert_eq!(app_shortcut(&press("f", true, false)), None);
        assert_eq!(terminal_key_bytes(&press("f", true, false), false), Some(vec![0x06]));
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

    #[test]
    fn test_c_edit_8_save_and_go_to_line_are_file_keys_that_still_reach_the_pty() {
        assert_eq!(file_shortcut(&press("s", true, false)), Some(FileShortcut::Save));
        assert_eq!(file_shortcut(&press("g", true, false)), Some(FileShortcut::GoToLine));
        assert_eq!(file_shortcut(&press("s", true, true)), None);
        // Ctrl+Alt+M은 검토 메모, 다른 Alt 조합은 앱 키가 아니다. 터미널 활성이면 셸로 간다(ESC + 0x0d).
        let alt = |key: &'static str| KeyInput { key, key_char: None, control: true, shift: false, alt: true };
        assert_eq!(file_shortcut(&alt("m")), Some(FileShortcut::AddReviewNote));
        assert_eq!(file_shortcut(&alt("s")), None);
        let altgr = KeyInput { key: "m", key_char: Some("\u{b5}"), control: true, shift: false, alt: true };
        assert_eq!(file_shortcut(&altgr), None, "AltGr+M types a character on some layouts");
        assert_eq!(app_shortcut(&alt("m")), None);
        assert!(terminal_key_bytes(&alt("m"), false).is_some());
        assert_eq!(file_shortcut(&press("s", false, false)), None);
        assert_eq!(file_shortcut(&press("f", true, false)), None);
        // 앱 단축키가 아니므로 터미널 활성일 때 그대로 PTY로 간다.
        assert_eq!(app_shortcut(&press("s", true, false)), None);
        assert_eq!(terminal_key_bytes(&press("s", true, false), false), Some(vec![0x13]));
        assert_eq!(terminal_key_bytes(&press("g", true, false), false), Some(vec![0x07]));
    }

    #[test]
    fn test_c_sync_8_find_in_terminal_is_ctrl_shift_h_and_plain_ctrl_h_stays_with_the_shell() {
        assert_eq!(app_shortcut(&press("h", true, true)), Some(AppShortcut::FindInTerminal));
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("h", true, true), app_cursor), None);
            // Ctrl+H는 백스페이스(0x08)로 셸에 간다.
            assert_eq!(terminal_key_bytes(&press("h", true, false), app_cursor), Some(vec![0x08]));
        }
        assert_eq!(app_shortcut(&press("h", true, false)), None);
    }

    #[test]
    fn test_c_d72_quick_open_is_ctrl_shift_p_and_plain_ctrl_p_stays_with_the_shell() {
        assert_eq!(app_shortcut(&press("p", true, true)), Some(AppShortcut::QuickOpen));
        assert_eq!(app_shortcut(&press("p", true, false)), None);
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("p", true, true), app_cursor), None);
            assert_eq!(terminal_key_bytes(&press("p", true, false), app_cursor), Some(vec![0x10]));
        }
    }
}
