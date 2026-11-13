//! 편집기의 "다음 같은 글자 선택 추가"(Cmd·Ctrl+D)가 고르는 범위의 규칙.
//!
//! 글자를 읽고 선택을 바꾸는 일은 앱이 하고, 여기서는 글과 선택 범위(UTF-8 바이트 위치)만 받아 계산하는 순수 규칙만 둔다.
use std::ops::Range;

/// 단어를 이루는 글자: 글자·숫자·밑줄.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// `offset`이 놓인 단어의 범위. 단어 위나 바로 끝(오른쪽 끝)에 있을 때만 있고, 공백·기호 사이면 None이다.
///
/// Examples
/// --------
/// | 글             | offset | 결과       |
/// |----------------|--------|------------|
/// | `foo bar`      | 1      | `0..3`     |
/// | `foo bar`      | 3      | `0..3`     |
/// | `foo bar`      | 4      | `4..7`     |
/// | `foo  bar`     | 4      | None       |
pub fn word_at(
    text: &str,
    offset: usize,
) -> Option<Range<usize>> {
    let offset = offset.min(text.len());
    let mut start = offset;
    for (index, ch) in text[..offset].char_indices().rev() {
        if is_word_char(ch) {
            start = index;
        } else {
            break;
        }
    }
    let mut end = offset;
    for (index, ch) in text[offset..].char_indices() {
        if is_word_char(ch) {
            end = offset + index + ch.len_utf8();
        } else {
            break;
        }
    }
    (start < end).then_some(start..end)
}

/// 직전 "다음 같은 글자 선택 추가"가 남긴 기록. 편집기는 처음 고른 선택(`selected`)만 읽을 수 있고 뒤에 더한 선택은 읽을 수 없으므로,
/// 다음 번에 어디부터 찾을지 알려면 마지막으로 더한 자리(`added`)를 따로 기억해야 한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccurrenceTrail {
    /// 더할 때 읽은 처음 선택의 범위.
    pub selected: Range<usize>,
    /// 마지막으로 더한 자리.
    pub added: Range<usize>,
    /// 더한 직후 글의 길이(바이트). 글이 고쳐져 기록이 낡았는지 가르는 데 쓴다.
    pub text_len: usize,
}

/// 선택 `selected`와 같은 글자가 다음으로 나오는 범위. 직전 기록 `trail`이 지금 선택·글과 맞으면 그 기록의 마지막 자리 뒤부터,
/// 아니면 `selected` 뒤부터 찾는다. 끝까지 없으면 글 처음부터 앞쪽에서 다시 찾는다(이미 고른 자리가 나올 수 있다).
/// 같은 글자가 없거나 `selected`가 비었으면 None이다. 단어 경계는 따지지 않는다(고른 글자 그대로 찾는다).
///
/// Examples
/// --------
/// | 글             | selected | trail의 마지막 자리 | 결과     |
/// |----------------|----------|---------------------|----------|
/// | `ab ab ab`     | `0..2`   | 없음                | `3..5`   |
/// | `ab ab ab`     | `0..2`   | `3..5`              | `6..8`   |
/// | `ab ab ab`     | `6..8`   | 없음                | `0..2`   |
/// | `ab cd`        | `0..2`   | 없음                | None     |
pub fn next_occurrence(
    text: &str,
    selected: Range<usize>,
    trail: Option<&OccurrenceTrail>,
) -> Option<Range<usize>> {
    let needle = text.get(selected.clone()).filter(|needle| !needle.is_empty())?;
    let anchor = trail
        .filter(|trail| trail.selected == selected && trail.text_len == text.len())
        .map_or(selected.clone(), |trail| trail.added.clone());
    let after = text.get(anchor.end..).and_then(|rest| rest.find(needle)).map(|index| anchor.end + index);
    let before = text.get(..anchor.start).and_then(|rest| rest.find(needle));
    after.or(before).map(|start| start..start + needle.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_word_under_the_cursor_includes_its_right_edge_but_not_a_gap() {
        assert_eq!(word_at("foo bar", 1), Some(0..3));
        assert_eq!(word_at("foo bar", 3), Some(0..3));
        assert_eq!(word_at("foo bar", 4), Some(4..7));
        assert_eq!(word_at("foo  bar", 4), None);
        assert_eq!(word_at("", 0), None);
        assert_eq!(word_at("a_b1.c", 2), Some(0..4));
    }

    #[test]
    fn test_words_are_cut_on_character_boundaries_for_non_ascii_text() {
        // 한글은 글자 하나가 3바이트다.
        assert_eq!(word_at("가나 다", 3), Some(0..6));
        assert_eq!(word_at("가나 다", 100), Some(7..10));
    }

    #[test]
    fn test_the_next_occurrence_comes_after_the_selection_and_wraps_to_the_start() {
        assert_eq!(next_occurrence("ab ab ab", 0..2, None), Some(3..5));
        assert_eq!(next_occurrence("ab ab ab", 3..5, None), Some(6..8));
        assert_eq!(next_occurrence("ab ab ab", 6..8, None), Some(0..2));
        assert_eq!(next_occurrence("ab cd", 0..2, None), None);
        assert_eq!(next_occurrence("ab ab", 2..2, None), None);
        assert_eq!(next_occurrence("가나 가나", 0..6, None), Some(7..13));
    }

    #[test]
    fn test_a_trail_that_matches_the_selection_resumes_after_the_last_added_place() {
        let text = "ab ab ab";
        let trail = OccurrenceTrail { selected: 0..2, added: 3..5, text_len: text.len() };
        assert_eq!(next_occurrence(text, 0..2, Some(&trail)), Some(6..8));
        // 선택이 바뀌었거나 글이 고쳐졌으면 기록은 낡은 것이라 처음 선택 뒤부터 찾는다.
        assert_eq!(next_occurrence(text, 6..8, Some(&trail)), Some(0..2));
        let stale = OccurrenceTrail { text_len: text.len() + 1, ..trail };
        assert_eq!(next_occurrence(text, 0..2, Some(&stale)), Some(3..5));
    }
}
