//! 탭 끌기의 창 없는 규칙: 끌기 상태, 놓을 자리 찾기(칸 본문·칸 위 탭 줄·탭), 다른 창에서 끌어 온 점을 본 창 좌표로 바꿔 놓을 자리 찾기,
//! 놓을 때 할 일.
//!
//! 화면 좌표와 탭 번호만 다룬다. 끌기 이벤트·그리기·창 열기는 `tab_drag`와 `detached_window`가 맡는다. 놓을 자리의 규칙(가장자리 10 %,
//! 가운데, 끼우기, 바뀌는 것이 없는 자리)은 core의 `tab_drop`이 정하고, 이 파일은 그 규칙에 이 창의 칸 배치와 좌표를 먹인다.
use std::collections::HashSet;

use paddock_core::layout_model::Layout;
use paddock_core::pane_columns::tab_columns;
use paddock_core::tab_drop::{DropMode, Rect, RowTab, dock_ref, drop_target, is_same_place, row_insert};
use paddock_core::theme::{detached_window as detached_size, tab_drag as drag_size};
use paddock_core::window_point::{ScreenBounds, is_outside, other_window_point};

use crate::workspace::{DetachedTab, DockPlace};

/// 끌고 있는 탭이 가리키는 터미널 번호.
pub fn terminal_of(tab: DetachedTab) -> u32 {
    match tab {
        DetachedTab::Terminal(id) => id,
    }
}

/// 마우스를 누른 창이 쥐는 끌기 상태.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabDrag {
    pub tab: DetachedTab,
    /// 마지막으로 본 마우스 자리(이 창 안 좌표).
    pub position: (f32, f32),
    /// `Esc`로 취소했다. 이 뒤의 놓기는 아무 일도 하지 않는다.
    pub cancelled: bool,
    /// 자동 점검이 만든 끌기다. 실제 gpui 끌기가 없어도 그리기가 상태를 지우지 않는다.
    pub synthetic: bool,
}

impl TabDrag {
    pub fn new(
        tab: DetachedTab,
        position: (f32, f32),
    ) -> Self {
        Self { tab, position, cancelled: false, synthetic: false }
    }

    /// `Esc`: 이 끌기의 놓기를 무시하게 한다.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

/// 놓을 자리 미리보기: 놓는 자리와 그릴 상자(창 안 좌표). `line`이면 탭 줄에 끼울 자리의 세로선이고 아니면 칸 위의 상자다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockPreview {
    pub place: DockPlace,
    pub rect: Rect,
    pub line: bool,
}

impl DockPreview {
    /// 자동 점검 상태 글에 쓰는 `<방식>:<기준 탭>`.
    pub fn summary(&self) -> String {
        format!("{}:{}", mode_name(self.place.mode), self.place.reference)
    }
}

/// 놓는 방식의 이름(상태 글과 자동 점검 명령이 쓴다).
pub fn mode_name(mode: DropMode) -> &'static str {
    match mode {
        DropMode::SplitLeft => "left",
        DropMode::SplitRight => "right",
        DropMode::SplitTop => "top",
        DropMode::SplitBottom => "bottom",
        DropMode::TabAfter => "after",
        DropMode::TabBefore => "before",
    }
}

/// 이름으로 놓는 방식을 고른다. `center`는 칸 가운데(그 칸의 끝에 붙이기)다.
pub fn mode_named(name: &str) -> Option<DropMode> {
    match name {
        "left" => Some(DropMode::SplitLeft),
        "right" => Some(DropMode::SplitRight),
        "top" => Some(DropMode::SplitTop),
        "bottom" => Some(DropMode::SplitBottom),
        "center" | "after" => Some(DropMode::TabAfter),
        "before" => Some(DropMode::TabBefore),
        _ => None,
    }
}

/// 칸 하나가 창 안에서 차지한 자리: 본문과 그 칸의 탭 줄.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneSlot {
    /// 칸의 탭(칸 안 순서).
    pub tabs: Vec<u32>,
    /// 칸이 지금 보여 주는 탭.
    pub current: u32,
    /// 탭 줄 열 번호(왼쪽부터).
    pub column: usize,
    /// 열의 맨 위 칸이다(탭이 위쪽 탭 줄에 놓이는 칸).
    pub is_top: bool,
    /// 터미널이 그려지는 본문.
    pub body: Rect,
    /// 이 칸의 탭 줄: 맨 위 칸은 본문 바로 위의 줄 띠, 아래 칸은 본문 위의 자기 탭 줄이다.
    pub strip: Rect,
}

