//! 텍스트 파일을 편집기에 올리고 다시 디스크에 쓸 때의 바이트 규칙: UTF-8 BOM·줄바꿈 종류를 읽을 때 기억해 두었다가 저장할 때 되살린다.
//!
//! 편집기는 BOM이 없는 글자를 쥐므로, 읽을 때 뗀 BOM과 줄바꿈 종류를 `Encoded`에 남기고 `encode_for_save`가 그대로 되돌린다.
//! 디스크를 읽고 쓰는 일은 앱이 하고, 여기서는 바이트와 글자 사이의 순수 규칙만 둔다.
use crate::file_kind::FileKind;

/// UTF-8 BOM 바이트.
const BOM: &[u8] = b"\xef\xbb\xbf";

/// 파일이 쓰는 줄바꿈 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

/// 읽을 때 기억해 저장 때 되살리는 것: BOM 유무, 줄바꿈 종류, 그리고 UTF-8이 아니어서 글자를 바꿔 읽었는지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub bom: bool,
    pub ending: LineEnding,
    /// UTF-8로 읽을 수 없는 바이트를 대체 글자로 바꿔 읽었다. 저장하면 원래 바이트가 사라지므로 이런 파일은 고치지 않는다.
    pub lossy: bool,
}

impl Default for Encoded {
    fn default() -> Self {
        Self { bom: false, ending: LineEnding::Lf, lossy: false }
    }
}

/// 줄바꿈 종류를 센다: `\r\n`이 단독 `\n`보다 많으면 `CrLf`, 아니면(같거나 줄바꿈이 없으면 포함) `Lf`.
/// 섞인 파일은 더 많은 쪽을 따른다.
///
/// Examples
/// --------
/// | text           | 결과   |
/// |----------------|--------|
/// | `a\r\nb\r\n`   | `CrLf` |
/// | `a\nb\n`       | `Lf`   |
/// | `abc`          | `Lf`   |
/// | `a\r\nb\nc\n`  | `Lf`   |
/// | `a\r\nb\r\nc\n`| `CrLf` |
pub fn detect_line_ending(text: &str) -> LineEnding {
    let n_crlf = text.matches("\r\n").count();
    let n_lf = text.matches('\n').count() - n_crlf;
    if n_crlf > n_lf { LineEnding::CrLf } else { LineEnding::Lf }
}

/// 디스크에서 읽은 바이트를 편집기에 올릴 글자와 되살릴 정보로 나눈다. BOM은 글자에서 떼고, UTF-8이 아닌 바이트는 대체 글자로 바꾸며 `lossy`를 켠다.
/// 글자는 줄바꿈을 건드리지 않은 채(`\r\n` 그대로) 돌려준다 — 편집기가 그대로 쥐고 보여 준다.
///
/// Examples
/// --------
/// | bytes                 | text     | bom   | ending | lossy |
/// |-----------------------|----------|-------|--------|-------|
/// | `EF BB BF` `a\r\nb`   | `a\r\nb` | true  | `CrLf` | false |
/// | `a\nb`                | `a\nb`   | false | `Lf`   | false |
/// | `caf` `E9`            | `caf\u{fffd}` | false | `Lf` | true |
pub fn decode_text(mut bytes: Vec<u8>) -> (String, Encoded) {
    let bom = bytes.starts_with(BOM);
    if bom {
        bytes.drain(..BOM.len());
    }
    let (text, lossy) = match String::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(error) => (String::from_utf8_lossy(error.as_bytes()).into_owned(), true),
    };
    let ending = detect_line_ending(&text);
    (text, Encoded { bom, ending, lossy })
}

/// 편집기의 글자를 디스크에 쓸 바이트로 만든다: 줄바꿈이 `CrLf`인 파일은 단독 `\n`(편집 중 Enter로 생긴 것)을 `\r\n`으로 맞추고,
/// BOM이 있던 파일은 BOM을 앞에 다시 붙인다. 읽은 뒤 고치지 않은 글자는 읽은 바이트와 같게 돌아간다.
///
/// Examples
/// --------
/// | text          | bom   | ending | 바이트                      |
/// |---------------|-------|--------|-----------------------------|
/// | `a\r\nb`      | true  | `CrLf` | `EF BB BF` `a\r\nb`         |
/// | `a\nb`        | false | `CrLf` | `a\r\nb` (고치다 생긴 `\n`) |
/// | `a\r\nb`      | false | `Lf`   | `a\r\nb` (그대로)           |
/// | `a\nb`        | false | `Lf`   | `a\nb`                      |
pub fn encode_for_save(
    text: &str,
    encoded: &Encoded,
) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len() + BOM.len() + text.len() / 32);
    if encoded.bom {
        bytes.extend_from_slice(BOM);
    }
    match encoded.ending {
        LineEnding::Lf => bytes.extend_from_slice(text.as_bytes()),
        LineEnding::CrLf => {
            let mut previous = 0u8;
            for &byte in text.as_bytes() {
                if byte == b'\n' && previous != b'\r' {
                    bytes.push(b'\r');
                }
                bytes.push(byte);
                previous = byte;
            }
        }
    }
    bytes
}

