//! 터미널 안 찾기의 상태 규칙: 질의, 일치 수, 지금 가리키는 일치와 다음·이전 순환, `3 of 12` 글자.
//! 일치를 실제로 찾는 일(스크롤백 검색)은 앱이 하고, 이 모듈은 그 결과를 어떻게 걷는지만 정한다.

/// 찾기 띠의 상태.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindState {
    pub query: String,
    pub n_matches: usize,
    /// 지금 가리키는 일치의 번호(0부터). 일치가 없으면 None이다.
    pub current: Option<usize>,
}

impl FindState {
    /// 질의를 바꾸고 검색 결과를 반영한다. 일치가 있으면 첫 일치를 가리킨다.
    pub fn set_results(
        &mut self,
        query: &str,
        n_matches: usize,
    ) {
        self.query = query.to_string();
        self.n_matches = n_matches;
        self.current = (n_matches > 0).then_some(0);
    }

    /// 다음 일치로 간다. 마지막에서는 첫 일치로 돈다.
    pub fn next(&mut self) {
        self.current = self.current.map(|at| (at + 1) % self.n_matches.max(1));
    }

    /// 이전 일치로 간다. 첫 일치에서는 마지막으로 돈다.
    pub fn previous(&mut self) {
        self.current = self.current.map(|at| (at + self.n_matches.max(1) - 1) % self.n_matches.max(1));
    }

    /// 띠 오른쪽에 보이는 글자. 질의가 비어 있으면 아무것도 보이지 않는다.
    pub fn count_text(&self) -> String {
        if self.query.is_empty() { String::new() } else { count_text(self.current, self.n_matches) }
    }
}

/// 터미널 `closed`가 닫힐 때 찾기 띠(`bar`는 띠가 붙은 터미널)가 남아도 되는지를 정한 뒤의 띠. 닫히는 터미널의 띠는 없어지고 다른 터미널의 띠는 그대로다.
/// 터미널 번호는 데몬이 바뀌면 다시 쓰이므로(새 데몬이 1부터 매긴다), 닫힌 터미널의 띠가 남으면 같은 번호의 새 터미널에 옛 띠가 뜬다.
///
/// Examples
/// --------
/// | 띠가 붙은 터미널 | 닫는 터미널 | 결과     |
/// |------------------|-------------|----------|
/// | 3                | 3           | 없음     |
/// | 3                | 4           | 3        |
/// | 없음             | 3           | 없음     |
pub fn bar_after_close(
    bar: Option<u32>,
    closed: u32,
) -> Option<u32> {
    bar.filter(|terminal| *terminal != closed)
}

/// 일치 위치 글자.
///
/// Examples
/// --------
/// | 현재(0부터) | 일치 수 | 결과         |
/// |-------------|---------|--------------|
/// | 2           | 12      | `3 of 12`    |
/// | 0           | 1       | `1 of 1`     |
/// | 없음        | 0       | `No results` |
pub fn count_text(
    current: Option<usize>,
    n_matches: usize,
) -> String {
    match current {
        Some(at) if n_matches > 0 => format!("{} of {n_matches}", at + 1),
        _ => "No results".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_sync_14_next_and_previous_go_round_the_matches_and_the_count_text_is_one_based() {
        let mut find = FindState::default();
        find.set_results("needle", 3);
        assert_eq!((find.current, find.count_text().as_str()), (Some(0), "1 of 3"));
        find.next();
        find.next();
        assert_eq!(find.count_text(), "3 of 3");
        find.next();
        assert_eq!(find.current, Some(0), "after the last match comes the first");
        find.previous();
        assert_eq!(find.current, Some(2), "before the first match comes the last");
    }

    #[test]
    fn test_c_sync_21_the_bar_of_a_closed_terminal_goes_away_so_a_reused_number_gets_no_old_bar() {
        assert_eq!(bar_after_close(Some(3), 3), None);
        assert_eq!(bar_after_close(Some(3), 4), Some(3));
        assert_eq!(bar_after_close(None, 3), None);
    }

    #[test]
    fn test_c_sync_15_no_match_says_no_results_and_an_empty_query_says_nothing() {
        let mut find = FindState::default();
        find.set_results("zzz", 0);
        assert_eq!((find.current, find.count_text().as_str()), (None, "No results"));
        find.next();
        find.previous();
        assert_eq!(find.current, None);
        find.set_results("", 0);
        assert_eq!(find.count_text(), "");
        assert_eq!(count_text(Some(2), 12), "3 of 12");
    }
}