/// 칸 배치 `layout`을 `size`(본문 폭·높이)로 놓고 본문 왼쪽 위가 창 안 `origin`일 때 칸마다의 자리.
/// `bar_height`는 탭 줄 높이다. 터미널이 없는 칸은 건너뛴다.
pub fn pane_slots(
    layout: &Layout,
    size: (f32, f32),
    origin: (f32, f32),
    bar_height: f32,
) -> Vec<PaneSlot> {
    let columns = tab_columns(layout, size.0, size.1);
    let lower: HashSet<usize> = columns.iter().flat_map(|column| column.panes.iter().copied().skip(1)).collect();
    let column_of = |pane: usize| columns.iter().position(|column| column.panes.contains(&pane)).unwrap_or(0);
    layout
        .pane_boxes(size.0, size.1)
        .into_iter()
        .filter_map(|pane| {
            let current = pane.tabs.get(pane.selected.min(pane.tabs.len().saturating_sub(1))).copied()?;
            let (left, top) = (origin.0 + pane.left, origin.1 + pane.top);
            let is_top = !lower.contains(&pane.index);
            let (strip, body) = if is_top {
                (
                    Rect { left, top: top - bar_height, width: pane.width, height: bar_height },
                    Rect { left, top, width: pane.width, height: pane.height },
                )
            } else {
                (
                    Rect { left, top, width: pane.width, height: bar_height },
                    Rect {
                        left,
                        top: top + bar_height,
                        width: pane.width,
                        height: (pane.height - bar_height).max(0.0),
                    },
                )
            };
            Some(PaneSlot { tabs: pane.tabs, current, column: column_of(pane.index), is_top, body, strip })
        })
        .collect()
}

fn contains(
    rect: Rect,
    x: f32,
    y: f32,
) -> bool {
    x >= rect.left && y >= rect.top && x < rect.left + rect.width && y < rect.top + rect.height
}

impl PaneSlot {
    /// 끈 탭 `dragged`를 이 칸의 `mode` 자리에 놓을 때의 자리. 바뀌는 것이 없으면 None이다(같은 칸 가운데, 하나뿐인 탭의 가장자리).
    pub fn place_for(
        &self,
        dragged: u32,
        mode: DropMode,
    ) -> Option<DockPlace> {
        dock_ref(mode, &self.tabs, self.current, dragged).map(|reference| DockPlace { reference, mode })
    }

    /// 이 칸의 탭 줄 빈 자리에 놓을 때의 자리: 이 칸의 끝. 끈 탭이 이미 칸 끝이면 None이다.
    pub fn strip_place_for(
        &self,
        dragged: u32,
    ) -> Option<DockPlace> {
        let last = self.tabs.last().copied()?;
        (!is_same_place(&self.tabs, dragged, DropMode::TabAfter, last))
            .then_some(DockPlace { reference: last, mode: DropMode::TabAfter })
    }
}

/// 창 안의 한 점 `(x, y)`가 가리키는 놓을 자리(칸 본문의 가장자리·가운데, 칸 탭 줄의 빈 자리). 자리가 없거나 바뀌는 것이 없으면 None이다.
pub fn preview_at(
    slots: &[PaneSlot],
    dragged: u32,
    x: f32,
    y: f32,
) -> Option<DockPreview> {
    slots
        .iter()
        .find_map(|slot| {
            if let Some(target) = drop_target(slot.body, x, y) {
                // 이 칸에서 정해졌다: 놓아도 바뀌는 것이 없으면 다른 칸을 더 찾지 않고 미리보기도 없다.
                Some(slot.place_for(dragged, target.mode).map(|place| DockPreview {
                    place,
                    rect: target.rect,
                    line: false,
                }))
            } else if contains(slot.strip, x, y) {
                Some(slot.strip_place_for(dragged).map(|place| DockPreview { place, rect: slot.body, line: false }))
            } else {
                None
            }
        })
        .flatten()
}

