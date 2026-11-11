//! 앱이 터미널보다 먼저 가로채는 단축키: 분할, 새 터미널, 탭 이동, 사이드바 접기, 폴더 검색, 터미널 안 찾기.
//!
//! 이 키들은 셸·프로그램에 보내면 안 된다(예: Ctrl+Shift+C가 Ctrl+C로 나가 실행 중인 프로그램을 끊는다).
//! 키 이름은 창 도구가 준 값 그대로다. 글자 키는 Shift가 눌려도 누른 키 자체의 이름(`shift`는 참)이다.
//! 그러나 Windows는 Shift가 눌린 숫자·기호 키를 바뀐 글자로 넘기고 `shift`를 거짓으로 비운다
//! (Ctrl+Shift+`는 `~`, Ctrl+Shift+5는 `%`). 그래서 두 형태를 모두 받는다.
//!
//! 이 단축키 중 Ctrl+Shift+C·`Ctrl+\`·Ctrl+←/→·Ctrl+B와 폴더 검색의 Ctrl+Shift+F는 기존 앱에 있는 키이고, Ctrl+Shift+T·W는 기존 앱에는 없는
//! 네이티브 추가 키다(터미널 앱의 관례이고 Ctrl+Shift 조합이라 셸의 키와 겹치지 않는다).
//!
//! 위 설명은 Windows·Linux(`ShortcutScheme::Ctrl`)의 규칙이다. macOS(`ShortcutScheme::Command`)는 수식 키가 Cmd이고, Ctrl 조합은 하나도
//! 가로채지 않고 모두 터미널로 보낸다(Terminal·iTerm과 같다). Cmd 조합은 셸이 받는 키가 아니라서 겹침을 피하려 붙였던 Shift는 뗀다.
//!
//! | 동작                 | Windows·Linux          | macOS            |
//! |----------------------|------------------------|------------------|
//! | 새 터미널 탭         | Ctrl+Shift+T · Ctrl+`  | Cmd+T            |
//! | 활성 탭 닫기         | Ctrl+Shift+W           | Cmd+W            |
//! | 앞·뒤 탭             | Ctrl+←/→               | Cmd+Shift+[ / ]  |
//! | n번째·마지막 탭      | Ctrl+↑(첫)·Ctrl+↓(끝)  | Cmd+1~8 · Cmd+9  |
//! | 오른쪽·아래 분할     | Ctrl+\ · Ctrl+Shift+` | Cmd+D · Cmd+Shift+D |
//! | 사이드바             | Ctrl+B                 | Cmd+B            |
//! | 폴더 검색            | Ctrl+Shift+F           | Cmd+Shift+F      |
//! | 터미널 안 찾기       | Ctrl+Shift+H           | Cmd+F            |
//! | 빠른 열기            | Ctrl+Shift+P           | Cmd+P            |
use crate::key_encoding::{KeyInput, ShortcutScheme, encode_key};

