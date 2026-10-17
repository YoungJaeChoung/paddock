//! 키 입력 → 터미널로 보낼 바이트. 창 도구와 터미널 에뮬레이터의 타입을 모르고, 모드 플래그는 bool로 받는다.

/// 한 번의 키 누름. 창 도구가 준 값을 그대로 옮겨 담는다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyInput<'a> {
    /// 키 이름(`enter`, `up`, `a`, `space` …).
    pub key: &'a str,
    /// 이 누름이 만든 글자. 글자를 만들지 않는 키는 None이다.
    pub key_char: Option<&'a str>,
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// 키 누름이 터미널에 보낼 바이트. 보낼 것이 없는 키(수정 키만 누름 등)는 None이다.
///
/// `app_cursor`는 터미널 프로그램이 커서 키를 애플리케이션 모드로 바꿨는지다 — 방향키·Home·End가 `ESC [` 대신 `ESC O`로 나간다.
///
/// Examples
/// --------
/// | 입력                       | app_cursor | 결과             |
/// |----------------------------|------------|------------------|
/// | `enter`                    | -          | `\r`             |
/// | `up`                       | false      | `ESC [ A`        |
/// | `up`                       | true       | `ESC O A`        |
/// | Ctrl + `c`                 | -          | `0x03`           |
/// | Ctrl + `[`                 | -          | `0x1b`           |
/// | Alt + 글자 `x`             | -          | `ESC x`          |
/// | Alt + `backspace`          | -          | `ESC 0x7f`       |
/// | Alt + `left`               | -          | `ESC [ 1 ; 3 D`  |
/// | `shift` 하나               | -          | `None`           |
pub fn encode_key(
    input: &KeyInput<'_>,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let csi = |c: char| {
        if input.alt {
            format!("\x1b[1;3{c}")
        } else if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    // `~`로 끝나는 편집 키. Alt는 수정 키 번호 3을 넣는다.
    let tilde = |n: u8| if input.alt { format!("\x1b[{n};3~") } else { format!("\x1b[{n}~") };
    // 한 바이트로 나가는 이름 키는 Alt를 누르면 ESC가 앞에 붙는다.
    let alt_prefix = if input.alt { "\x1b" } else { "" };
    let named: Option<String> = match input.key {
        "enter" => Some(format!("{alt_prefix}\r")),
        "tab" if input.shift => Some("\x1b[Z".into()),
        "tab" => Some(format!("{alt_prefix}\t")),
        "backspace" => Some(format!("{alt_prefix}\x7f")),
        "escape" => Some(format!("{alt_prefix}\x1b")),
        "up" => Some(csi('A')),
        "down" => Some(csi('B')),
        "right" => Some(csi('C')),
        "left" => Some(csi('D')),
        "home" => Some(csi('H')),
        "end" => Some(csi('F')),
        "pageup" => Some(tilde(5)),
        "pagedown" => Some(tilde(6)),
        "delete" => Some(tilde(3)),
        "insert" => Some(tilde(2)),
        "space" if input.control => Some(format!("{alt_prefix}\0")),
        _ => None,
    };
    let key = input.key.as_bytes();
    let mut result = named.map(String::into_bytes);
    if result.is_none() && input.control && key.len() == 1 {
        // Ctrl+글자는 0x01~0x1a, Ctrl+[ \\ ] ^ _ / 는 0x1b~0x1f 제어 바이트다.
        let control_byte = match key[0] {
            letter if letter.is_ascii_alphabetic() => Some(letter.to_ascii_lowercase() - b'a' + 1),
            b'[' => Some(0x1b),
            b'\\' => Some(0x1c),
            b']' => Some(0x1d),
            b'^' => Some(0x1e),
            b'_' | b'/' => Some(0x1f),
            _ => None,
        };
        result = control_byte.map(|byte| {
            let mut out = Vec::new();
            if input.alt {
                out.push(0x1b);
            }
            out.push(byte);
            out
        });
    }
    if result.is_none() {
        result = input.key_char.map(|text| {
            let mut out = Vec::new();
            if input.alt {
                out.push(0x1b);
            }
            out.extend_from_slice(text.as_bytes());
            out
        });
    }
    result
}

/// 클립보드 키가 하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardKey {
    /// 선택한 글자를 클립보드로 복사한다.
    Copy,
    /// 클립보드 글자를 터미널에 붙여넣는다.
    Paste,
}