/// 탭 줄의 탭 하나 위에 놓을 때의 자리: 탭의 왼쪽 절반이면 그 탭 앞, 오른쪽 절반이면 뒤. 그 자리가 이미 끈 탭의 자리면 None이다.
/// `pane_tabs`는 그 탭이 든 칸의 탭, `top`·`height`는 탭의 세로 구간(끼움 선의 길이)이다.
pub fn row_preview(
    pane_tabs: &[u32],
    dragged: u32,
    tab: RowTab<u32>,
    top: f32,
    height: f32,
    x: f32,
) -> Option<DockPreview> {
    let insert = row_insert(&[tab], x)?;
    let moves = insert.reference != dragged && !is_same_place(pane_tabs, dragged, insert.mode, insert.reference);
    moves.then(|| DockPreview {
        place: DockPlace { reference: insert.reference, mode: insert.mode },
        rect: Rect {
            left: insert.edge - paddock_core::theme::dock_preview::INSERT_LINE_WIDTH / 2.0,
            top,
            width: paddock_core::theme::dock_preview::INSERT_LINE_WIDTH,
            height,
        },
        line: true,
    })
}

/// 다른 창에서 끌던 점의 화면 좌표 `screen`이 본 창(`main`) 본문 안이면 그 점이 가리키는 놓을 자리. 본 창 밖이거나 놓을 자리가 없으면 None이다.
/// 탭 줄 위는 그 칸의 끝에 붙이는 자리다(탭 사이 정확한 끼우기는 같은 창 안의 끌기만 한다).
pub fn main_point_preview(
    screen: (f32, f32),
    main: ScreenBounds,
    slots: &[PaneSlot],
    dragged: u32,
) -> Option<DockPreview> {
    let (x, y) = other_window_point(screen, main)?;
    preview_at(slots, dragged, x, y)
}

/// 사각형 `rect` 안에서 `mode`의 놓을 자리를 가리키는 점: 가장자리 방식은 그 가장자리에서 3 % 안쪽 가운데, 그 밖은 사각형 가운데다.
/// 자동 점검이 마우스 대신 합성 좌표를 만드는 데 쓴다.
pub fn point_in(
    rect: Rect,
    mode: DropMode,
) -> (f32, f32) {
    let (center_x, center_y) = (rect.left + rect.width / 2.0, rect.top + rect.height / 2.0);
    match mode {
        DropMode::SplitLeft => (rect.left + rect.width * 0.03, center_y),
        DropMode::SplitRight => (rect.left + rect.width * 0.97, center_y),
        DropMode::SplitTop => (center_x, rect.top + rect.height * 0.03),
        DropMode::SplitBottom => (center_x, rect.top + rect.height * 0.97),
        DropMode::TabAfter | DropMode::TabBefore => (center_x, center_y),
    }
}

/// 새 분리 창을 열 화면 좌표: 놓은 지점에서 왼쪽·위로 (60, 16)만큼 옮긴 자리다.
pub fn detached_origin(drop_screen: (f32, f32)) -> (f32, f32) {
    (drop_screen.0 - detached_size::DROP_OFFSET_X, drop_screen.1 - detached_size::DROP_OFFSET_Y)
}

/// 끌기를 놓았을 때 할 일.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Release {
    /// 창 밖에 놓았다: 탭을 새 창으로 뗀다.
    OpenWindow,
    /// 놓을 자리에 놓았다.
    Dock(DockPlace),
    Nothing,
}

/// 본 창에서 끈 탭을 `position`(창 안 좌표)에서 놓았을 때 할 일. 취소했거나 끌기 상태가 없으면 아무것도 하지 않고,
/// 창 밖이면 새 창으로 떼고, 창 안이면 미리보기가 가리키던 자리에 놓는다(자리가 없으면 아무 일 없음).
pub fn release_action(
    drag: Option<&TabDrag>,
    position: (f32, f32),
    viewport: (f32, f32),
    preview: Option<&DockPreview>,
) -> Release {
    match drag {
        Some(drag) if !drag.cancelled => {
            if is_outside(position.0, position.1, viewport.0, viewport.1) {
                Release::OpenWindow
            } else {
                preview.map_or(Release::Nothing, |preview| Release::Dock(preview.place))
            }
        }
        _ => Release::Nothing,
    }
}

/// 본 창에서 끌기 시작한 파일 탭: 지금 보는 묶음 파일 칸의 몇 번째 탭인지와 마지막으로 본 마우스 자리(이 창 안 좌표).
///
/// 파일 탭은 터미널 배치의 칸이 아니라 묶음의 파일 칸에 속하므로(D51) 칸 가장자리·가운데·탭 줄에 놓는 자리가 없다.
/// 그래서 놓을 자리 미리보기도 없고, 창 밖에서 뗄 때만 새 창으로 뗀다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileDrag {
    pub index: usize,
    pub position: (f32, f32),
    /// `Esc`로 취소했다. 이 뒤의 놓기는 아무 일도 하지 않는다.
    pub cancelled: bool,
}

