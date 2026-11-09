//! 끌고 온 탭을 어디에 놓을지 정하는 순수 규칙: 칸 가장자리 분할·가운데 붙이기·탭 줄 끼우기와 "바뀌는 것이 없는 자리" 판정.
//!
//! 화면 좌표(칸의 사각형·탭의 가로 구간)와 탭 번호만 다룬다. 끌기 이벤트와 그리기는 호출자가 맡는다.
//! 탭 번호는 비교만 하므로 터미널 번호(`u32`)든 파일 탭 열쇠든 `PartialEq + Copy`면 된다.

use crate::layout_model::Side;

/// 칸 가장자리 분할 띠의 폭 비율. 이 안쪽은 그 칸으로 옮기기다.
/// 넓으면 다른 칸으로 옮기려던 놓기가 새 칸 만들기가 되므로 좁게 둔다.
pub const EDGE_SPLIT_RATIO: f32 = 0.1;

/// 탭 하나를 끌 때 보이는 안내 글: 아직 이 창에 있는 탭.
pub const HINT_DETACH: &str = "Drag to an edge to split, or outside for a new window";
/// 탭 하나를 끌 때 보이는 안내 글: 이미 다른 창에 있는 탭.
pub const HINT_MOVE_BACK: &str = "Drop on a pane to move it back to this window";
/// 파일 탭을 끌 때 보이는 안내 글: 이 창에 있는 탭. 파일 탭은 칸에 놓을 수 없어 창 밖만 안내한다.
pub const HINT_DETACH_FILE: &str = "Drag outside for a new window";
/// 파일 탭을 끌 때 보이는 안내 글: 이미 다른 창에 있는 탭.
pub const HINT_MOVE_BACK_FILE: &str = "Drop on the main window to move it back";

/// 놓는 방식. 가장자리 넷은 칸을 나누고, 나머지 둘은 탭 줄에 끼운다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropMode {
    SplitLeft,
    SplitRight,
    SplitTop,
    SplitBottom,
    /// 기준 탭 뒤(칸 가운데에 놓으면 그 칸의 끝)에 붙인다.
    TabAfter,
    /// 기준 탭 앞에 끼운다.
    TabBefore,
}

impl DropMode {
    /// 칸을 나누는 방식이면 새 칸이 놓일 쪽. 탭 줄에 끼우는 방식이면 None이다.
    pub fn side(self) -> Option<Side> {
        match self {
            Self::SplitLeft => Some(Side::Left),
            Self::SplitRight => Some(Side::Right),
            Self::SplitTop => Some(Side::Top),
            Self::SplitBottom => Some(Side::Bottom),
            Self::TabAfter | Self::TabBefore => None,
        }
    }
}

/// 화면의 사각형(왼쪽·위·폭·높이).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// 놓을 자리: 놓는 방식과 놓으면 차지할 영역(미리보기 상자).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropTarget {
    pub mode: DropMode,
    pub rect: Rect,
}

/// 탭 줄의 탭 하나가 차지한 가로 구간.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowTab<T> {
    pub id: T,
    pub left: f32,
    pub right: f32,
}

/// 탭 줄에 끼워 넣을 자리: 기준 탭과 앞·뒤, 끼울 자리의 가로 좌표(`edge`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insert<T> {
    pub mode: DropMode,
    pub reference: T,
    pub edge: f32,
}

/// 칸 안의 한 점이 가리키는 놓을 자리. 칸 밖이거나 크기가 0인 칸이면 None이다.
///
/// 네 가장자리 중 점에 가장 가까운 쪽이 `EDGE_SPLIT_RATIO` 안이면 그 방향으로 나누고(미리보기는 칸의 절반),
/// 아니면 가운데이니 탭으로 붙인다(미리보기는 칸 전체). 가까운 가장자리가 같으면 왼쪽·오른쪽·위·아래 순으로 고른다.
///
/// Examples
/// --------
/// | 칸 (left, top, w, h) | 점          | 결과                       |
/// |----------------------|-------------|----------------------------|
/// | (200, 50, 800, 400)  | (205, 250)  | SplitLeft, 왼쪽 절반       |
/// | (200, 50, 800, 400)  | (600, 250)  | TabAfter, 칸 전체          |
/// | (200, 50, 800, 400)  | (1000, 200) | None (오른쪽 밖)           |
pub fn drop_target(
    rect: Rect,
    x: f32,
    y: f32,
) -> Option<DropTarget> {
    let Rect { left, top, width, height } = rect;
    let mut target = None;
    if width > 0.0 && height > 0.0 && x >= left && y >= top && x < left + width && y < top + height {
        let edges = [
            ((x - left) / width, DropMode::SplitLeft, Rect { left, top, width: width / 2.0, height }),
            (
                (left + width - x) / width,
                DropMode::SplitRight,
                Rect { left: left + width / 2.0, top, width: width / 2.0, height },
            ),
            ((y - top) / height, DropMode::SplitTop, Rect { left, top, width, height: height / 2.0 }),
            (
                (top + height - y) / height,
                DropMode::SplitBottom,
                Rect { left, top: top + height / 2.0, width, height: height / 2.0 },
            ),
        ];
        let nearest = edges.iter().min_by(|first, second| first.0.total_cmp(&second.0));
        target = Some(match nearest {
            Some((ratio, mode, split_rect)) if *ratio < EDGE_SPLIT_RATIO => {
                DropTarget { mode: *mode, rect: *split_rect }
            }
            _ => DropTarget { mode: DropMode::TabAfter, rect },
        });
    }
    target
}

