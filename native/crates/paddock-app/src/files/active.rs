//! 키를 받는 탭(터미널 또는 파일 탭)이 선택 변화 뒤 어느 쪽이 되는지의 규칙.
//!
//! 터미널 선택을 바꾸는 모든 경로(새 터미널 열기·고르기·닫기·폴더로 올리기)가 이 규칙 하나를 거친다.

/// 선택이 바뀐 상황.
#[derive(Clone, Copy, Debug)]
pub struct SelectionChange {
    /// 바뀌기 전에 파일 탭이 활성이었다.
    pub was_file_active: bool,
    /// 사용자가 터미널을 새로 열었거나 직접 골랐다.
    pub user_chose_terminal: bool,
    /// 바뀐 뒤에도 같은 묶음을 보고 있다.
    pub same_group: bool,
    /// 지금 보이는 묶음에서 마지막으로 활성이었던 쪽이 파일 탭이었다(묶음이 바뀐 경우에 쓴다).
    pub remembered_file_active: bool,
    /// 지금 보이는 묶음에 파일 칸이 있다.
    pub has_pane: bool,
}

/// 선택이 바뀐 뒤 파일 탭이 계속(또는 다시) 활성인지.
///
/// 사용자가 터미널을 새로 열거나 고르면 터미널이 활성이다. 그 밖에 같은 묶음에 머물면 이전 상태를 지키므로(다른 터미널이 닫히는 것만으로는
/// 바뀌지 않음) 선택한 터미널이 시작 터미널이든 아니든 결과가 같고, 보이는 묶음이 바뀌면 그 묶음에서 마지막으로 활성이었던 쪽이다.
/// 파일 칸이 없으면 늘 터미널이다.
///
/// Examples
/// --------
/// | 사용자가 고름 | 같은 묶음 | 이전 파일 활성 | 묶음의 마지막 | 파일 칸 | 결과  |
/// |---------------|-----------|----------------|---------------|---------|-------|
/// | 예            | 예        | 예             | -             | 예      | 터미널 |
/// | 아니오        | 예        | 예             | -             | 예      | 파일  |
/// | 아니오        | 아니오    | 예             | 파일          | 예      | 파일  |
/// | 아니오        | 아니오    | 예             | 터미널        | 예      | 터미널 |
/// | 아니오        | 예        | 예             | -             | 아니오  | 터미널 |
pub fn file_stays_active(change: &SelectionChange) -> bool {
    let wanted = if change.user_chose_terminal {
        false
    } else if change.same_group {
        change.was_file_active
    } else {
        change.remembered_file_active
    };
    wanted && change.has_pane
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change() -> SelectionChange {
        SelectionChange {
            was_file_active: true,
            user_chose_terminal: false,
            same_group: true,
            remembered_file_active: false,
            has_pane: true,
        }
    }

    #[test]
    fn test_c_d51_opening_or_choosing_a_terminal_makes_the_terminal_active_even_in_the_same_group() {
        assert!(!file_stays_active(&SelectionChange { user_chose_terminal: true, ..change() }));
    }

    #[test]
    fn test_c_d51_closing_another_terminal_of_the_shown_group_keeps_the_file_tab_active_root_or_not() {
        // 선택한 터미널이 시작 터미널이든 아니든 같은 묶음에 머물면 결과가 같다.
        assert!(file_stays_active(&change()));
        assert!(!file_stays_active(&SelectionChange { was_file_active: false, ..change() }));
    }

    #[test]
    fn test_c_d51_a_different_shown_group_comes_back_to_what_was_active_there_last() {
        let other = SelectionChange { same_group: false, ..change() };
        assert!(!file_stays_active(&other));
        assert!(file_stays_active(&SelectionChange { remembered_file_active: true, ..other }));
    }

    #[test]
    fn test_c_d51_without_a_file_pane_the_terminal_is_always_active() {
        assert!(!file_stays_active(&SelectionChange { has_pane: false, remembered_file_active: true, ..change() }));
    }
}