impl FileDrag {
    pub fn new(
        index: usize,
        position: (f32, f32),
    ) -> Self {
        Self { index, position, cancelled: false }
    }
}

/// 파일 탭 끌기를 놓았을 때 할 일. 칸에 놓는 쪽은 없다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRelease {
    /// 창 밖에 놓았다: 파일 탭을 새 창으로 뗀다.
    OpenWindow,
    Nothing,
}

/// 본 창에서 끈 파일 탭을 `position`(창 안 좌표)에서 놓았을 때 할 일. 창 밖에서 놓았을 때만 새 창으로 떼고, 창 안(칸·탭 줄 포함)이나
/// 취소했거나 끌기 상태가 없으면 아무 일도 없다.
pub fn file_release_action(
    drag: Option<&FileDrag>,
    position: (f32, f32),
    viewport: (f32, f32),
) -> FileRelease {
    match drag {
        Some(drag) if !drag.cancelled && is_outside(position.0, position.1, viewport.0, viewport.1) => {
            FileRelease::OpenWindow
        }
        _ => FileRelease::Nothing,
    }
}

/// 분리 창에서 시작한 끌기: 누른 자리와, 끌기로 인정할 만큼 움직였는지.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetachedDrag {
    start: (f32, f32),
    dragging: bool,
}

impl DetachedDrag {
    pub fn new(start: (f32, f32)) -> Self {
        Self { start, dragging: false }
    }

    /// 마우스가 `position`으로 움직였다. 처음 누른 자리에서 시작 거리(8px)를 넘으면 끌기가 시작된다. 끌고 있으면 true다.
    pub fn moved(
        &mut self,
        position: (f32, f32),
    ) -> bool {
        let distance = (position.0 - self.start.0).hypot(position.1 - self.start.1);
        self.dragging |= distance > drag_size::START_DISTANCE;
        self.is_active()
    }

