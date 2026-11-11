//! 팝업 메뉴의 키보드 이동: ↑·↓·Home·End로 강조 항목을 옮기고 Enter로 강조 항목을 실행한다.
//! 화면을 모르는 순수 규칙이라 창 없이 시험한다. 마우스를 올린 항목도 같은 강조 하나를 공유한다.

/// 메뉴 안 이동 키.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Home,
    End,
}

impl NavKey {
    /// 창 도구의 키 이름(`up`·`down`·`home`·`end`)이 이동 키이면 그 키다.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "home" => Some(Self::Home),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

/// 열린 메뉴의 강조 항목. 메뉴를 막 열었을 때는 강조가 없다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuNav {
    pub highlight: Option<usize>,
}

impl MenuNav {
    /// `key`만큼 강조를 옮긴다. 항목이 없으면 강조도 없다.
    ///
    /// 끝을 넘으면 반대쪽 끝으로 돈다(기존 앱의 메뉴와 같다). 강조가 없을 때 ↓는 첫 항목, ↑는 마지막 항목이다.
    /// 구분선은 항목 위에 그어진 선일 뿐 항목이 아니므로 이동에 영향이 없다.
    ///
    /// Examples
    /// --------
    /// | 항목 수 | 강조 | 키   | 결과 |
    /// |---------|------|------|------|
    /// | 3       | 없음 | ↓    | 0    |
    /// | 3       | 없음 | ↑    | 2    |
    /// | 3       | 2    | ↓    | 0    |
    /// | 3       | 0    | ↑    | 2    |
    /// | 3       | 1    | Home | 0    |
    /// | 3       | 1    | End  | 2    |
    /// | 0       | 없음 | ↓    | 없음 |
    pub fn step(
        &mut self,
        key: NavKey,
        n_items: usize,
    ) {
        self.highlight = if n_items == 0 {
            None
        } else {
            let last = n_items - 1;
            Some(match (key, self.highlight) {
                (NavKey::Home, _) => 0,
                (NavKey::End, _) => last,
                (NavKey::Down, None) => 0,
                (NavKey::Down, Some(at)) => (at + 1) % n_items,
                (NavKey::Up, None) => last,
                (NavKey::Up, Some(at)) => (at + n_items - 1) % n_items,
            })
        };
    }

    /// 마우스를 올린 항목으로 강조를 옮긴다.
    pub fn hover(
        &mut self,
        index: usize,
    ) {
        self.highlight = Some(index);
    }

    /// Enter가 실행할 항목의 번호. 강조가 항목 수 안에 있을 때만 있다.
    pub fn chosen(
        &self,
        n_items: usize,
    ) -> Option<usize> {
        self.highlight.filter(|at| *at < n_items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(
        keys: &[NavKey],
        n_items: usize,
    ) -> Option<usize> {
        let mut nav = MenuNav::default();
        for key in keys {
            nav.step(*key, n_items);
        }
        nav.highlight
    }

    #[test]
    fn test_c_sync_9_arrow_keys_start_at_the_first_or_last_item_and_wrap_around_the_ends() {
        assert_eq!(after(&[NavKey::Down], 3), Some(0));
        assert_eq!(after(&[NavKey::Up], 3), Some(2));
        assert_eq!(after(&[NavKey::Down, NavKey::Down, NavKey::Down, NavKey::Down], 3), Some(0));
        assert_eq!(after(&[NavKey::Down, NavKey::Up], 3), Some(2));
        assert_eq!(after(&[NavKey::Down], 1), Some(0));
        assert_eq!(after(&[NavKey::Up, NavKey::Up], 1), Some(0));
    }

    #[test]
    fn test_c_sync_10_home_and_end_jump_to_the_ends_from_anywhere() {
        assert_eq!(after(&[NavKey::Down, NavKey::Down, NavKey::Home], 4), Some(0));
        assert_eq!(after(&[NavKey::End], 4), Some(3));
        assert_eq!(after(&[NavKey::Down, NavKey::End, NavKey::Home], 4), Some(0));
    }

    #[test]
    fn test_c_sync_11_an_empty_menu_has_no_highlight_and_enter_runs_only_a_highlight_inside_the_items() {
        assert_eq!(after(&[NavKey::Down, NavKey::End], 0), None);
        let mut nav = MenuNav::default();
        assert_eq!(nav.chosen(3), None, "nothing is highlighted yet");
        nav.hover(1);
        assert_eq!(nav.chosen(3), Some(1), "the mouse and the keys share one highlight");
        nav.step(NavKey::Down, 3);
        assert_eq!(nav.chosen(3), Some(2));
        assert_eq!(nav.chosen(2), None, "a stale highlight beyond the items runs nothing");
        assert_eq!(NavKey::from_name("home"), Some(NavKey::Home));
        assert_eq!(NavKey::from_name("enter"), None);
    }
}
