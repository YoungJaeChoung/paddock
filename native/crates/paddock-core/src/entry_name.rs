//! 새 파일·폴더 이름을 입력할 때 쓰는 검사. 화면을 모르는 순수 규칙이다.

/// 이름 입력 창의 검사 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryNameCheck {
    Valid,
    /// 아직 입력하지 않은 상태. 잘못이 아니므로 경고 없이 확인 단추만 끈다.
    Empty,
    /// 규칙을 어긴 이름. 경고 문장을 보인다.
    Invalid(&'static str),
}

/// 새 파일·폴더 이름이 규칙에 맞는지 검사한다. 한 번에 한 단계 이름만 받으므로 `/`·`\`를 쓸 수 없고,
/// `.`·`..`은 예약된 이름이며 제어 문자는 쓸 수 없다. 앞뒤 공백과 끝 마침표는 Windows가 말없이 지워 다른 이름이 되므로 모든 플랫폼에서 막는다.
/// 공백뿐인 이름은 `Empty`다.
///
/// Examples
/// --------
/// | 입력          | 결과                              |
/// |---------------|-----------------------------------|
/// | `notes.md`    | `Valid`                           |
/// | `` / `  `     | `Empty`                           |
/// | `a/b`, `a\b`  | `Invalid` (`/`·`\`를 쓸 수 없음)  |
/// | `.`, `..`     | `Invalid` (예약된 이름)           |
/// | ` a`, `a `    | `Invalid` (앞뒤 공백)            |
/// | `a.`          | `Invalid` (끝 마침표)             |
pub fn validate_entry_name(value: &str) -> EntryNameCheck {
    if value.trim().is_empty() {
        EntryNameCheck::Empty
    } else if value.contains(['/', '\\']) {
        EntryNameCheck::Invalid("A name can\u{2019}t contain / or \\. Create one level at a time.")
    } else if value == "." || value == ".." {
        EntryNameCheck::Invalid("\u{201c}.\u{201d} and \u{201c}..\u{201d} are reserved. Choose another name.")
    } else if value.starts_with(char::is_whitespace) || value.ends_with(char::is_whitespace) {
        EntryNameCheck::Invalid("A name can\u{2019}t start or end with a space.")
    } else if value.ends_with('.') {
        EntryNameCheck::Invalid("A name can\u{2019}t end with a period.")
    } else if value.chars().any(|c| c <= '\u{1f}') {
        EntryNameCheck::Invalid("Remove control characters from the name.")
    } else {
        EntryNameCheck::Valid
    }
}

/// 같은 이름이 있는 폴더에 붙여 넣거나 끌어 놓은 항목에 붙일 `n`번째 다른 이름. 덮어쓰지도 이름을 묻지도 않고 이어 받게 한다.
/// 호출자는 0부터 `n`을 올리며 비어 있는 이름을 찾는다. 점으로 시작하고 다른 점이 없는 이름(`.env`)은 이름 전체를 앞부분으로 본다.
///
/// Examples
/// --------
/// | name        | n | 결과               |
/// |-------------|---|--------------------|
/// | `image.png` | 0 | `image.png`        |
/// | `image.png` | 1 | `image copy.png`   |
/// | `image.png` | 2 | `image copy 2.png` |
/// | `a.tar.gz`  | 1 | `a.tar copy.gz`    |
/// | `.env`      | 1 | `.env copy`        |
/// | `Makefile`  | 3 | `Makefile copy 3`  |
pub fn copy_entry_name(
    name: &str,
    n: usize,
) -> String {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let suffix = match n {
        0 => String::new(),
        1 => " copy".to_string(),
        n => format!(" copy {n}"),
    };
    format!("{stem}{suffix}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_entry_name_empty_name_blocks_confirm_silently_and_rule_breaks_give_a_reason() {
        assert_eq!(validate_entry_name("notes.md"), EntryNameCheck::Valid);
        assert_eq!(validate_entry_name("한글 폴더"), EntryNameCheck::Valid);
        assert_eq!(validate_entry_name(""), EntryNameCheck::Empty);
        assert_eq!(validate_entry_name("   "), EntryNameCheck::Empty);
        let reason = |value| match validate_entry_name(value) {
            EntryNameCheck::Invalid(reason) => reason,
            other => panic!("expected a reason for {value:?}, got {other:?}"),
        };
        assert!(reason("a/b").contains("/ or \\"));
        assert!(reason("a\\b").contains("/ or \\"));
        assert!(reason("..").contains("reserved"));
        assert!(reason(".").contains("reserved"));
        assert!(reason(" a").contains("start or end with a space"));
        assert!(reason("a ").contains("start or end with a space"));
        assert!(reason("a.").contains("end with a period"));
        assert_eq!(validate_entry_name(".env"), EntryNameCheck::Valid);
        assert!(reason("a\tb").contains("control characters"));
        assert!(reason("a\u{1f}b").contains("control characters"));
        // 기존 앱은 0x00~0x1f만 막는다. DEL(0x7f)은 허용한다.
        assert_eq!(validate_entry_name("a\u{7f}b"), EntryNameCheck::Valid);
    }

    #[test]
    fn test_c_d69_copy_names_add_copy_and_a_number_before_the_last_extension() {
        let table = [
            ("image.png", 0, "image.png"),
            ("image.png", 1, "image copy.png"),
            ("image.png", 2, "image copy 2.png"),
            ("a.tar.gz", 1, "a.tar copy.gz"),
            (".env", 1, ".env copy"),
            ("Makefile", 3, "Makefile copy 3"),
        ];
        for (name, n, expected) in table {
            assert_eq!(copy_entry_name(name, n), expected, "{name} {n}");
        }
    }
}