/// 키 누름이 복사·붙여넣기인지. 아니면 None이고, 그 키는 터미널로 간다.
///
/// 기존 앱(Theia)과 같다: Ctrl+C는 선택한 글자가 있을 때만 복사이고 없으면 셸의 `^C`다. Ctrl+V는 붙여넣기다.
/// `Ctrl+Insert`(복사)·`Ctrl+Shift+V`·`Shift+Insert`(붙여넣기)는 보조 키다. `Ctrl+Shift+C`는 옆 칸 분할 키라 복사가 아니다.
///
/// Examples
/// --------
/// | 키               | 선택 있음 | 결과           |
/// |------------------|-----------|----------------|
/// | Ctrl+C           | 예        | `Copy`         |
/// | Ctrl+C           | 아니오    | None(`^C`)     |
/// | Ctrl+Insert      | 예        | `Copy`         |
/// | Ctrl+V           | (무관)    | `Paste`        |
/// | Ctrl+Shift+V     | (무관)    | `Paste`        |
/// | Shift+Insert     | (무관)    | `Paste`        |
/// | Ctrl+Shift+C     | (무관)    | None           |
pub fn clipboard_key(
    input: &KeyInput<'_>,
    has_selection: bool,
) -> Option<ClipboardKey> {
    let ctrl_only = input.control && !input.shift && !input.alt;
    match input.key {
        "c" if ctrl_only && has_selection => Some(ClipboardKey::Copy),
        "insert" if ctrl_only && has_selection => Some(ClipboardKey::Copy),
        "v" if input.control && !input.alt => Some(ClipboardKey::Paste),
        "insert" if input.shift && !input.control && !input.alt => Some(ClipboardKey::Paste),
        _ => None,
    }
}