/// 편집기로 고칠 수 있는 종류인가. 텍스트와 Markdown 원문만이다(이미지·PDF·안내·변경 비교는 읽기 전용).
///
/// Examples
/// --------
/// | kind                | 결과  |
/// |---------------------|-------|
/// | `Text`, `Markdown`  | true  |
/// | `Image`, `Pdf`, `Binary`, `Diff` | false |
pub fn can_edit(kind: FileKind) -> bool {
    matches!(kind, FileKind::Text | FileKind::Markdown)
}

/// 줄로 가기 창에 적은 글자에서 갈 줄(1부터)을 구한다. 앞뒤 공백은 무시하고, 숫자가 아니거나 0이면 None이며, 파일 줄 수(`n_lines`)보다 크면 마지막 줄이다.
///
/// Examples
/// --------
/// | typed   | n_lines | 결과     |
/// |---------|---------|----------|
/// | `42`    | 100     | `Some(42)` |
/// | ` 7 `   | 100     | `Some(7)`  |
/// | `500`   | 100     | `Some(100)`|
/// | `0`     | 100     | `None`   |
/// | `4x`    | 100     | `None`   |
/// | ``      | 100     | `None`   |
pub fn line_target(
    typed: &str,
    n_lines: usize,
) -> Option<usize> {
    let digits = typed.trim();
    let all_digits = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    let line = if all_digits { digits.parse::<usize>().ok() } else { None };
    line.filter(|line| *line >= 1).map(|line| line.min(n_lines.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_edit_1_line_ending_follows_the_majority_and_ties_go_to_lf() {
        let table = [
            ("a\r\nb\r\n", LineEnding::CrLf),
            ("a\nb\n", LineEnding::Lf),
            ("abc", LineEnding::Lf),
            ("a\r\nb\nc\n", LineEnding::Lf),
            ("a\r\nb\r\nc\n", LineEnding::CrLf),
            ("", LineEnding::Lf),
        ];
        for (text, expected) in table {
            assert_eq!(detect_line_ending(text), expected, "{text:?}");
        }
    }

    #[test]
    fn test_c_edit_2_decode_strips_the_bom_and_flags_bytes_that_are_not_utf8() {
        let (text, encoded) = decode_text(b"\xef\xbb\xbfa\r\nb".to_vec());
        assert_eq!((text.as_str(), encoded), ("a\r\nb", Encoded { bom: true, ending: LineEnding::CrLf, lossy: false }));
        let (text, encoded) = decode_text(b"a\nb".to_vec());
        assert_eq!((text.as_str(), encoded), ("a\nb", Encoded::default()));
        let (text, encoded) = decode_text(b"caf\xe9".to_vec());
        assert_eq!(text, "caf\u{fffd}");
        assert!(encoded.lossy);
    }

    #[test]
    fn test_c_edit_3_saving_an_unchanged_text_gives_back_the_bytes_that_were_read() {
        let cases: [&[u8]; 6] = [
            b"a\r\nb\r\n",
            b"\xef\xbb\xbfa\r\nb\r\n",
            b"\xef\xbb\xbfa\nb\n",
            b"a\nb\nc",
            b"",
            "한글 \u{1f600}\r\n".as_bytes(),
        ];
        for bytes in cases {
            let (text, encoded) = decode_text(bytes.to_vec());
            assert_eq!(encode_for_save(&text, &encoded), bytes, "{bytes:?}");
        }
    }

    #[test]
    fn test_c_edit_4_saving_a_crlf_file_turns_a_typed_lone_lf_into_crlf_and_never_doubles_the_cr() {
        let encoded = Encoded { bom: false, ending: LineEnding::CrLf, lossy: false };
        assert_eq!(encode_for_save("a\nb\r\nc", &encoded), b"a\r\nb\r\nc");
        let lf = Encoded::default();
        assert_eq!(encode_for_save("a\nb\r\nc", &lf), b"a\nb\r\nc");
    }

    #[test]
    fn test_c_edit_5_only_text_and_markdown_can_be_edited() {
        assert!(can_edit(FileKind::Text));
        assert!(can_edit(FileKind::Markdown));
        for kind in [
            FileKind::Image,
            FileKind::Svg,
            FileKind::Pdf,
            FileKind::Media,
            FileKind::Binary,
            FileKind::Diff { staged: false },
        ] {
            assert!(!can_edit(kind), "{kind:?}");
        }
    }

    #[test]
    fn test_c_edit_10_go_to_line_takes_digits_only_and_clamps_to_the_last_line() {
        let table = [
            ("42", 100, Some(42)),
            (" 7 ", 100, Some(7)),
            ("500", 100, Some(100)),
            ("1", 0, Some(1)),
            ("0", 100, None),
            ("4x", 100, None),
            ("-3", 100, None),
            ("", 100, None),
            ("99999999999999999999999", 100, None),
        ];
        for (typed, n_lines, expected) in table {
            assert_eq!(line_target(typed, n_lines), expected, "{typed:?}");
        }
    }
}
