//! 본문 칸의 화면 위치로 위쪽 탭 줄의 열을 정하는 순수 계산.
//!
//! 칸을 나누면 탭은 그 화면이 든 칸 바로 위에 놓여야 하므로, 왼쪽 가장자리가 같은 칸들을 한 열로 묶고 열을 왼쪽부터 늘어놓는다.
//! 화면 크기 측정과 그리기는 호출자가 맡고, 이 모듈은 숫자와 키만 다룬다.

use crate::layout_model::Layout;

/// 칸 하나의 화면 위치. `key`는 호출자가 칸을 가리키는 값이다.
#[derive(Clone, Debug, PartialEq)]
pub struct PanePlace<K> {
    pub key: K,
    pub left: f32,
    pub top: f32,
}

/// 왼쪽 가장자리가 같은 칸들의 열. 열 하나가 위쪽 탭 줄의 묶음 하나가 된다.
#[derive(Clone, Debug, PartialEq)]
pub struct Column<K> {
    pub left: f32,
    /// 위에서 아래 순서의 칸.
    pub keys: Vec<K>,
}

/// 탭 줄 안에서 열이 차지하는 가로 구간.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub left: f32,
    pub width: f32,
}

/// 칸들을 왼쪽 가장자리가 같은 열로 묶는다.
///
/// 옆으로 나눈 칸은 열이 다르고, 위아래로 나눈 칸은 같은 열에 위에서 아래 순으로 든다. `tolerance`(px) 안의 왼쪽 차이는
/// 같은 열로 본다(경계선 두께·소수점 오차).
///
/// Examples
/// --------
/// | 칸 (key, left, top)                 | 열                                  |
/// |-------------------------------------|-------------------------------------|
/// | (a, 0, 0)                           | [{0, [a]}]                          |
/// | (a, 0, 0), (b, 500, 0)              | [{0, [a]}, {500, [b]}]              |
/// | (a, 0, 0), (b, 500, 0), (c, 500, 300) | [{0, [a]}, {500, [b, c]}]         |
/// | (없음)                              | []                                  |
pub fn pane_columns<K: Clone>(
    panes: &[PanePlace<K>],
    tolerance: f32,
) -> Vec<Column<K>> {
    let mut ordered: Vec<&PanePlace<K>> = panes.iter().collect();
    ordered.sort_by(|first, second| first.left.total_cmp(&second.left).then_with(|| first.top.total_cmp(&second.top)));
    let mut columns: Vec<Column<K>> = Vec::new();
    for pane in ordered {
        match columns.iter_mut().find(|column| (column.left - pane.left).abs() <= tolerance) {
            Some(column) => column.keys.push(pane.key.clone()),
            None => columns.push(Column { left: pane.left, keys: vec![pane.key.clone()] }),
        }
    }
    columns
}

/// 파일 칸이 있을 때 본문 폭을 터미널 배치와 파일 칸으로 나눈다: (터미널 배치 폭, 파일 칸 폭).
///
/// 파일 칸은 `ratio`만큼 차지하되 어느 쪽도 `min_pane`보다 좁아지지 않는다. 본문이 두 최소 폭의 합보다 좁으면 반반이다.
///
/// Examples
/// --------
/// | 본문 폭 | ratio | 최소 폭 | 결과           |
/// |---------|-------|---------|----------------|
/// | 1000    | 0.4   | 120     | (600, 400)     |
/// | 1000    | 0.05  | 120     | (880, 120)     |
/// | 200     | 0.5   | 120     | (100, 100)     |
pub fn split_body_width(
    width: f32,
    ratio: f32,
    min_pane: f32,
) -> (f32, f32) {
    let file_width =
        if width < 2.0 * min_pane { width / 2.0 } else { (width * ratio).clamp(min_pane, width - min_pane) };
    (width - file_width, file_width)
}