/// 앱 단축키가 하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppShortcut {
    /// 선택한 터미널 옆 칸에 새 터미널을 연다.
    SplitRight,
    /// 선택한 터미널 아래 칸에 새 터미널을 연다.
    SplitDown,
    /// 지금 보는 자리(작업 폴더·묶음)에 새 터미널을 탭으로 연다.
    NewTerminalHere,
    /// 활성 탭(터미널 또는 파일 탭)을 닫는다. Windows·Linux는 평 Ctrl+W가 셸의 단어 지우기라 Ctrl+Shift+W를 쓴다.
    CloseActiveTab,
    PreviousTab,
    NextTab,
    FirstTab,
    LastTab,
    /// 탭 줄의 `index`번째(0부터) 탭으로 간다. 그 자리에 탭이 없으면 아무 일 없다(macOS의 Cmd+1~8).
    TabAt(usize),
    /// 터미널 글꼴을 한 단계 키우거나 줄인다(macOS의 Cmd+= · Cmd+-). 설정의 터미널 글꼴 크기 단계와 같다.
    FontLarger,
    FontSmaller,
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
/// 체계(`scheme`)가 `Ctrl`이면 아래 표의 Ctrl 키가, `Command`이면 `command_shortcut`의 Cmd 키가 단축키다. `Command` 체계에서 Ctrl이 든 키는 모두 None이다.
///
/// Examples
/// --------
/// | 체계    | 키                    | 결과                       |
/// |---------|-----------------------|----------------------------|
/// | Ctrl    | Ctrl+Shift+C          | `SplitRight`               |
/// | Ctrl    | Ctrl+\ · Ctrl+Shift+5 | `SplitRight`               |
/// | Ctrl    | Ctrl+Shift+`          | `SplitDown`                |
/// | Ctrl    | `%`·`~`(Ctrl, shift 거짓) | Windows가 Shift+5·Shift+`를 넘기는 형태. 위 둘과 같다 |
/// | Ctrl    | Ctrl+`                | `NewTerminalHere`          |
/// | Ctrl    | Ctrl+←/→/↑/↓          | 앞·뒤·처음·마지막 탭       |
/// | Ctrl    | Ctrl+B                | `ToggleSidebar`            |
/// | Ctrl    | Ctrl+Shift+F          | `SearchInFolder`           |
/// | Ctrl    | Ctrl+Shift+H          | `FindInTerminal`(네이티브 추가 키) |
/// | Ctrl    | Ctrl+Shift+P          | `QuickOpen`(네이티브 추가 키)      |
/// | Ctrl    | Ctrl+P                | `None`(셸의 `0x10`)                |
/// | Ctrl    | Ctrl+Shift+T          | `NewTerminalHere`(네이티브 추가 키) |
/// | Ctrl    | Ctrl+Shift+W          | `CloseActiveTab`(네이티브 추가 키) |
/// | Ctrl    | Ctrl+C                | `None`                     |
/// | Command | Cmd+T · Cmd+W         | `NewTerminalHere` · `CloseActiveTab` |
/// | Command | Cmd+Shift+[ · Cmd+Shift+] | `PreviousTab` · `NextTab` |
/// | Command | Cmd+2 · Cmd+9         | `TabAt(1)` · `LastTab`     |
/// | Command | Cmd+D · Cmd+Shift+D   | `SplitRight` · `SplitDown` |
/// | Command | Ctrl+C · Ctrl+B · Ctrl+Shift+T | `None`(터미널로)  |
pub fn app_shortcut(
    input: &KeyInput<'_>,
    scheme: ShortcutScheme,
) -> Option<AppShortcut> {
    match scheme {
        ShortcutScheme::Ctrl => ctrl_shortcut(input),
        ShortcutScheme::Command => command_shortcut(input),
    }
}

