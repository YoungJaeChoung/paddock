//! 터미널 안 찾기의 검색: 스크롤백까지 포함한 격자에서 질의와 일치하는 구간을 모두 찾고, 하나를 선택으로 강조해 그 줄로 스크롤한다.
//!
//! 질의는 정규식이 아니라 글자 그대로 찾는다. 대문자가 하나도 없는 질의는 대소문자를 가리지 않고, 대문자가 있으면 가린다
//! (터미널 검색 엔진의 기본 규칙이다). 창을 모르는 순수 계산이라 창 없이 시험한다.
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use crate::terminal::grid::Grid;

/// 한 번에 찾는 일치의 상한. 스크롤백이 가득 찬 터미널에서 한 글자 질의가 창을 오래 붙잡지 않게 한다.
pub const MAX_MATCHES: usize = 10_000;

/// 질의를 글자 그대로 찾는 정규식으로 바꾼다. 정규식에서 뜻이 있는 글자 앞에 `\`를 붙인다.
///
/// Examples
/// --------
/// | 질의      | 결과       |
/// |-----------|------------|
/// | `needle`  | `needle`   |
/// | `a.b`     | `a\.b`     |
/// | `(x)+[1]` | `\(x\)\+\[1\]` |
pub fn literal_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len());
    for c in query.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// 격자 전체(스크롤백 포함)에서 `query`와 일치하는 구간을 위에서 아래, 왼쪽에서 오른쪽 순서로 모두 찾는다. 질의가 비면 없다.
/// 상한(`MAX_MATCHES`)을 넘는 일치는 버린다.
pub fn find_matches(
    grid: &Grid,
    query: &str,
) -> Vec<Match> {
    let mut found = Vec::new();
    if !query.is_empty()
        && let Ok(mut regex) = RegexSearch::new(&literal_pattern(query))
    {
        let term = grid.term.lock();
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        found = RegexIter::new(start, end, Direction::Right, &*term, &mut regex).take(MAX_MATCHES).collect();
    }
    found
}

/// 일치 구간을 선택으로 만들어 강조하고, 화면 밖이면 그 줄이 보이게 스크롤한다.
pub fn show_match(
    grid: &Grid,
    found: &Match,
) {
    let mut term = grid.term.lock();
    let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
    selection.update(*found.end(), Side::Right);
    term.selection = Some(selection);
    term.scroll_to_point(*found.start());
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;

    fn grid_with(text: &str) -> Grid {
        let grid = Grid::new(20, 4, |_| {}, |_, _| {});
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *grid.term.lock(), text.as_bytes());
        grid
    }

    /// 일치 구간이 가리키는 글자(한 줄 안의 구간만).
    fn texts(
        grid: &Grid,
        found: &[Match],
    ) -> Vec<String> {
        let term = grid.term.lock();
        found
            .iter()
            .map(|m| {
                let line = &term.grid()[m.start().line];
                (m.start().column.0..=m.end().column.0).map(|c| line[Column(c)].c).collect()
            })
            .collect()
    }

    #[test]
    fn test_c_d71_find_in_terminal_matches_text_literally_in_screen_order_with_smart_case() {
        let grid = grid_with("Needle one\r\nneedle two\r\na.b a-b");
        // 소문자 질의는 대소문자를 가리지 않는다.
        let found = find_matches(&grid, "needle");
        assert_eq!(texts(&grid, &found), ["Needle", "needle"]);
        // 대문자가 있으면 가린다.
        assert_eq!(texts(&grid, &find_matches(&grid, "Needle")), ["Needle"]);
        // 정규식 글자는 글자 그대로다: `a.b`는 `a-b`와 일치하지 않는다.
        assert_eq!(texts(&grid, &find_matches(&grid, "a.b")), ["a.b"]);
        assert!(find_matches(&grid, "").is_empty());
        assert!(find_matches(&grid, "absent").is_empty());
    }

    #[test]
    fn test_c_d71_find_in_terminal_reaches_the_scrollback_and_scrolls_to_the_match() {
        let grid = grid_with("alpha\r\nbeta\r\ngamma\r\ndelta\r\nepsilon\r\nzeta\r\neta");
        assert!(grid.term.lock().history_size() > 0, "the first lines moved into the scrollback");
        let found = find_matches(&grid, "alpha");
        assert_eq!(found.len(), 1);
        assert!(found[0].start().line.0 < 0, "above the screen");
        show_match(&grid, &found[0]);
        let term = grid.term.lock();
        assert!(term.grid().display_offset() > 0, "scrolled up to the match");
        assert_eq!(term.selection_to_string().as_deref(), Some("alpha"));
    }

    #[test]
    fn test_literal_pattern_escapes_every_regex_character() {
        assert_eq!(literal_pattern("needle"), "needle");
        assert_eq!(literal_pattern("a.b"), "a\\.b");
        assert_eq!(literal_pattern("(x)+[1]"), "\\(x\\)\\+\\[1\\]");
        assert_eq!(literal_pattern("C:\\path"), "C:\\\\path");
        assert_eq!(literal_pattern("한글"), "한글");
    }

    #[test]
    fn test_c_d71_find_in_terminal_keys_never_reach_the_pty() {
        use paddock_core::app_shortcut::{AppShortcut, app_shortcut, terminal_key_bytes};
        use paddock_core::key_encoding::KeyInput;

        let press = |key: &'static str, shift: bool| KeyInput { key, key_char: None, control: true, shift, alt: false };
        // 찾기 키(Ctrl+Shift+H)는 앱 단축키라 PTY로 나가지 않고, 평 Ctrl+H만 셸의 백스페이스로 간다.
        assert_eq!(app_shortcut(&press("h", true)), Some(AppShortcut::FindInTerminal));
        assert_eq!(terminal_key_bytes(&press("h", true), false), None);
        assert_eq!(terminal_key_bytes(&press("h", false), false), Some(vec![0x08]));
        // 찾고 강조하고 스크롤하는 일 자체도 PTY에 아무것도 쓰지 않는다.
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let recorded = sent.clone();
        let grid = Grid::new(20, 4, move |bytes| recorded.lock().unwrap().extend(bytes), |_, _| {});
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *grid.term.lock(), b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
        let found = find_matches(&grid, "one");
        show_match(&grid, &found[0]);
        assert!(sent.lock().unwrap().is_empty());
    }
}