/// 끈 탭 `id`를 칸에 놓을 때 기준이 될 탭. 놓아도 바뀌는 것이 없으면 None이다.
///
/// `pane_ids`는 놓을 칸의 탭, `current`는 그 칸이 지금 보여 주는 탭이다. 끈 탭이 이미 그 칸에 있으면 가운데에 놓아도
/// 갈 곳이 없다 — 기준을 고르면 그 탭 뒤로 옮겨져 의도하지 않은 순서 바꾸기가 되므로 None이다. 다른 칸 가운데에 놓으면
/// 그 칸의 끝 탭을 기준으로 삼아 예측 가능한 끝 자리에 붙인다. 가장자리 분할에서 끈 탭이 그 칸의 현재 탭이면
/// 자기를 뺀 첫 탭이 기준이고, 칸에 탭이 하나뿐이면 나눌 수 없어 None이다.
///
/// Examples
/// --------
/// | mode       | pane_ids  | current | id | 결과                |
/// |------------|-----------|---------|----|---------------------|
/// | SplitRight | [a, b]    | a       | a  | b                   |
/// | SplitRight | [a]       | a       | a  | None                |
/// | TabAfter   | [a, b]    | b       | a  | None (이미 그 칸)   |
/// | TabAfter   | [c]       | c       | a  | c                   |
/// | TabAfter   | [b, c, d] | b       | a  | d (끝에 붙임)       |
pub fn dock_ref<T: PartialEq + Copy>(
    mode: DropMode,
    pane_ids: &[T],
    current: T,
    id: T,
) -> Option<T> {
    if mode == DropMode::TabAfter {
        if pane_ids.contains(&id) { None } else { pane_ids.last().copied() }
    } else if current == id {
        pane_ids.iter().copied().find(|other| *other != id)
    } else {
        Some(current)
    }
}

/// 탭 줄 위에 놓을 때 끼워 넣을 자리. 가리킨 탭의 왼쪽 절반이면 그 탭 앞, 오른쪽 절반이면 뒤다.
///
/// 첫 탭 앞·탭 사이 빈 곳·줄 끝은 오른쪽에서 가장 가까운 탭(없으면 마지막 탭)의 그쪽 가장자리로 본다. 탭이 없으면 None이다.
///
/// Examples
/// --------
/// | 탭                       | x   | 결과                  |
/// |--------------------------|-----|-----------------------|
/// | a 0–100, b 100–200       | 30  | a 앞 (edge 0)         |
/// | a 0–100, b 100–200       | 160 | b 뒤 (edge 200)       |
/// | a 0–100, b 100–200       | 260 | b 뒤 (줄 끝)          |
/// | (없음)                   | 50  | None                  |
pub fn row_insert<T: PartialEq + Copy>(
    tabs: &[RowTab<T>],
    x: f32,
) -> Option<Insert<T>> {
    tabs.iter().find(|tab| x < tab.right).or(tabs.last()).map(|hit| {
        let is_before = x < (hit.left + hit.right) / 2.0;
        Insert {
            mode: if is_before { DropMode::TabBefore } else { DropMode::TabAfter },
            reference: hit.id,
            edge: if is_before { hit.left } else { hit.right },
        }
    })
}