/// 터미널 배치의 열 뒤에 파일 칸의 열(왼쪽 가장자리 `file_left`)을 덧붙인다. 탭 줄에서 파일 탭이 마지막 열이 되게 한다.
///
/// Examples
/// --------
/// | 열 (왼쪽)  | file_left | 결과                |
/// |------------|-----------|---------------------|
/// | 0, 300     | 600       | 0, 300, 600(파일)   |
/// | (없음)     | 600       | 600(파일)           |
pub fn columns_with_file<K: Clone>(
    columns: &[Column<K>],
    file_key: K,
    file_left: f32,
) -> Vec<Column<K>> {
    let mut all = columns.to_vec();
    all.push(Column { left: file_left, keys: vec![file_key] });
    all
}

/// 탭을 열 순서로 나눈다. 열 안에서는 칸 순서(위→아래), 같은 칸 안에서는 `ids`의 원래 순서를 지킨다.
///
/// `pane_of`는 그 탭의 화면이 든 칸의 키, 칸이 없으면(다른 창에 분리된 탭 등) None이다. 칸이 없는 탭은 마지막 열 뒤에 붙인다.
/// 열이 없으면 모든 탭이 한 묶음이다.
///
/// Examples
/// --------
/// | ids         | columns   | pane_of                | 결과               |
/// |-------------|-----------|------------------------|--------------------|
/// | t1, t2, f   | [A], [B]  | t1→A, t2→B, f→B        | [t1], [t2, f]      |
/// | t1, t2      | [A], [B, C] | t1→C, t2→B           | [], [t2, t1]       |
/// | t1, w       | [A]       | t1→A, w→없음           | [t1, w]            |
/// | t1          | (없음)    | t1→없음                | [t1]               |
pub fn group_tabs_by_column<I: Clone + PartialEq, K: PartialEq>(
    ids: &[I],
    pane_of: impl Fn(&I) -> Option<K>,
    columns: &[Column<K>],
) -> Vec<Vec<I>> {
    let mut groups: Vec<Vec<I>> = if columns.is_empty() {
        vec![ids.to_vec()]
    } else {
        columns
            .iter()
            .map(|column| {
                column
                    .keys
                    .iter()
                    .flat_map(|key| ids.iter().filter(|id| pane_of(id).as_ref() == Some(key)).cloned())
                    .collect()
            })
            .collect()
    };
    if !columns.is_empty() {
        let placed: Vec<I> = groups.iter().flatten().cloned().collect();
        let unplaced: Vec<I> = ids.iter().filter(|id| !placed.contains(id)).cloned().collect();
        if let Some(last) = groups.last_mut() {
            last.extend(unplaced);
        }
    }
    groups
}

/// 열마다 위쪽 탭 줄 안에서 차지할 가로 구간을 정한다. 열은 다음 열의 왼쪽까지, 마지막 열은 줄 끝까지 뻗는다.
///
/// `strip_left`·`strip_width`는 탭 줄의 화면 위치와 폭이다. 줄 왼쪽보다 앞선 열(사이드바 단추에 가린 첫 열)은 0에서 시작하고,
/// 줄 오른쪽 밖에서 시작하는 열(오른쪽 단추들에 가린 열)은 줄 끝에서 폭 0으로 남는다.
///
/// Examples
/// --------
/// | 열 왼쪽    | strip_left | strip_width | 결과                         |
/// |------------|------------|-------------|------------------------------|
/// | 0, 500     | 30         | 900         | {0, 470}, {470, 430}         |
/// | 0          | 30         | 900         | {0, 900}                     |
pub fn column_spans<K>(
    columns: &[Column<K>],
    strip_left: f32,
    strip_width: f32,
) -> Vec<Span> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let left = (column.left - strip_left).max(0.0).min(strip_width);
            let right = match columns.get(index + 1) {
                Some(next) => (next.left - strip_left).max(left).min(strip_width),
                None => strip_width,
            };
            Span { left, width: right - left }
        })
        .collect()
}

