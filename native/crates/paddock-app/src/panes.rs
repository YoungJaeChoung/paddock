//! 본문의 칸 트리 그리기: 칸마다 선택된 탭의 터미널 보기, 칸 사이 경계선과 끌어서 크기 바꾸기.
//!
//! 경계선은 1px `border` 색이고 그 위에 폭 8px의 잡는 영역이 겹쳐 있다. 마우스를 올리거나 끌면 가운데 2px만 강조색이 된다.
//! 끌기는 창 단위로 처리한다(`WorkWindow::drag_divider`) — 이 파일은 끌기 시작만 알린다.
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::{
    AnyElement, Bounds, Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Styled, canvas, div, relative,
};
use paddock_core::layout_model::{Layout, Orientation};
use paddock_core::pane_columns::tab_columns;
use paddock_core::theme::split;

use crate::chrome::zpx;
use crate::look::colors;
use crate::tabs::{StripGeometry, pane_tab_row};
use crate::terminal::paint::hsla;
use crate::window_view::{DividerStart, WorkWindow};

/// 칸 트리 전체를 그린다. 그리는 동안 본문이 차지한 크기를 `measured`에 남겨 탭 줄과 끌기가 쓰게 한다.
pub fn pane_area(
    window_view: &WorkWindow,
    layout: &Layout,
    geometry: StripGeometry,
    measured: Rc<Cell<(f32, f32)>>,
    cx: &mut Context<WorkWindow>,
) -> Div {
    // 맨 위가 아닌 칸은 자기 위에 탭 줄을 따로 가진다.
    let lower: HashSet<usize> = tab_columns(layout, geometry.width, geometry.height)
        .into_iter()
        .flat_map(|column| column.panes.into_iter().skip(1))
        .collect();
    let mut n_panes_visited = 0;
    let tree = node(window_view, layout, &[], &mut n_panes_visited, &lower, cx);
    div().size_full().relative().child(tree).child(
        canvas(
            |bounds, _window, _cx| bounds,
            move |_bounds, bounds: Bounds<Pixels>, _window, _cx| {
                measured.set((f32::from(bounds.size.width), f32::from(bounds.size.height)));
            },
        )
        .absolute()
        .size_full(),
    )
}

fn node(
    window_view: &WorkWindow,
    layout: &Layout,
    path: &[usize],
    n_panes_visited: &mut usize,
    lower: &HashSet<usize>,
    cx: &mut Context<WorkWindow>,
) -> AnyElement {
    match layout {
        Layout::Pane { tabs, selected } => {
            let index = *n_panes_visited;
            *n_panes_visited += 1;
            pane(window_view, index, tabs, *selected, lower.contains(&index), cx).into_any_element()
        }
        Layout::Split { orientation, children, sizes } => {
            let total: f64 = sizes.iter().sum();
            let share = |index: usize| {
                if total > 0.0 { sizes.get(index).copied().unwrap_or(1.0) / total } else { 1.0 / children.len() as f64 }
            };
            let mut container = div().relative().size_full().flex().overflow_hidden();
            container = match orientation {
                Orientation::Horizontal => container.flex_row(),
                Orientation::Vertical => container.flex_col(),
            };
            let mut before = 0.0_f64;
            let mut dividers = Vec::new();
            for (index, child) in children.iter().enumerate() {
                let mut child_path = path.to_vec();
                child_path.push(index);
                let element = node(window_view, child, &child_path, n_panes_visited, lower, cx);
                let fraction = share(index) as f32;
                let cell = div().flex_none().min_w_0().min_h_0().overflow_hidden().child(element);
                container = container.child(match orientation {
                    Orientation::Horizontal => cell.w(relative(fraction)).h_full(),
                    Orientation::Vertical => cell.h(relative(fraction)).w_full(),
                });
                before += share(index);
                if index + 1 < children.len() {
                    dividers.push(divider(window_view, path, index, *orientation, before as f32, cx));
                }
            }
            container.children(dividers).into_any_element()
        }
    }
}

/// 칸 하나: (맨 위가 아니면) 탭 줄과 선택된 탭의 터미널 보기. 보기를 누르면 그 터미널이 선택된다.
fn pane(
    window_view: &WorkWindow,
    index: usize,
    tabs: &[u32],
    selected: usize,
    has_own_tab_row: bool,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let current = tabs.get(selected.min(tabs.len().saturating_sub(1))).copied();
    let view = current.and_then(|id| window_view.workspace.tab(id)).map(|tab| tab.content.view.clone());
    let body = div().flex_1().min_h_0().size_full().children(view).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
            if let Some(id) = current.filter(|id| this.workspace.selected() != Some(*id) || this.file_active()) {
                this.select_tab(id, window, cx);
            }
        }),
    );
    let mut column = div().size_full().flex().flex_col().min_w_0().min_h_0();
    if has_own_tab_row {
        column = column.child(pane_tab_row(window_view, index + 1000, tabs, selected, cx));
    }
    column.child(body)
}

/// 두 칸 사이 경계선과 잡는 영역. `before`는 분할 안에서 이 경계 앞쪽이 차지한 비율이다.
fn divider(
    window_view: &WorkWindow,
    path: &[usize],
    index: usize,
    orientation: Orientation,
    before: f32,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let active = window_view.divider_is_dragged(path, index);
    let group = format!("divider-{}-{index}", path.iter().map(usize::to_string).collect::<Vec<_>>().join("-"));
    let line = |width: f32, color: u32| match orientation {
        Orientation::Horizontal => div().w(zpx(width)).h_full().bg(hsla(color)),
        Orientation::Vertical => div().h(zpx(width)).w_full().bg(hsla(color)),
    };
    let base = line(split::LINE_WIDTH, if active { palette.accent } else { palette.border });
    let line = if active { line(split::HOVER_WIDTH, palette.accent) } else { base };
    let line = line.group_hover(group.clone(), |style| style.bg(hsla(colors().accent)));
    let hit = div().absolute().group(group).flex().items_center().justify_center().child(line);
    let path = path.to_vec();
    let hit = match orientation {
        Orientation::Horizontal => hit
            .top_0()
            .bottom_0()
            .left(relative(before))
            .ml(zpx(-split::HIT_WIDTH / 2.0))
            .w(zpx(split::HIT_WIDTH))
            .cursor_col_resize(),
        Orientation::Vertical => hit
            .left_0()
            .right_0()
            .top(relative(before))
            .mt(zpx(-split::HIT_WIDTH / 2.0))
            .h(zpx(split::HIT_WIDTH))
            .cursor_row_resize(),
    };
    hit.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            // 경계를 잡는 누름이 이웃 칸으로 번져 칸 선택·포커스·글자 선택을 함께 시작하지 않게 한다.
            cx.stop_propagation();
            let position = match orientation {
                Orientation::Horizontal => f32::from(event.position.x),
                Orientation::Vertical => f32::from(event.position.y),
            };
            this.begin_divider_drag(
                DividerStart { path: path.clone(), index, orientation, position, file: false },
                window,
                cx,
            );
        }),
    )
}