/// 끈 탭을 끼워 넣어도 칸 안 순서가 그대로인지. 끈 탭이 그 칸에 없으면(다른 칸으로 옮김) 늘 false다.
/// 바뀌는 것이 없는 자리에는 미리보기도 이동도 하지 않는다.
///
/// Examples
/// --------
/// | pane_ids  | id | mode       | reference | 결과                 |
/// |-----------|----|------------|-----------|----------------------|
/// | [a, b, c] | a  | TabBefore  | b         | true (a는 이미 b 앞) |
/// | [a, b, c] | a  | TabAfter   | a         | true (자기 자신)     |
/// | [a, b, c] | c  | TabAfter   | b         | true                 |
/// | [a, b, c] | a  | TabAfter   | b         | false                |
/// | [b, c]    | a  | TabBefore  | b         | false (다른 칸)      |
pub fn is_same_place<T: PartialEq + Copy>(
    pane_ids: &[T],
    id: T,
    mode: DropMode,
    reference: T,
) -> bool {
    let from = pane_ids.iter().position(|other| *other == id);
    let at = pane_ids.iter().position(|other| *other == reference);
    match (from, at) {
        (Some(from), Some(at)) => {
            reference == id || if mode == DropMode::TabBefore { at == from + 1 } else { at + 1 == from }
        }
        _ => false,
    }
}