/// Windows·Linux의 앱 단축키(Ctrl 체계). Cmd(Win 키)가 함께 눌린 키는 단축키가 아니다.
fn ctrl_shortcut(input: &KeyInput<'_>) -> Option<AppShortcut> {
    if input.control && !input.alt && !input.command {
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

/// macOS의 앱 단축키(Cmd 체계). Ctrl·Alt(Option)가 함께 눌린 키는 단축키가 아니다 — Ctrl은 터미널로 가고 Option은 글자 입력용이다.
///
/// 글자 키는 Shift가 눌려도 누른 키 자체의 이름(`shift`는 참)이다. 그러나 macOS는 Shift가 눌린 `[`·`]`를 바뀐 글자(`{`·`}`)로 넘기고 `shift`를 거짓으로 비운다(실측).
/// 그래서 Cmd+Shift+[ 는 `[`+shift 참이거나 `{`(shift 무관)이다. Shift 없는 Cmd+[ 는 편집기의 내어쓰기라 단축키가 아니다.
fn command_shortcut(input: &KeyInput<'_>) -> Option<AppShortcut> {
    if input.command && !input.control && !input.alt {
        match (input.key, input.shift) {
            ("t", false) => Some(AppShortcut::NewTerminalHere),
            ("w", false) => Some(AppShortcut::CloseActiveTab),
            ("[", true) | ("{", _) => Some(AppShortcut::PreviousTab),
            ("]", true) | ("}", _) => Some(AppShortcut::NextTab),
            ("9", false) => Some(AppShortcut::LastTab),
            (digit @ ("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"), false) => {
                digit.parse::<usize>().ok().map(|number| AppShortcut::TabAt(number - 1))
            }
            ("d", false) => Some(AppShortcut::SplitRight),
            ("d", true) => Some(AppShortcut::SplitDown),
            ("b", false) => Some(AppShortcut::ToggleSidebar),
            ("f", true) => Some(AppShortcut::SearchInFolder),
            ("f", false) => Some(AppShortcut::FindInTerminal),
            ("p", false) => Some(AppShortcut::QuickOpen),
            ("=" | "+", _) => Some(AppShortcut::FontLarger),
            ("-", _) => Some(AppShortcut::FontSmaller),
            _ => None,
        }
    } else {
        None
    }
}

/// 파일 탭이 활성일 때만 앱이 가져가는 단축키. 터미널이 활성이면 같은 키가 셸로 간다(Ctrl+S는 `0x13`, Ctrl+G는 `0x07`).
/// 그래서 `app_shortcut`에 넣지 않고 따로 둔다 — `terminal_key_bytes`가 `app_shortcut`인 키를 PTY에서 막기 때문이다.
/// 찾기(Ctrl+F)·바꾸기(Ctrl+H)는 편집기 부품이 편집기가 포커스일 때만 받으므로 여기에 없다.
/// macOS는 Ctrl 대신 Cmd(Cmd+S·Cmd+G)이고, 검토 메모는 Option이 글자 입력용이라 Cmd+Shift+M이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileShortcut {
    /// 지금 파일을 저장한다(Ctrl+S).
    Save,
    /// 줄 번호를 물어 그 줄로 간다(Ctrl+G).
    GoToLine,
    /// 고른 줄에 검토 메모를 남긴다(Ctrl+Alt+M). Windows·Linux에서 Alt가 든 앱 키는 이것 하나뿐이다.
    AddReviewNote,
}

/// 키 누름이 파일 탭용 단축키이면 그 일을, 아니면 None을 돌려준다. Shift가 더해진 키는 아니고, Alt는 Windows·Linux의 Ctrl+Alt+M(검토 메모)에서만 허용한다.
/// macOS는 Cmd+S·Cmd+G·Cmd+Shift+M이고 Alt(Option)가 든 키는 없다.
///
/// Examples
/// --------
/// | 체계    | 키       | 결과               |
/// |---------|----------|--------------------|
/// | Ctrl    | Ctrl+S   | `Save`             |
/// | Ctrl    | Ctrl+G   | `GoToLine`         |
/// | Ctrl    | Ctrl+Alt+M | `AddReviewNote`  |
/// | Ctrl    | Ctrl+Alt+S | `None`           |
/// | Ctrl    | Ctrl+Shift+S | `None`         |
/// | Ctrl    | Ctrl+F   | `None`(편집기 부품이 받음) |
/// | Command | Cmd+S    | `Save`             |
/// | Command | Cmd+G    | `GoToLine`         |
/// | Command | Cmd+Shift+M | `AddReviewNote` |
/// | Command | Ctrl+S   | `None`(터미널로)   |
pub fn file_shortcut(
    input: &KeyInput<'_>,
    scheme: ShortcutScheme,
) -> Option<FileShortcut> {
    let mut result = None;
    match scheme {
        ShortcutScheme::Ctrl => {
            result = match (input.control && !input.shift && !input.command, input.alt, input.key) {
                (true, false, "s") => Some(FileShortcut::Save),
                (true, false, "g") => Some(FileShortcut::GoToLine),
                // AltGr(= Ctrl+Alt)로 글자를 만드는 배열에서 Ctrl+Alt+M은 글자 입력이다: 글자가 들어 있으면 단축키로 보지 않는다.
                (true, true, "m") if input.key_char.is_none_or(str::is_empty) => Some(FileShortcut::AddReviewNote),
                _ => None,
            };
        }
        ShortcutScheme::Command => {
            if scheme.primary(input) && !input.alt {
                result = match (input.key, input.shift) {
                    ("s", false) => Some(FileShortcut::Save),
                    ("g", false) => Some(FileShortcut::GoToLine),
                    ("m", true) => Some(FileShortcut::AddReviewNote),
                    _ => None,
                };
            }
        }
    }
    result
}

/// 키 누름이 터미널(PTY)에 보낼 바이트. 앱 단축키는 보내지 않으므로 None이다.
///
/// 터미널 보기의 키 처리는 `encode_key`가 아니라 이 함수만 불러, 단축키가 어떤 경로로도 PTY에 닿지 않게 한다.
/// macOS(`Command` 체계)에서는 Cmd가 든 키가 단축키가 아니어도 보내지 않고(Terminal·iTerm처럼 글자가 입력되지 않는다),
/// Option+←/→는 단어 이동(`ESC b`·`ESC f`)으로 보낸다(Terminal.app 기본). Cmd+←/→·Cmd+Backspace는 줄 처음·끝·앞부분 지우기(`0x01`·`0x05`·`0x15`, iTerm 기본)다.
/// Windows·Linux는 Win 키가 든 키를 보내지 않는다. 그 밖의 키는 두 체계가 같다.
///
/// Examples
/// --------
/// | 체계    | 키               | 결과                |
/// |---------|------------------|---------------------|
/// | Command | Ctrl+C           | `0x03`              |
/// | Command | Cmd+T            | None(앱 단축키)     |
/// | Command | Cmd+Z            | None(Cmd는 보내지 않음) |
/// | Command | Option+←         | `ESC b`             |
/// | Command | Option+→         | `ESC f`             |
/// | Command | Cmd+← · Cmd+→    | `0x01` · `0x05`     |
/// | Command | Cmd+Backspace    | `0x15`              |
/// | Ctrl    | Win+키·Ctrl+Win+키 | None              |
/// | Ctrl    | Alt+←            | `ESC [ 1 ; 3 D`     |
pub fn terminal_key_bytes(
    input: &KeyInput<'_>,
    app_cursor: bool,
    scheme: ShortcutScheme,
) -> Option<Vec<u8>> {
    // 맥 터미널 앱(iTerm 기본)처럼 Cmd+←/→는 줄 처음·끝(Ctrl+A·Ctrl+E), Cmd+Backspace는 줄 앞부분 지우기(Ctrl+U)다.
    let plain_modifiers = !input.control && !input.shift && !input.alt;
    let mac_edit = match (scheme, input.key, plain_modifiers && input.command) {
        (ShortcutScheme::Command, "left", true) => Some(vec![0x01]),
        (ShortcutScheme::Command, "right", true) => Some(vec![0x05]),
        (ShortcutScheme::Command, "backspace", true) => Some(vec![0x15]),
        _ => None,
    };
    let word_move = match (scheme, input.key, input.alt && !input.control && !input.shift && !input.command) {
        (ShortcutScheme::Command, "left", true) => Some(b"\x1bb".to_vec()),
        (ShortcutScheme::Command, "right", true) => Some(b"\x1bf".to_vec()),
        _ => None,
    };
    if app_shortcut(input, scheme).is_some() || input.command && mac_edit.is_none() {
        // 앱 단축키이거나 Cmd(Win)가 든 다른 키: 맥은 아무것도 보내지 않고(Terminal·iTerm처럼), Windows·Linux는 Win 키 조합을 시스템 몫으로 둔다.
        None
    } else {
        mac_edit.or(word_move).or_else(|| encode_key(input, app_cursor))
    }
}

/// 키 누름이 글자 입력(입력 방식 경로가 한 번 보내는 키)인지. 참이면 터미널 보기는 이 키의 바이트를 직접 보내지 않는다.
///
/// 글자를 만드는 키(제어 문자가 아닌 `key_char`가 있는 키)가 대상이다. Windows·Linux는 Ctrl·Alt 없이 누른 글자와 AltGr 조합(`prefer_character_input`)이고,
/// Ctrl·Alt가 든 키는 제어 바이트·`ESC` 접두로 직접 보낸다. macOS는 Option이 글자 입력용이라 Option 조합(`å` 등)도 글자 입력이고, Cmd가 든 키는 글자가 아니다.
///
/// Examples
/// --------
/// | 체계    | 키               | `key_char` | 결과 |
/// |---------|------------------|------------|------|
/// | Ctrl    | `a`              | `a`        | 참   |
/// | Ctrl    | Alt+`x`          | `x`        | 거짓 |
/// | Command | Option+`a`       | `å`        | 참   |
/// | Command | Ctrl+`a`         | `\x01`     | 거짓 |
/// | Command | Cmd+`a`          | `a`        | 거짓 |
pub fn makes_text(
    input: &KeyInput<'_>,
    prefer_character_input: bool,
    scheme: ShortcutScheme,
) -> bool {
    let has_text = input.key_char.is_some_and(|text| !text.chars().any(char::is_control));
    let direct = match scheme {
        ShortcutScheme::Ctrl => !input.control && !input.alt,
        ShortcutScheme::Command => !input.control && !input.command,
    };
    has_text && (direct || prefer_character_input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(
        key: &'static str,
        control: bool,
        shift: bool,
    ) -> KeyInput<'static> {
        KeyInput { key, key_char: None, control, shift, alt: false, command: false }
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
            assert_eq!(app_shortcut(&input, ShortcutScheme::Ctrl), Some(expected), "{input:?}");
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
            assert_eq!(app_shortcut(&input, ShortcutScheme::Ctrl), Some(expected), "{input:?}");
        }
        // 변환된 글자라도 Ctrl이 없으면 그냥 글자다.
        assert_eq!(app_shortcut(&press("~", false, false), ShortcutScheme::Ctrl), None);
        assert_eq!(app_shortcut(&press("%", false, false), ShortcutScheme::Ctrl), None);
        // Ctrl+Shift+\는 `|`로 오며 분할 키가 아니다(Ctrl+\만 분할이다).
        assert_eq!(app_shortcut(&press("|", true, false), ShortcutScheme::Ctrl), None);
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
            KeyInput { key: "left", key_char: None, control: true, shift: false, alt: true, command: false },
            KeyInput { key: "b", key_char: None, control: true, shift: false, alt: true, command: false },
        ];
        for input in table {
            assert_eq!(app_shortcut(&input, ShortcutScheme::Ctrl), None, "{input:?}");
        }
    }

    #[test]
    fn test_c_d51_closing_the_active_tab_uses_ctrl_shift_w_only_so_plain_ctrl_w_still_deletes_a_word_in_the_shell() {
        // 파일 탭이 활성이어도 터미널이 활성이어도 같은 키가 활성 탭을 닫고, 그 키는 PTY로 가지 않는다.
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("w", true, true), app_cursor, ShortcutScheme::Ctrl), None);
            assert_eq!(
                terminal_key_bytes(&press("w", true, false), app_cursor, ShortcutScheme::Ctrl),
                Some(vec![0x17])
            );
        }
        assert_eq!(app_shortcut(&press("w", true, false), ShortcutScheme::Ctrl), None);
    }

    #[test]
    fn test_c_d38_search_shortcut_never_reaches_the_pty() {
        // Ctrl+Shift+F는 앱의 폴더 검색이다. Windows가 Shift 눌린 글자 키를 넘기는 이름은 `f` 그대로(`shift` 참)다.
        assert_eq!(app_shortcut(&press("f", true, true), ShortcutScheme::Ctrl), Some(AppShortcut::SearchInFolder));
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("f", true, true), app_cursor, ShortcutScheme::Ctrl), None);
        }
        // Shift 없는 Ctrl+F는 셸·프로그램의 키(오른쪽으로 한 글자)다.
        assert_eq!(app_shortcut(&press("f", true, false), ShortcutScheme::Ctrl), None);
        assert_eq!(terminal_key_bytes(&press("f", true, false), false, ShortcutScheme::Ctrl), Some(vec![0x06]));
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
                assert_eq!(
                    terminal_key_bytes(&input, app_cursor, ShortcutScheme::Ctrl),
                    None,
                    "{input:?} app_cursor={app_cursor}"
                );
            }
        }
        // 막지 않는 키는 그대로 인코딩된다: Ctrl+C는 0x03이고 일반 방향키는 이스케이프 열이다.
        assert_eq!(terminal_key_bytes(&press("c", true, false), false, ShortcutScheme::Ctrl), Some(vec![0x03]));
        assert_eq!(
            terminal_key_bytes(&press("left", false, false), false, ShortcutScheme::Ctrl),
            Some(b"\x1b[D".to_vec())
        );
        // 수정 키를 더한 방향키(Ctrl+Shift+←)는 막지 않는다.
        assert_eq!(
            terminal_key_bytes(&press("left", true, true), false, ShortcutScheme::Ctrl),
            Some(b"\x1b[D".to_vec())
        );
    }

    #[test]
    fn test_c_edit_8_save_and_go_to_line_are_file_keys_that_still_reach_the_pty() {
        assert_eq!(file_shortcut(&press("s", true, false), ShortcutScheme::Ctrl), Some(FileShortcut::Save));
        assert_eq!(file_shortcut(&press("g", true, false), ShortcutScheme::Ctrl), Some(FileShortcut::GoToLine));
        assert_eq!(file_shortcut(&press("s", true, true), ShortcutScheme::Ctrl), None);
        // Ctrl+Alt+M은 검토 메모, 다른 Alt 조합은 앱 키가 아니다. 터미널 활성이면 셸로 간다(ESC + 0x0d).
        let alt = |key: &'static str| KeyInput {
            key,
            key_char: None,
            control: true,
            shift: false,
            alt: true,
            command: false,
        };
        assert_eq!(file_shortcut(&alt("m"), ShortcutScheme::Ctrl), Some(FileShortcut::AddReviewNote));
        assert_eq!(file_shortcut(&alt("s"), ShortcutScheme::Ctrl), None);
        let altgr =
            KeyInput { key: "m", key_char: Some("\u{b5}"), control: true, shift: false, alt: true, command: false };
        assert_eq!(file_shortcut(&altgr, ShortcutScheme::Ctrl), None, "AltGr+M types a character on some layouts");
        assert_eq!(app_shortcut(&alt("m"), ShortcutScheme::Ctrl), None);
        assert!(terminal_key_bytes(&alt("m"), false, ShortcutScheme::Ctrl).is_some());
        assert_eq!(file_shortcut(&press("s", false, false), ShortcutScheme::Ctrl), None);
        assert_eq!(file_shortcut(&press("f", true, false), ShortcutScheme::Ctrl), None);
        // 앱 단축키가 아니므로 터미널 활성일 때 그대로 PTY로 간다.
        assert_eq!(app_shortcut(&press("s", true, false), ShortcutScheme::Ctrl), None);
        assert_eq!(terminal_key_bytes(&press("s", true, false), false, ShortcutScheme::Ctrl), Some(vec![0x13]));
        assert_eq!(terminal_key_bytes(&press("g", true, false), false, ShortcutScheme::Ctrl), Some(vec![0x07]));
    }

    #[test]
    fn test_c_sync_8_find_in_terminal_is_ctrl_shift_h_and_plain_ctrl_h_stays_with_the_shell() {
        assert_eq!(app_shortcut(&press("h", true, true), ShortcutScheme::Ctrl), Some(AppShortcut::FindInTerminal));
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("h", true, true), app_cursor, ShortcutScheme::Ctrl), None);
            // Ctrl+H는 백스페이스(0x08)로 셸에 간다.
            assert_eq!(
                terminal_key_bytes(&press("h", true, false), app_cursor, ShortcutScheme::Ctrl),
                Some(vec![0x08])
            );
        }
        assert_eq!(app_shortcut(&press("h", true, false), ShortcutScheme::Ctrl), None);
    }

    #[test]
    fn test_c_d72_quick_open_is_ctrl_shift_p_and_plain_ctrl_p_stays_with_the_shell() {
        assert_eq!(app_shortcut(&press("p", true, true), ShortcutScheme::Ctrl), Some(AppShortcut::QuickOpen));
        assert_eq!(app_shortcut(&press("p", true, false), ShortcutScheme::Ctrl), None);
        for app_cursor in [false, true] {
            assert_eq!(terminal_key_bytes(&press("p", true, true), app_cursor, ShortcutScheme::Ctrl), None);
            assert_eq!(
                terminal_key_bytes(&press("p", true, false), app_cursor, ShortcutScheme::Ctrl),
                Some(vec![0x10])
            );
        }
    }

    const MAC: ShortcutScheme = ShortcutScheme::Command;

    fn cmd(
        key: &'static str,
        shift: bool,
    ) -> KeyInput<'static> {
        KeyInput { key, key_char: None, control: false, shift, alt: false, command: true }
    }

    #[test]
    fn test_macos_app_shortcuts_are_cmd_keys() {
        let table = [
            (cmd("t", false), AppShortcut::NewTerminalHere),
            (cmd("w", false), AppShortcut::CloseActiveTab),
            (cmd("[", true), AppShortcut::PreviousTab),
            (cmd("{", true), AppShortcut::PreviousTab),
            (cmd("{", false), AppShortcut::PreviousTab),
            (cmd("]", true), AppShortcut::NextTab),
            (cmd("}", true), AppShortcut::NextTab),
            (cmd("}", false), AppShortcut::NextTab),
            (cmd("1", false), AppShortcut::TabAt(0)),
            (cmd("2", false), AppShortcut::TabAt(1)),
            (cmd("8", false), AppShortcut::TabAt(7)),
            (cmd("9", false), AppShortcut::LastTab),
            (cmd("d", false), AppShortcut::SplitRight),
            (cmd("d", true), AppShortcut::SplitDown),
            (cmd("b", false), AppShortcut::ToggleSidebar),
            (cmd("f", true), AppShortcut::SearchInFolder),
            (cmd("f", false), AppShortcut::FindInTerminal),
            (cmd("p", false), AppShortcut::QuickOpen),
        ];
        for (input, expected) in table {
            assert_eq!(app_shortcut(&input, MAC), Some(expected), "{input:?}");
            // 앱 단축키는 어떤 경로로도 PTY에 닿지 않는다.
            for app_cursor in [false, true] {
                assert_eq!(terminal_key_bytes(&input, app_cursor, MAC), None, "{input:?}");
            }
        }
        assert_eq!(app_shortcut(&cmd("[", false), MAC), None, "Cmd+[ without Shift is the editor's outdent");
    }

    #[test]
    fn test_macos_ctrl_keys_are_never_app_shortcuts_and_all_reach_the_shell() {
        // Windows·Linux에서 앱 키였던 Ctrl 조합 전부와 평 Ctrl 키가 macOS에서는 터미널로 간다.
        let ctrl_keys = [
            press("c", true, true),
            press("\\", true, false),
            press("`", true, true),
            press("`", true, false),
            press("t", true, true),
            press("w", true, true),
            press("w", true, false),
            press("left", true, false),
            press("right", true, false),
            press("up", true, false),
            press("down", true, false),
            press("b", true, false),
            press("f", true, true),
            press("h", true, true),
            press("p", true, true),
            press("c", true, false),
            press("d", true, false),
        ];
        for input in ctrl_keys {
            assert_eq!(app_shortcut(&input, MAC), None, "{input:?}");
            // 보낼 바이트가 없는 키(Ctrl+`)는 앱이 막은 것이 아니라 터미널에도 대응 바이트가 없는 것이다.
            let encodable = input.key != "`";
            assert_eq!(terminal_key_bytes(&input, false, MAC).is_some(), encodable, "{input:?}");
        }
        assert_eq!(terminal_key_bytes(&press("c", true, false), false, MAC), Some(vec![0x03]));
        assert_eq!(terminal_key_bytes(&press("w", true, false), false, MAC), Some(vec![0x17]));
        // Cmd와 Ctrl을 함께 누른 키도 앱 단축키가 아니다.
        let both = KeyInput { control: true, ..cmd("t", false) };
        assert_eq!(app_shortcut(&both, MAC), None);
    }

    #[test]
    fn test_windows_and_linux_scheme_ignores_cmd_keys_so_the_old_rules_stay() {
        // Ctrl 체계에서 Cmd(Win) 키만 누른 키는 단축키가 아니고, Ctrl과 함께 눌러도 단축키가 아니다.
        for input in [cmd("t", false), cmd("d", false), cmd("p", false), cmd("1", false), cmd("[", true)] {
            assert_eq!(app_shortcut(&input, ShortcutScheme::Ctrl), None, "{input:?}");
        }
        let with_ctrl = KeyInput { control: true, ..cmd("t", true) };
        assert_eq!(app_shortcut(&with_ctrl, ShortcutScheme::Ctrl), None);
        // 기존 Ctrl 키는 체계가 Ctrl일 때 그대로다.
        assert_eq!(app_shortcut(&press("t", true, true), ShortcutScheme::Ctrl), Some(AppShortcut::NewTerminalHere));
        assert_eq!(app_shortcut(&cmd("t", false), ShortcutScheme::Ctrl), None);
    }

    #[test]
    fn test_macos_file_keys_are_cmd_s_cmd_g_and_cmd_shift_m() {
        assert_eq!(file_shortcut(&cmd("s", false), MAC), Some(FileShortcut::Save));
        assert_eq!(file_shortcut(&cmd("g", false), MAC), Some(FileShortcut::GoToLine));
        assert_eq!(file_shortcut(&cmd("m", true), MAC), Some(FileShortcut::AddReviewNote));
        assert_eq!(file_shortcut(&cmd("s", true), MAC), None);
        assert_eq!(file_shortcut(&cmd("m", false), MAC), None);
        // Ctrl+S·Ctrl+G는 터미널의 `0x13`·`0x07`이고 Option이 든 키는 앱 키가 아니다.
        assert_eq!(file_shortcut(&press("s", true, false), MAC), None);
        assert_eq!(file_shortcut(&KeyInput { alt: true, ..cmd("s", false) }, MAC), None);
        // Windows·Linux 체계에서 Cmd+S는 파일 키가 아니다.
        assert_eq!(file_shortcut(&cmd("s", false), ShortcutScheme::Ctrl), None);
    }

    #[test]
    fn test_macos_option_arrows_move_by_word_and_cmd_keys_that_are_not_shortcuts_send_nothing() {
        let option = |key| KeyInput { key, key_char: None, control: false, shift: false, alt: true, command: false };
        assert_eq!(terminal_key_bytes(&option("left"), false, MAC), Some(b"\x1bb".to_vec()));
        assert_eq!(terminal_key_bytes(&option("right"), true, MAC), Some(b"\x1bf".to_vec()));
        // Windows·Linux의 Alt+←는 xterm 수정 키 열 그대로다.
        assert_eq!(terminal_key_bytes(&option("left"), false, ShortcutScheme::Ctrl), Some(b"\x1b[1;3D".to_vec()));
        // Option+Backspace는 `ESC DEL`(단어 지우기)이다.
        assert_eq!(terminal_key_bytes(&option("backspace"), false, MAC), Some(vec![0x1b, 0x7f]));
        // 앱 단축키가 아닌 Cmd 키(Cmd+Z·Cmd+C 등)는 글자도 제어 바이트도 보내지 않는다.
        let typed = KeyInput { key_char: Some("z"), ..cmd("z", false) };
        assert_eq!(terminal_key_bytes(&typed, false, MAC), None);
        assert_eq!(terminal_key_bytes(&cmd("c", false), false, MAC), None);
    }

    #[test]
    fn test_macos_cmd_arrows_and_backspace_edit_the_shell_line() {
        assert_eq!(terminal_key_bytes(&cmd("left", false), false, MAC), Some(vec![0x01]));
        assert_eq!(terminal_key_bytes(&cmd("right", false), true, MAC), Some(vec![0x05]));
        assert_eq!(terminal_key_bytes(&cmd("backspace", false), false, MAC), Some(vec![0x15]));
        // Shift가 더해지면 줄 편집 키가 아니다.
        assert_eq!(terminal_key_bytes(&cmd("left", true), false, MAC), None);
        // Windows·Linux는 Win 키가 든 키를 터미널로 보내지 않고, Ctrl+Win 조합도 마찬가지다.
        let ctrl = ShortcutScheme::Ctrl;
        assert_eq!(terminal_key_bytes(&cmd("left", false), false, ctrl), None);
        let ctrl_win = KeyInput { control: true, ..cmd("c", false) };
        assert_eq!(terminal_key_bytes(&ctrl_win, false, ctrl), None);
        let ctrl_win_v = KeyInput { control: true, ..cmd("v", false) };
        assert_eq!(terminal_key_bytes(&ctrl_win_v, false, ctrl), None);
    }

    #[test]
    fn test_macos_cmd_plus_and_minus_resize_the_terminal_font() {
        assert_eq!(app_shortcut(&cmd("=", false), MAC), Some(AppShortcut::FontLarger));
        assert_eq!(app_shortcut(&cmd("+", true), MAC), Some(AppShortcut::FontLarger));
        assert_eq!(app_shortcut(&cmd("-", false), MAC), Some(AppShortcut::FontSmaller));
        assert_eq!(app_shortcut(&press("=", true, false), ShortcutScheme::Ctrl), None);
    }

    #[test]
    fn test_ctrl_scheme_ignores_win_key_combinations_in_every_decision() {
        let ctrl = ShortcutScheme::Ctrl;
        assert!(ctrl.accepts_command_key(false));
        assert!(!ctrl.accepts_command_key(true));
        assert!(MAC.accepts_command_key(true));
        for input in [KeyInput { control: true, ..cmd("t", true) }, KeyInput { control: true, ..cmd("b", false) }] {
            assert_eq!(app_shortcut(&input, ctrl), None);
            assert_eq!(file_shortcut(&input, ctrl), None);
        }
    }

    #[test]
    fn test_makes_text_treats_option_as_text_input_on_macos_only() {
        let typed = |key, key_char| KeyInput { key, key_char: Some(key_char), ..Default::default() };
        let alt = |key, key_char| KeyInput { alt: true, ..typed(key, key_char) };
        // 평 글자는 두 체계 모두 입력 방식 경로가 보낸다.
        assert!(makes_text(&typed("a", "a"), false, ShortcutScheme::Ctrl));
        assert!(makes_text(&typed("a", "a"), false, MAC));
        // Option+a(`å`)는 macOS에서 글자이고, Windows·Linux의 Alt+x는 `ESC x`로 직접 보낸다.
        assert!(makes_text(&alt("a", "\u{e5}"), false, MAC));
        assert!(!makes_text(&alt("x", "x"), false, ShortcutScheme::Ctrl));
        assert!(makes_text(&alt("x", "x"), true, ShortcutScheme::Ctrl), "AltGr");
        // Ctrl·Cmd가 든 키와 제어 문자는 글자가 아니다.
        assert!(!makes_text(&KeyInput { control: true, ..typed("a", "a") }, false, MAC));
        assert!(!makes_text(&KeyInput { command: true, ..typed("a", "a") }, false, MAC));
        assert!(!makes_text(&typed("a", "\u{1}"), false, MAC));
        assert!(!makes_text(&KeyInput { key_char: None, ..typed("left", "") }, false, MAC));
    }
}