    /// 끌기가 진행 중이다(시작 거리를 넘었다). 취소(`Esc`·창 비활성)는 창이 이 값을 버리는 것이다.
    pub fn is_active(&self) -> bool {
        self.dragging
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::layout_model::Orientation;

    use super::*;

    fn pane(
        tabs: &[u32],
        selected: usize,
    ) -> Layout {
        Layout::pane(tabs.to_vec(), selected)
    }

    /// 두 칸(왼쪽 [1, 2], 오른쪽 [3])을 폭 800 × 높이 500 본문에 놓고 본문 왼쪽 위가 창 안 (200, 60)인 자리들.
    fn two_slots() -> Vec<PaneSlot> {
        let layout = Layout::Split {
            orientation: Orientation::Horizontal,
            children: vec![pane(&[1, 2], 0), pane(&[3], 0)],
            sizes: vec![0.5, 0.5],
        };
        pane_slots(&layout, (800.0, 500.0), (200.0, 60.0), 30.0)
    }

    #[test]
    fn test_c_d83_an_outside_release_opens_a_window_and_an_inside_release_without_a_target_does_nothing() {
        let drag = TabDrag::new(DetachedTab::Terminal(2), (100.0, 100.0));
        let viewport = (1000.0, 700.0);
        for outside in [(-5.0, 100.0), (1000.0, 100.0), (100.0, -1.0), (100.0, 700.0)] {
            assert_eq!(release_action(Some(&drag), outside, viewport, None), Release::OpenWindow, "{outside:?}");
        }
        // 창 안에서 놓았는데 놓을 자리가 없으면 아무 일도 없다(사이드바·상태 줄 위 등).
        assert_eq!(release_action(Some(&drag), (100.0, 100.0), viewport, None), Release::Nothing);
        // 자리가 있으면 그 자리에 놓는다.
        let place = DockPlace { reference: 1, mode: DropMode::SplitRight };
        let preview = DockPreview { place, rect: Rect { left: 0.0, top: 0.0, width: 1.0, height: 1.0 }, line: false };
        assert_eq!(release_action(Some(&drag), (100.0, 100.0), viewport, Some(&preview)), Release::Dock(place));
        // 끌기 상태가 없으면(창 비활성·MouseDown에서 지웠다) 창 밖에서 놓아도 아무 일 없다.
        assert_eq!(release_action(None, (-5.0, 100.0), viewport, None), Release::Nothing);
    }

    #[test]
    fn test_c_d86_a_file_tab_refuses_pane_targets_and_accepts_outside() {
        let drag = FileDrag::new(1, (100.0, 100.0));
        let viewport = (1000.0, 700.0);
        // 창 밖에서 놓으면 새 창으로 뗀다.
        for outside in [(-5.0, 100.0), (1000.0, 100.0), (100.0, -1.0), (100.0, 700.0)] {
            assert_eq!(file_release_action(Some(&drag), outside, viewport), FileRelease::OpenWindow, "{outside:?}");
        }
        // 창 안에서는 칸 가운데·가장자리·탭 줄 어디에 놓아도(터미널이라면 놓을 자리가 있는 곳) 아무 일도 없다 — 놓을 자리 종류가 아예 없다.
        let slots = two_slots();
        for (x, y) in [(800.0, 300.0), (305.0, 350.0), (800.0, 45.0), (300.0, 45.0)] {
            assert!(preview_at(&slots, 9, x, y).is_some(), "a terminal could be docked at {x},{y}");
            assert_eq!(file_release_action(Some(&drag), (x, y), viewport), FileRelease::Nothing, "{x},{y}");
        }
        // 취소했거나 끌기 상태가 없으면 창 밖에서 놓아도 아무 일 없다.
        let cancelled = FileDrag { cancelled: true, ..drag };
        assert_eq!(file_release_action(Some(&cancelled), (-5.0, 100.0), viewport), FileRelease::Nothing);
        assert_eq!(file_release_action(None, (-5.0, 100.0), viewport), FileRelease::Nothing);
    }

    #[test]
    fn test_c_d83_an_inside_release_reported_as_outside_up_by_gpui_still_docks_at_the_last_preview() {
        // 키를 누른 뒤 마우스를 움직이지 않고 놓으면 gpui가 창 안 좌표로 `on_mouse_up_out`을 부른다. 같은 규칙이 마지막 미리보기에 놓는다.
        let drag = TabDrag::new(DetachedTab::Terminal(2), (400.0, 300.0));
        let place = DockPlace { reference: 3, mode: DropMode::SplitRight };
        let preview = DockPreview { place, rect: Rect { left: 0.0, top: 0.0, width: 1.0, height: 1.0 }, line: false };
        assert_eq!(release_action(Some(&drag), (400.0, 300.0), (1000.0, 700.0), Some(&preview)), Release::Dock(place));
        assert_eq!(release_action(Some(&drag), (400.0, 300.0), (1000.0, 700.0), None), Release::Nothing);
    }

    #[test]
    fn test_c_d83_escape_marks_the_drag_cancelled_so_the_release_is_ignored() {
        let mut drag = TabDrag::new(DetachedTab::Terminal(2), (100.0, 100.0));
        let viewport = (1000.0, 700.0);
        let place = DockPlace { reference: 1, mode: DropMode::TabAfter };
        let preview = DockPreview { place, rect: Rect { left: 0.0, top: 0.0, width: 1.0, height: 1.0 }, line: false };
        drag.cancel();
        assert!(drag.cancelled);
        assert_eq!(release_action(Some(&drag), (-5.0, 100.0), viewport, None), Release::Nothing, "outside");
        assert_eq!(release_action(Some(&drag), (100.0, 100.0), viewport, Some(&preview)), Release::Nothing, "inside");
        // 분리 창에서 시작한 끌기는 8px를 넘어야 끌기가 되고, 취소는 창이 상태를 버리는 것(`end_drag` 뒤 `None`)이라 더 움직여도 끌기가 아니다.
        let mut detached = Some(DetachedDrag::new((10.0, 10.0)));
        assert!(detached.as_mut().is_some_and(|drag| !drag.moved((14.0, 10.0))), "under 8 px is a click");
        assert!(detached.as_mut().is_some_and(|drag| drag.moved((30.0, 10.0))));
        detached = None;
        assert!(detached.as_mut().is_none_or(|drag| !drag.moved((50.0, 10.0))));
    }

    #[test]
    fn test_c_d83_a_screen_point_inside_the_main_window_maps_to_a_pane_target_and_outside_to_none() {
        let slots = two_slots();
        // 본 창은 화면 (100, 50)에서 1000×700이고 본문 왼쪽 위는 창 안 (200, 60)이다. 오른쪽 칸은 창 안 x 600~1000이다.
        let main = ScreenBounds { left: 100.0, top: 50.0, width: 1000.0, height: 700.0 };
        let at = |x: f32, y: f32| main_point_preview((x, y), main, &slots, 9);
        // 오른쪽 칸 가운데(창 안 (800, 300) = 화면 (900, 350)): 그 칸의 끝에 붙이는 자리, 칸 전체가 미리보기다.
        let center = at(900.0, 350.0).unwrap();
        assert_eq!(center.place, DockPlace { reference: 3, mode: DropMode::TabAfter });
        assert_eq!(center.rect, Rect { left: 600.0, top: 60.0, width: 400.0, height: 500.0 });
        // 왼쪽 칸 왼쪽 가장자리(창 안 (205, 300)): 왼쪽 칸이 고른 탭(1) 기준으로 왼쪽 분할, 미리보기는 칸의 왼쪽 절반이다.
        let edge = at(305.0, 350.0).unwrap();
        assert_eq!(edge.place, DockPlace { reference: 1, mode: DropMode::SplitLeft });
        assert_eq!(edge.rect, Rect { left: 200.0, top: 60.0, width: 200.0, height: 500.0 });
        // 본 창 밖(왼쪽·위·오른쪽·아래)이면 자리가 없다.
        for outside in [(50.0, 350.0), (900.0, 40.0), (1101.0, 350.0), (900.0, 751.0)] {
            assert_eq!(at(outside.0, outside.1), None, "{outside:?}");
        }
        // 본 창 안이어도 칸이 없는 곳(사이드바 자리: 본문 왼쪽 밖)은 자리가 없다.
        assert_eq!(at(150.0, 350.0), None);
        // 새 창은 놓은 지점에서 (-60, -16)만큼 옮긴 자리에 연다.
        assert_eq!(detached_origin((400.0, 300.0)), (340.0, 284.0));
    }

    #[test]
    fn test_c_d83_dropping_on_the_strip_of_a_column_appends_to_that_pane() {
        let slots = two_slots();
        // 오른쪽 열의 탭 줄(본문 위 30px 띠, 창 안 y 30~60): 그 칸의 끝 탭 3 뒤에 붙인다.
        let on_strip = preview_at(&slots, 1, 800.0, 45.0).unwrap();
        assert_eq!(on_strip.place, DockPlace { reference: 3, mode: DropMode::TabAfter });
        assert!(!on_strip.line);
        // 왼쪽 열 탭 줄에서 왼쪽 칸 탭 2를 끌면 그 칸 끝 탭이 자기 자신이라 바뀌는 것이 없다. 탭 1을 끌면 2 뒤로 간다.
        assert_eq!(preview_at(&slots, 2, 300.0, 45.0), None);
        assert_eq!(
            preview_at(&slots, 1, 300.0, 45.0).map(|preview| preview.place),
            Some(DockPlace { reference: 2, mode: DropMode::TabAfter })
        );
        // 위아래로 나눈 열의 아래 칸은 자기 위에 탭 줄을 가지고, 그 줄도 그 칸의 끝이다.
        let stacked = Layout::Split {
            orientation: Orientation::Vertical,
            children: vec![pane(&[1], 0), pane(&[2, 3], 0)],
            sizes: vec![0.5, 0.5],
        };
        let slots = pane_slots(&stacked, (800.0, 500.0), (200.0, 60.0), 30.0);
        assert!(slots[0].is_top && !slots[1].is_top);
        assert_eq!(slots[1].strip, Rect { left: 200.0, top: 310.0, width: 800.0, height: 30.0 });
        assert_eq!(slots[1].body, Rect { left: 200.0, top: 340.0, width: 800.0, height: 220.0 });
        assert_eq!(
            preview_at(&slots, 1, 500.0, 320.0).map(|preview| preview.place),
            Some(DockPlace { reference: 3, mode: DropMode::TabAfter })
        );
        // 탭 위에 놓으면 왼쪽 절반은 그 탭 앞, 오른쪽 절반은 뒤에 끼우고 선은 그 가장자리에 선다. 같은 자리면 없다.
        let tab = RowTab { id: 3, left: 100.0, right: 250.0 };
        let before = row_preview(&[2, 3], 1, tab, 34.0, 26.0, 120.0).unwrap();
        assert_eq!(before.place, DockPlace { reference: 3, mode: DropMode::TabBefore });
        assert!(before.line);
        assert_eq!((before.rect.left, before.rect.width), (99.0, 2.0));
        assert_eq!(row_preview(&[2, 3], 2, tab, 34.0, 26.0, 120.0), None, "2 already sits before 3");
    }
}