/// 붙여넣을 글자가 터미널에 나갈 바이트. 줄바꿈(`\n`·`\r\n`)은 Enter(`\r`) 하나로 바꾼다(xterm.js와 같다).
/// 터미널 프로그램이 붙여넣기 감싸기를 켰으면 바꾼 글자를 `ESC[200~ … ESC[201~`로 감싼다.
///
/// Examples
/// --------
/// | 글자      | 감싸기 | 결과                          |
/// |-----------|--------|-------------------------------|
/// | `a\nb`    | 끔     | `a\rb`                        |
/// | `a\r\nb`  | 끔     | `a\rb`                        |
/// | `x\ny`    | 켬     | `ESC[200~x\ryESC[201~`        |
pub fn paste_bytes(
    text: &str,
    bracketed: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    out.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> KeyInput<'_> {
        KeyInput { key: name, key_char: None, control: false, shift: false, alt: false }
    }

    #[test]
    fn test_clipboard_keys_follow_the_old_app_ctrl_c_copies_only_with_a_selection_and_ctrl_v_pastes() {
        let ctrl = |name, shift| KeyInput { key: name, key_char: None, control: true, shift, alt: false };
        assert_eq!(clipboard_key(&ctrl("c", false), true), Some(ClipboardKey::Copy));
        assert_eq!(clipboard_key(&ctrl("c", false), false), None, "without a selection Ctrl+C is the shell's ^C");
        assert_eq!(clipboard_key(&ctrl("insert", false), true), Some(ClipboardKey::Copy));
        assert_eq!(clipboard_key(&ctrl("insert", false), false), None);
        for has_selection in [true, false] {
            assert_eq!(clipboard_key(&ctrl("v", false), has_selection), Some(ClipboardKey::Paste));
            assert_eq!(clipboard_key(&ctrl("v", true), has_selection), Some(ClipboardKey::Paste));
            let shift_insert = KeyInput { key: "insert", key_char: None, control: false, shift: true, alt: false };
            assert_eq!(clipboard_key(&shift_insert, has_selection), Some(ClipboardKey::Paste));
            // Ctrl+Shift+C는 분할 키라 클립보드 키가 아니다.
            assert_eq!(clipboard_key(&ctrl("c", true), has_selection), None);
            assert_eq!(clipboard_key(&key("c"), has_selection), None);
            assert_eq!(clipboard_key(&key("v"), has_selection), None);
        }
    }

    fn bytes(
        input: &KeyInput<'_>,
        app_cursor: bool,
    ) -> Option<Vec<u8>> {
        encode_key(input, app_cursor)
    }

    #[test]
    fn test_named_keys() {
        assert_eq!(bytes(&key("enter"), false), Some(b"\r".to_vec()));
        assert_eq!(bytes(&key("backspace"), false), Some(vec![0x7f]));
        assert_eq!(bytes(&key("escape"), false), Some(vec![0x1b]));
        assert_eq!(bytes(&key("tab"), false), Some(b"\t".to_vec()));
        assert_eq!(bytes(&KeyInput { shift: true, ..key("tab") }, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(bytes(&key("delete"), false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(bytes(&key("pageup"), false), Some(b"\x1b[5~".to_vec()));
    }

    #[test]
    fn test_arrows_follow_application_cursor_mode() {
        assert_eq!(bytes(&key("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(bytes(&key("up"), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(bytes(&key("home"), true), Some(b"\x1bOH".to_vec()));
    }

    #[test]
    fn test_control_letters_and_space() {
        assert_eq!(bytes(&KeyInput { control: true, ..key("c") }, false), Some(vec![0x03]));
        assert_eq!(bytes(&KeyInput { control: true, ..key("a") }, false), Some(vec![0x01]));
        assert_eq!(bytes(&KeyInput { control: true, ..key("space") }, false), Some(vec![0]));
    }

    #[test]
    fn test_control_punctuation_gives_control_bytes() {
        // Ctrl+[ \\ ] ^ _ 와 Ctrl+/ 는 글자가 아니라 0x1b~0x1f 제어 바이트로 나간다.
        for (name, byte) in [("[", 0x1b), ("\\", 0x1c), ("]", 0x1d), ("^", 0x1e), ("_", 0x1f), ("/", 0x1f)] {
            let input = KeyInput { control: true, key_char: Some(name), ..key(name) };
            assert_eq!(bytes(&input, false), Some(vec![byte]), "ctrl+{name}");
        }
    }

    #[test]
    fn test_alt_with_named_keys_gets_escape_prefix() {
        let alt = |name| KeyInput { alt: true, ..key(name) };
        assert_eq!(bytes(&alt("backspace"), false), Some(b"\x1b\x7f".to_vec()));
        assert_eq!(bytes(&alt("enter"), false), Some(b"\x1b\r".to_vec()));
        assert_eq!(bytes(&alt("escape"), false), Some(b"\x1b\x1b".to_vec()));
        // 방향키·편집 키는 xterm 방식으로 수정 키 번호(Alt = 3)를 넣는다.
        assert_eq!(bytes(&alt("left"), false), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(bytes(&alt("left"), true), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(bytes(&alt("delete"), false), Some(b"\x1b[3;3~".to_vec()));
        assert_eq!(bytes(&alt("pageup"), false), Some(b"\x1b[5;3~".to_vec()));
    }

    #[test]
    fn test_alt_with_control_letter_gets_escape_prefix() {
        let input = KeyInput { alt: true, control: true, ..key("c") };
        assert_eq!(bytes(&input, false), Some(vec![0x1b, 0x03]));
    }

    #[test]
    fn test_text_and_alt_prefix() {
        let plain = KeyInput { key_char: Some("가"), ..key("g") };
        assert_eq!(bytes(&plain, false), Some("가".as_bytes().to_vec()));
        let alt = KeyInput { alt: true, key_char: Some("x"), ..key("x") };
        assert_eq!(bytes(&alt, false), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn test_unmapped_key_without_text_is_none() {
        assert_eq!(bytes(&key("f13"), false), None);
        assert_eq!(bytes(&KeyInput { shift: true, ..key("shift") }, false), None);
    }

    #[test]
    fn test_paste_bytes_turns_every_line_break_into_one_enter_like_xterm_js() {
        // 붙여넣은 글자의 줄바꿈은 터미널에서 Enter(\r)여야 셸이 명령으로 받는다. \r\n은 하나의 Enter다.
        assert_eq!(paste_bytes("a\nb", false), b"a\rb".to_vec());
        assert_eq!(paste_bytes("a\r\nb", false), b"a\rb".to_vec());
        assert_eq!(paste_bytes("a\n\nb\r\n", false), b"a\r\rb\r".to_vec());
        assert_eq!(paste_bytes("a\rb", false), b"a\rb".to_vec(), "a lone CR is already an Enter");
        // 감싸기 모드에서도 안쪽 줄바꿈을 바꾼 뒤 감싼다.
        assert_eq!(paste_bytes("x\r\ny\nz", true), b"\x1b[200~x\ry\rz\x1b[201~".to_vec());
    }

    #[test]
    fn test_paste_bytes_brackets_only_when_requested() {
        assert_eq!(paste_bytes("a", false), b"a".to_vec());
        assert_eq!(paste_bytes("a", true), b"\x1b[200~a\x1b[201~".to_vec());
    }
}
