//! 글자 키가 라틴 글자를 내는 키보드 배열인지 판정하는 순수 규칙.
//!
//! 한글·러시아어처럼 글자 키가 라틴 글자를 내지 않는 배열에서는 단축키 글자(`b` 같은)가 키 자리와 어긋날 수 있다.
//! 그런 배열을 골라내어 단축키를 키 자리(미국 배열 기준)로 맞추는 쪽이 쓴다.
use std::collections::HashMap;

/// 라틴 배열로 보려면 글자 키 26개 중 이 수 이상이 라틴 글자를 내야 한다. 프랑스어(AZERTY)·독일어(QWERTZ)처럼
/// 자리만 바뀐 배열도 라틴이다.
const N_LATIN_LETTERS_MIN: usize = 13;

/// 글자 키가 라틴 글자를 내는 키보드 배열인지.
///
/// 입력은 키 자리 이름(`KeyA`…`KeyZ`)에서 그 키가 내는 글자로 가는 표다. 글자 키가 하나도 없는 표(비어 있음)는 라틴으로 본다
/// (어긋남이 원래 일어나지 않는다).
///
/// Examples
/// --------
/// | 배열            | KeyA·KeyB가 내는 글자 | 결과  |
/// |-----------------|-----------------------|-------|
/// | 미국(US)        | `a`, `b`              | true  |
/// | 프랑스(AZERTY)  | `q`, `b`              | true  |
/// | 한글 두벌식     | `ㅁ`, `ㅠ`            | false |
/// | 러시아어        | `ф`, `и`              | false |
/// | 빈 표           |                       | true  |
pub fn is_latin_layout(mapping: &HashMap<String, String>) -> bool {
    let codes: Vec<String> = ('A'..='Z').map(|letter| format!("Key{letter}")).collect();
    let n_present = codes.iter().filter(|code| mapping.contains_key(*code)).count();
    let n_latin = codes
        .iter()
        .filter(|code| {
            mapping.get(*code).is_some_and(|value| {
                let mut chars = value.chars();
                matches!((chars.next(), chars.next()), (Some(letter), None) if letter.is_ascii_alphabetic())
            })
        })
        .count();
    n_present == 0 || n_latin >= N_LATIN_LETTERS_MIN
}

/// 비라틴 배열에서 글자 키가 낸 글자(`и`)를 같은 자리의 영문 소문자(`b`)로 바꾼다. 라틴 배열이거나 표에 없는 글자면 None이다.
///
/// 앱 단축키(Ctrl+B 등)는 영문 키 이름으로 정해져 있어, 러시아어 배열에서는 같은 키가 `и`로 와서 맞지 않는다.
/// 표가 같은 자리(`KeyB`)의 글자를 알려 주므로 자리 이름에서 영문자를 돌려준다. 한글 두벌식처럼 키보드 배열 자체는
/// 라틴 글자를 내는 배열은 `is_latin_layout`이 참이라 바뀌지 않는다.
///
/// Examples
/// --------
/// | 배열        | 받은 키 | 결과       |
/// |-------------|---------|------------|
/// | 러시아어    | `и`     | `Some("b")` |
/// | 러시아어    | `b`     | None       |
/// | 미국(US)    | `b`     | None       |
/// | 프랑스(AZERTY) | `q`  | None       |
pub fn latin_equivalent_key(
    mapping: &HashMap<String, String>,
    key: &str,
) -> Option<String> {
    let is_ascii_key = key.is_ascii();
    if is_latin_layout(mapping) || is_ascii_key {
        None
    } else {
        ('A'..='Z')
            .find(|letter| mapping.get(&format!("Key{letter}")).is_some_and(|value| value == key))
            .map(|letter| letter.to_ascii_lowercase().to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn layout(letters: &str) -> HashMap<String, String> {
        ('A'..='Z').zip(letters.chars()).map(|(code, letter)| (format!("Key{code}"), letter.to_string())).collect()
    }

    #[test]
    fn test_c_keyboard_layout_only_layouts_whose_letter_keys_give_no_latin_letters_are_picked_out() {
        assert!(is_latin_layout(&layout("abcdefghijklmnopqrstuvwxyz")));
        assert!(is_latin_layout(&layout("qbcdefghijkl,nopazstuvwxy")));
        assert!(!is_latin_layout(&layout("ㅁㅠㅊㅇㄷㄹㅎㅗㅑㅓㅏㅣㅡㅜㅐㅔㅂㄱㄴㅅㅕㅍㅈㅌㅛㅋ")));
        assert!(!is_latin_layout(&layout("фисвуапршолдьтщзйкыегмцчня")));
        assert!(is_latin_layout(&HashMap::new()));
    }

    #[test]
    fn test_c_keyboard_layout_a_non_latin_letter_key_maps_back_to_its_latin_key_position() {
        let russian = layout("фисвуапршолдьтщзйкыегмцчня");
        assert_eq!(latin_equivalent_key(&russian, "и").as_deref(), Some("b"));
        assert_eq!(latin_equivalent_key(&russian, "ф").as_deref(), Some("a"));
        assert_eq!(latin_equivalent_key(&russian, "я").as_deref(), Some("z"));
        // 영문 이름의 키(Ctrl+Insert 같은 이름 키 포함)와 표에 없는 글자는 그대로 둔다.
        assert_eq!(latin_equivalent_key(&russian, "b"), None);
        assert_eq!(latin_equivalent_key(&russian, "insert"), None);
        assert_eq!(latin_equivalent_key(&russian, "ъ"), None);
        // 라틴 글자를 내는 배열(미국·프랑스·한글 두벌식의 자판 자체)은 바꾸지 않는다.
        assert_eq!(latin_equivalent_key(&layout("abcdefghijklmnopqrstuvwxyz"), "b"), None);
        assert_eq!(latin_equivalent_key(&layout("qbcdefghijkl,nopazstuvwxy"), "q"), None);
    }
}