/// 탭을 본문 칸에 실제로 놓인 차례로 늘어놓는다. 앞 칸의 탭이 먼저, 같은 칸 안에서는 칸의 탭 순서를 따른다.
///
/// 위쪽 탭 줄은 칸 안 순서를 그대로 보여야 한다. `place`는 `(칸 차례, 칸 안 번호)`이고, 칸이 없는 탭(다른 창에 분리된 탭)은
/// None이다. 칸이 없는 탭은 맨 뒤에 원래 순서대로 남는다.
///
/// Examples
/// --------
/// | ids       | place                              | 결과       |
/// |-----------|------------------------------------|------------|
/// | a, b, c   | a→(0,1), b→(0,0), c→(0,2)          | b, a, c    |
/// | a, b, c   | a→(1,0), b→(0,1), c→(0,0)          | c, b, a    |
/// | w, a, b   | w→없음, a→(0,1), b→(0,0)           | b, a, w    |
pub fn order_tabs_by_pane<I: Clone>(
    ids: &[I],
    place: impl Fn(&I) -> Option<(usize, usize)>,
) -> Vec<I> {
    let mut ranked: Vec<(usize, (usize, usize), I)> =
        ids.iter().enumerate().map(|(order, id)| (order, place(id).unwrap_or((usize::MAX, 0)), id.clone())).collect();
    ranked.sort_by_key(|(order, rank, _)| (rank.0, rank.1, *order));
    ranked.into_iter().map(|(_, _, id)| id).collect()
}

/// 위쪽 탭 줄의 열 하나: 같은 열에 든 칸들과 그 칸들의 탭.
#[derive(Clone, Debug, PartialEq)]
pub struct TabColumn {
    /// 열의 왼쪽 가장자리(본문 왼쪽 기준).
    pub left: f32,
    /// 위에서 아래 순서의 칸 번호(`Layout::pane_boxes`의 `index`).
    pub panes: Vec<usize>,
    /// 열의 탭 전부: 위 칸부터, 같은 칸 안에서는 칸의 탭 순서다.
    pub tabs: Vec<u32>,
}

/// 배치를 `width`×`height` 본문에 놓았을 때의 탭 줄 열들을 왼쪽부터 돌려준다.
///
/// 열의 맨 위 칸(`panes[0]`)의 탭은 위쪽 탭 줄에 그 칸 바로 위에 놓이고, 나머지 칸은 자기 위에 탭 줄을 따로 가진다.
/// 이어 붙인 `tabs`는 탭 이동(앞·뒤) 순서이기도 하다.
///
/// Examples
/// --------
/// | 배치                                   | 열                                                  |
/// |----------------------------------------|-----------------------------------------------------|
/// | 칸 [1, 2]                              | [{0, 칸 0, 탭 1·2}]                                 |
/// | 가로 [칸 [2, 1], 세로 [칸 [3], 칸 [5, 4]]] | [{0, 칸 0, 탭 2·1}, {500, 칸 1·2, 탭 3·5·4}]       |
pub fn tab_columns(
    layout: &Layout,
    width: f32,
    height: f32,
) -> Vec<TabColumn> {
    let boxes = layout.pane_boxes(width, height);
    let places: Vec<PanePlace<usize>> =
        boxes.iter().map(|pane| PanePlace { key: pane.index, left: pane.left, top: pane.top }).collect();
    let columns = pane_columns(&places, 1.0);
    let place_of = |id: &u32| {
        boxes.iter().find_map(|pane| pane.tabs.iter().position(|tab| tab == id).map(|index| (pane.index, index)))
    };
    let ids = order_tabs_by_pane(&layout.terminal_ids(), place_of);
    let groups = group_tabs_by_column(&ids, |id| place_of(id).map(|(pane, _)| pane), &columns);
    columns
        .into_iter()
        .zip(groups)
        .map(|(column, tabs)| TabColumn { left: column.left, panes: column.keys, tabs })
        .collect()
}

