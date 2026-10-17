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
/// `.`·`..`은 예약된 이름이며 제어 문자는 쓸 수 없다. 공백뿐인 이름은 `Empty`다.
///
/// Examples
/// --------
/// | 입력          | 결과                              |
/// |---------------|-----------------------------------|
/// | `notes.md`    | `Valid`                           |
/// | `` / `  `     | `Empty`                           |
/// | `a/b`, `a\b`  | `Invalid` (`/`·`\`를 쓸 수 없음)  |
/// | `.`, `..`     | `Invalid` (예약된 이름)           |
pub fn validate_entry_name(value: &str) -> EntryNameCheck {
    if value.trim().is_empty() {
        EntryNameCheck::Empty
    } else if value.contains(['/', '\\']) {
        EntryNameCheck::Invalid("A name can\u{2019}t contain / or \\. Create one level at a time.")
    } else if value == "." || value == ".." {
        EntryNameCheck::Invalid("\u{201c}.\u{201d} and \u{201c}..\u{201d} are reserved. Choose another name.")
    } else if value.chars().any(|c| c <= '\u{1f}') {
        EntryNameCheck::Invalid("Remove control characters from the name.")
    } else {
        EntryNameCheck::Valid
    }
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
        assert!(reason("a\tb").contains("control characters"));
        assert!(reason("a\u{1f}b").contains("control characters"));
        // 기존 앱은 0x00~0x1f만 막는다. DEL(0x7f)은 허용한다.
        assert_eq!(validate_entry_name("a\u{7f}b"), EntryNameCheck::Valid);
    }
}