/// 같은 칸 안에서 끈 탭이 옮겨 가 앉을 번호. 끈 탭이 그 칸에 없으면(다른 칸으로 옮김) None이다.
///
/// 끼워 넣는 동안 끈 탭이 먼저 빠지면서 오른쪽 탭들이 한 칸씩 당겨지므로, 이 번호는 빠진 뒤의 번호다.
///
/// Examples
/// --------
/// | pane_ids  | id | mode       | reference | 결과                |
/// |-----------|----|------------|-----------|---------------------|
/// | [a, b, c] | a  | TabBefore  | c         | 1 (b, a, c)         |
/// | [a, b, c] | a  | TabAfter   | b         | 1 (b, a, c)         |
/// | [a, b, c] | a  | TabAfter   | c         | 2 (b, c, a)         |
/// | [a, b, c] | c  | TabBefore  | a         | 0 (c, a, b)         |
/// | [b, c]    | a  | TabBefore  | b         | None (다른 칸)      |
pub fn move_index<T: PartialEq + Copy>(
    pane_ids: &[T],
    id: T,
    mode: DropMode,
    reference: T,
) -> Option<usize> {
    let from = pane_ids.iter().position(|other| *other == id);
    let at = pane_ids.iter().position(|other| *other == reference);
    match (from, at) {
        (Some(from), Some(at)) => {
            let slot = at + usize::from(mode == DropMode::TabAfter);
            Some(slot - usize::from(from < slot))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: Rect = Rect { left: 200.0, top: 50.0, width: 800.0, height: 400.0 };

    #[test]
    fn test_wd01_pane_edges_split_in_the_intended_direction_and_the_center_merges_tabs() {
        for (x, y, mode) in [
            (205.0, 250.0, DropMode::SplitLeft),
            (995.0, 250.0, DropMode::SplitRight),
            (600.0, 55.0, DropMode::SplitTop),
            (600.0, 445.0, DropMode::SplitBottom),
            (600.0, 250.0, DropMode::TabAfter),
            (280.0, 250.0, DropMode::TabAfter),
            (600.0, 100.0, DropMode::TabAfter),
        ] {
            let target = drop_target(RECT, x, y).unwrap();
            assert_eq!(target.mode, mode, "at ({x}, {y})");
            assert!(target.rect.left >= RECT.left && target.rect.top >= RECT.top);
            assert!(target.rect.left + target.rect.width <= RECT.left + RECT.width);
            assert!(target.rect.top + target.rect.height <= RECT.top + RECT.height);
        }
        // 가장자리 분할의 미리보기는 칸의 절반, 가운데는 칸 전체다.
        let left = drop_target(RECT, 205.0, 250.0).unwrap().rect;
        assert_eq!(left, Rect { left: 200.0, top: 50.0, width: 400.0, height: 400.0 });
        let bottom = drop_target(RECT, 600.0, 445.0).unwrap().rect;
        assert_eq!(bottom, Rect { left: 200.0, top: 250.0, width: 800.0, height: 200.0 });
        assert_eq!(drop_target(RECT, 600.0, 250.0).unwrap().rect, RECT);
    }

    #[test]
    fn test_the_edge_modes_name_the_side_of_the_new_pane_and_the_tab_modes_have_none() {
        assert_eq!(DropMode::SplitLeft.side(), Some(Side::Left));
        assert_eq!(DropMode::SplitBottom.side(), Some(Side::Bottom));
        assert_eq!(DropMode::TabAfter.side(), None);
        assert_eq!(DropMode::TabBefore.side(), None);
    }

    #[test]
    fn test_wd02_outside_the_pane_and_zero_size_panes_have_no_drop_destination() {
        for (x, y) in [(199.0, 200.0), (1000.0, 200.0), (500.0, 49.0), (500.0, 450.0)] {
            assert_eq!(drop_target(RECT, x, y), None, "at ({x}, {y})");
        }
        assert_eq!(drop_target(Rect { width: 0.0, ..RECT }, 200.0, 100.0), None);
        assert_eq!(drop_target(Rect { height: 0.0, ..RECT }, 200.0, 50.0), None);
    }

    #[test]
    fn test_wd03_dropping_on_the_center_of_the_pane_that_already_holds_the_tab_changes_nothing() {
        assert_eq!(dock_ref(DropMode::TabAfter, &[1, 2, 3], 2, 1), None);
        assert_eq!(dock_ref(DropMode::TabAfter, &[1, 2], 1, 1), None);
        assert_eq!(dock_ref(DropMode::TabAfter, &[3], 3, 1), Some(3));
    }

    #[test]
    fn test_wd03b_dropping_on_the_center_of_another_pane_appends_at_its_end_not_after_its_current_tab() {
        assert_eq!(dock_ref(DropMode::TabAfter, &[2, 3, 4], 2, 1), Some(4));
    }

    #[test]
    fn test_wd04_an_edge_split_from_the_tabs_own_pane_keeps_another_tab_as_the_reference() {
        assert_eq!(dock_ref(DropMode::SplitRight, &[1, 2], 1, 1), Some(2));
        assert_eq!(dock_ref(DropMode::SplitBottom, &[1, 2], 2, 1), Some(2));
        assert_eq!(dock_ref(DropMode::SplitLeft, &[1], 1, 1), None);
        assert_eq!(dock_ref(DropMode::SplitTop, &[3], 3, 1), Some(3));
    }

    #[test]
    fn test_wd05_dropping_on_a_tab_row_inserts_before_the_left_half_and_after_the_right_half_of_the_pointed_tab() {
        let tabs = [RowTab { id: 1, left: 0.0, right: 100.0 }, RowTab { id: 2, left: 100.0, right: 200.0 }];
        let insert = |mode, reference, edge| Some(Insert { mode, reference, edge });
        assert_eq!(row_insert(&tabs, 30.0), insert(DropMode::TabBefore, 1, 0.0));
        assert_eq!(row_insert(&tabs, 70.0), insert(DropMode::TabAfter, 1, 100.0));
        assert_eq!(row_insert(&tabs, 160.0), insert(DropMode::TabAfter, 2, 200.0));
        // 줄 끝의 빈 곳은 마지막 탭 뒤다.
        assert_eq!(row_insert(&tabs, 260.0), insert(DropMode::TabAfter, 2, 200.0));
        assert_eq!(row_insert::<u32>(&[], 50.0), None);
    }

    #[test]
    fn test_wd06_inserting_where_the_tab_already_sits_in_its_own_pane_changes_nothing_moving_to_another_pane_always_does()
     {
        assert!(is_same_place(&[1, 2, 3], 1, DropMode::TabBefore, 2));
        assert!(is_same_place(&[1, 2, 3], 1, DropMode::TabAfter, 1));
        assert!(is_same_place(&[1, 2, 3], 3, DropMode::TabAfter, 2));
        assert!(!is_same_place(&[1, 2, 3], 1, DropMode::TabAfter, 2));
        assert!(!is_same_place(&[1, 2, 3], 3, DropMode::TabBefore, 1));
        assert!(!is_same_place(&[2, 3], 1, DropMode::TabBefore, 2));
    }

    #[test]
    fn test_wd07_moving_a_tab_within_its_own_pane_lands_exactly_at_the_marked_place_in_both_directions() {
        let moved = |ids: &[u32], id: u32, mode: DropMode, reference: u32| {
            let mut order: Vec<u32> = ids.iter().copied().filter(|other| *other != id).collect();
            order.insert(move_index(ids, id, mode, reference).unwrap(), id);
            order
        };
        assert_eq!(moved(&[1, 2, 3], 1, DropMode::TabBefore, 3), [2, 1, 3]);
        assert_eq!(moved(&[1, 2, 3], 1, DropMode::TabAfter, 2), [2, 1, 3]);
        assert_eq!(moved(&[1, 2, 3], 1, DropMode::TabAfter, 3), [2, 3, 1]);
        assert_eq!(moved(&[1, 2, 3], 3, DropMode::TabBefore, 1), [3, 1, 2]);
        assert_eq!(moved(&[1, 2, 3, 4], 4, DropMode::TabAfter, 1), [1, 4, 2, 3]);
        assert_eq!(move_index(&[2, 3], 1, DropMode::TabBefore, 2), None);
    }
}