/// 탭 줄 넘김 단추를 한 번 눌렀을 때의 새 가로 스크롤 위치(0 이하). `step`만큼 움직이되 `[-max, 0]` 밖으로 나가지 않는다.
///
/// Examples
/// --------
/// | 현재   | 최대 | 방향   | 결과   |
/// |--------|------|--------|--------|
/// | 0      | 200  | 오른쪽 | -150   |
/// | -150   | 200  | 오른쪽 | -200   |
/// | -50    | 200  | 왼쪽   | 0      |
pub fn scroll_step_offset(
    current: f32,
    max: f32,
    step: f32,
    toward_right: bool,
) -> f32 {
    let moved = if toward_right { current - step } else { current + step };
    moved.clamp(-max.max(0.0), 0.0)
}

/// 탭 줄이 칸 위치를 계산할 본문 크기를 고른다: 마지막으로 잰 크기가 지금 창 크기에서 잰 것이면 그 크기, 아니면 어림값이다.
///
/// 본문 크기는 그린 뒤에야 알 수 있어 다음 그리기에서 쓴다. 창 크기가 바뀐 직후의 첫 그리기에서 지난 크기를 그대로 쓰면
/// 탭 열 구간이 한 프레임 낡은 폭으로 놓이므로, 잴 때의 창 크기(`measured_for`)가 지금(`viewport`)과 다르면 어림값(`estimate`)을 쓴다.
/// 아직 재지 못했거나(0) 어림값 계산에 쓴 값이 같은 처음 그리기도 어림값이다.
///
/// Examples
/// --------
/// | 잰 크기   | 잴 때 창 크기 | 지금 창 크기 | 결과   |
/// |-----------|---------------|--------------|--------|
/// | 700×500   | 1000×600      | 1000×600     | 잰 크기 |
/// | 700×500   | 1000×600      | 1200×600     | 어림값 |
/// | 0×0       | (없음)        | 1000×600     | 어림값 |
pub fn strip_body_size(
    measured: (f32, f32),
    measured_for: (f32, f32),
    viewport: (f32, f32),
    estimate: (f32, f32),
) -> (f32, f32) {
    let is_current = measured_for == viewport && measured.0 > 0.0 && measured.1 > 0.0;
    if is_current { measured } else { estimate }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_model::Orientation;

    fn at(
        key: &'static str,
        left: f32,
        top: f32,
    ) -> PanePlace<&'static str> {
        PanePlace { key, left, top }
    }

    fn column(
        left: f32,
        keys: &[&'static str],
    ) -> Column<&'static str> {
        Column { left, keys: keys.to_vec() }
    }

    #[test]
    fn test_pc01_side_by_side_panes_form_separate_columns_ordered_from_the_left() {
        let columns = pane_columns(&[at("b", 500.0, 0.0), at("a", 0.0, 0.0)], 1.0);
        assert_eq!(columns, vec![column(0.0, &["a"]), column(500.0, &["b"])]);
    }

    #[test]
    fn test_pc02_stacked_panes_share_one_column_in_top_order_within_the_tolerance() {
        let columns = pane_columns(&[at("bottom", 500.6, 300.0), at("a", 0.0, 0.0), at("top", 500.0, 0.0)], 1.0);
        assert_eq!(columns, vec![column(0.0, &["a"]), column(500.0, &["top", "bottom"])]);
        assert_eq!(pane_columns::<&str>(&[], 1.0), Vec::new());
    }

    #[test]
    fn test_pc03_a_full_width_bottom_pane_joins_the_column_above_its_left_edge() {
        let columns = pane_columns(&[at("t1", 0.0, 0.0), at("t2", 400.0, 0.0), at("b", 0.0, 300.0)], 1.0);
        assert_eq!(columns, vec![column(0.0, &["t1", "b"]), column(400.0, &["t2"])]);
    }

    #[test]
    fn test_pc04_tabs_follow_their_pane_column_keep_the_strip_order_inside_a_pane_and_stacked_panes_go_top_first() {
        let pane_of = |id: &&str| match *id {
            "t1" => Some("C"),
            "t2" | "f" => Some("B"),
            _ => None,
        };
        let columns = [column(0.0, &["A"]), column(500.0, &["B", "C"])];
        let groups = group_tabs_by_column(&["t1", "t2", "f"], pane_of, &columns);
        assert_eq!(groups, vec![Vec::<&str>::new(), vec!["t2", "f", "t1"]]);
    }

    #[test]
    fn test_pc05_tabs_without_a_pane_trail_the_last_column_and_no_columns_means_one_group() {
        let pane_of = |id: &&str| (*id == "t1").then_some("A");
        assert_eq!(group_tabs_by_column(&["w", "t1"], pane_of, &[column(0.0, &["A"])]), vec![vec!["t1", "w"]]);
        assert_eq!(group_tabs_by_column(&["t1", "w"], pane_of, &[]), vec![vec!["t1", "w"]]);
    }

    #[test]
    fn test_pc06_each_column_spans_to_the_next_and_the_last_reaches_the_strip_end_and_clipped_lefts_start_at_0() {
        let columns = [column(0.0, &["a"]), column(500.0, &["b"])];
        assert_eq!(
            column_spans(&columns, 30.0, 900.0),
            vec![Span { left: 0.0, width: 470.0 }, Span { left: 470.0, width: 430.0 }]
        );
        assert_eq!(column_spans(&[column(0.0, &["a"])], 30.0, 900.0), vec![Span { left: 0.0, width: 900.0 }]);
        // 줄보다 오른쪽에서 시작하는 열(오른쪽 단추에 가린 열)은 폭 0으로 남고 음수가 되지 않는다.
        assert_eq!(
            column_spans(&[column(0.0, &["a"]), column(1000.0, &["b"])], 30.0, 900.0),
            vec![Span { left: 0.0, width: 900.0 }, Span { left: 900.0, width: 0.0 }]
        );
    }

    #[test]
    fn test_pc07_tabs_follow_the_order_they_sit_in_their_panes_with_paneless_tabs_last() {
        let place = |id: &&str| match *id {
            "a" => Some((0, 1)),
            "b" => Some((0, 0)),
            "c" => Some((0, 2)),
            "d" => Some((1, 0)),
            _ => None,
        };
        assert_eq!(order_tabs_by_pane(&["a", "b", "c"], place), vec!["b", "a", "c"]);
        assert_eq!(order_tabs_by_pane(&["d", "a", "b"], place), vec!["b", "a", "d"]);
        assert_eq!(order_tabs_by_pane(&["w", "a", "b"], place), vec!["b", "a", "w"]);
    }

    #[test]
    fn test_pc08_a_file_pane_adds_the_last_column_and_the_terminal_columns_end_where_it_starts() {
        let terminal_columns = [column(0.0, &["a"]), column(300.0, &["b"])];
        let (terminal_width, file_width) = split_body_width(1000.0, 0.4, 120.0);
        assert_eq!((terminal_width, file_width), (600.0, 400.0));
        let columns = columns_with_file(&terminal_columns, "file", terminal_width);
        assert_eq!(columns, vec![column(0.0, &["a"]), column(300.0, &["b"]), column(600.0, &["file"])]);
        assert_eq!(
            column_spans(&columns, 30.0, 900.0),
            vec![
                Span { left: 0.0, width: 270.0 },
                Span { left: 270.0, width: 300.0 },
                Span { left: 570.0, width: 330.0 },
            ]
        );
    }

    #[test]
    fn test_pc09_the_file_pane_never_shrinks_either_side_below_the_minimum() {
        assert_eq!(split_body_width(1000.0, 0.05, 120.0), (880.0, 120.0));
        assert_eq!(split_body_width(1000.0, 0.95, 120.0), (120.0, 880.0));
        // 두 칸의 최소 폭을 합친 것보다 좁으면 반반이다.
        assert_eq!(split_body_width(200.0, 0.5, 120.0), (100.0, 100.0));
    }

    fn pane(
        tabs: &[u32],
        selected: usize,
    ) -> Layout {
        Layout::pane(tabs.to_vec(), selected)
    }

    fn two(
        orientation: Orientation,
        first: Layout,
        second: Layout,
    ) -> Layout {
        Layout::Split { orientation, children: vec![first, second], sizes: vec![1.0, 1.0] }
    }

    #[test]
    fn test_tab_columns_put_each_panes_tabs_over_its_column_with_stacked_panes_top_first() {
        let stacked = two(Orientation::Vertical, pane(&[3], 0), pane(&[5, 4], 0));
        let layout = two(Orientation::Horizontal, pane(&[2, 1], 0), stacked);
        let columns = tab_columns(&layout, 1000.0, 600.0);
        assert_eq!(
            columns,
            vec![
                TabColumn { left: 0.0, panes: vec![0], tabs: vec![2, 1] },
                TabColumn { left: 500.0, panes: vec![1, 2], tabs: vec![3, 5, 4] },
            ]
        );
    }

    #[test]
    fn test_tab_columns_of_a_single_pane_or_a_vertical_stack_is_one_column() {
        assert_eq!(
            tab_columns(&pane(&[1, 2], 1), 800.0, 600.0),
            vec![TabColumn { left: 0.0, panes: vec![0], tabs: vec![1, 2] }]
        );
        let stack = two(Orientation::Vertical, pane(&[1], 0), pane(&[2], 0));
        assert_eq!(
            tab_columns(&stack, 800.0, 600.0),
            vec![TabColumn { left: 0.0, panes: vec![0, 1], tabs: vec![1, 2] }]
        );
    }

    #[test]
    fn test_tab_columns_order_a_bottom_pane_under_the_pane_above_its_left_edge() {
        // 위 줄이 둘로 나뉘고 아래는 폭 전체인 배치: 아래 칸은 왼쪽 열에 들어 열 순서는 1, 3 다음 2다.
        let top = two(Orientation::Horizontal, pane(&[1], 0), pane(&[2], 0));
        let layout = two(Orientation::Vertical, top, pane(&[3], 0));
        let columns = tab_columns(&layout, 1000.0, 600.0);
        assert_eq!(columns.iter().map(|column| column.tabs.clone()).collect::<Vec<_>>(), vec![vec![1, 3], vec![2]]);
    }

    #[test]
    fn test_scroll_step_offset_moves_one_step_and_stays_within_the_scrollable_range() {
        // 오른쪽으로 한 걸음: 0 → -150, 끝(최대 200)을 넘지 않는다.
        assert_eq!(scroll_step_offset(0.0, 200.0, 150.0, true), -150.0);
        assert_eq!(scroll_step_offset(-150.0, 200.0, 150.0, true), -200.0);
        // 왼쪽으로 한 걸음: 처음(0)을 넘지 않는다.
        assert_eq!(scroll_step_offset(-200.0, 200.0, 150.0, false), -50.0);
        assert_eq!(scroll_step_offset(-50.0, 200.0, 150.0, false), 0.0);
        // 넘치지 않는 상자(최대 0)는 움직이지 않는다.
        assert_eq!(scroll_step_offset(0.0, 0.0, 150.0, true), 0.0);
    }

    #[test]
    fn test_strip_body_size_uses_the_measured_size_only_for_the_window_size_it_was_measured_in() {
        let estimate = (800.0, 560.0);
        assert_eq!(strip_body_size((700.0, 500.0), (1000.0, 600.0), (1000.0, 600.0), estimate), (700.0, 500.0));
        // 창 크기가 바뀐 직후의 첫 그리기: 지난 크기는 낡았다.
        assert_eq!(strip_body_size((700.0, 500.0), (1000.0, 600.0), (1200.0, 600.0), estimate), estimate);
        assert_eq!(strip_body_size((700.0, 500.0), (1000.0, 600.0), (1000.0, 700.0), estimate), estimate);
        // 아직 재지 못했다.
        assert_eq!(strip_body_size((0.0, 0.0), (0.0, 0.0), (1000.0, 600.0), estimate), estimate);
        assert_eq!(strip_body_size((0.0, 500.0), (1000.0, 600.0), (1000.0, 600.0), estimate), estimate);
    }
}
