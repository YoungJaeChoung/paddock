//! 탭 줄에 초점이 있을 때의 방향키: ←·→·Home·End가 탭을 고른다. 탭 줄 빈 자리를 누르면 초점이 생기고,
//! Enter·Esc나 다른 곳의 누름, 터미널로 가야 하는 다른 키가 초점을 거둔다. 화면을 모르는 순수 규칙이다.
use crate::work_model::TabTarget;

/// 탭 줄 초점이 받는 키.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarKey {
    Left,
    Right,
    Home,
    End,
}

impl TabBarKey {
    /// 키가 옮길 탭. ←는 앞 탭(첫 탭에서는 마지막으로 돈다), →는 뒤 탭, Home·End는 첫·마지막 탭이다.
    pub fn target(self) -> TabTarget {
        match self {
            Self::Left => TabTarget::Previous,
            Self::Right => TabTarget::Next,
            Self::Home => TabTarget::First,
            Self::End => TabTarget::Last,
        }
    }
}

/// 키 누름이 탭 줄 초점이 받는 키이면 그 키를 돌려준다. Ctrl·Alt가 눌린 키는 앱 단축키(탭 이동)나 터미널의 것이라 받지 않는다.
///
/// Examples
/// --------
/// | 키 이름 | Ctrl | Alt | 결과          |
/// |---------|------|-----|---------------|
/// | `left`  | 거짓 | 거짓 | `Left`       |
/// | `end`   | 거짓 | 거짓 | `End`        |
/// | `left`  | 참   | 거짓 | None         |
/// | `a`     | 거짓 | 거짓 | None         |
pub fn tab_bar_key(
    key: &str,
    control: bool,
    alt: bool,
) -> Option<TabBarKey> {
    match (key, control || alt) {
        ("left", false) => Some(TabBarKey::Left),
        ("right", false) => Some(TabBarKey::Right),
        ("home", false) => Some(TabBarKey::Home),
        ("end", false) => Some(TabBarKey::End),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_sync_12_tab_bar_focus_takes_the_four_plain_keys_and_maps_them_to_tab_targets() {
        let table = [
            ("left", TabBarKey::Left, TabTarget::Previous),
            ("right", TabBarKey::Right, TabTarget::Next),
            ("home", TabBarKey::Home, TabTarget::First),
            ("end", TabBarKey::End, TabTarget::Last),
        ];
        for (name, key, target) in table {
            assert_eq!(tab_bar_key(name, false, false), Some(key), "{name}");
            assert_eq!(key.target(), target, "{name}");
        }
    }

    #[test]
    fn test_c_sync_13_keys_with_ctrl_or_alt_and_other_keys_leave_the_tab_bar_focus() {
        assert_eq!(tab_bar_key("left", true, false), None, "Ctrl+Left is the app shortcut");
        assert_eq!(tab_bar_key("right", false, true), None);
        for name in ["a", "enter", "escape", "up", "down", "tab"] {
            assert_eq!(tab_bar_key(name, false, false), None, "{name}");
        }
    }
}
