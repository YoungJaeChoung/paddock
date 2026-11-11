//! Work 목록의 터미널 행: 작업 폴더 안의 터미널 하나, 또는 Unassigned 묶음 하나.
//!
//! 묶음 행은 묶음의 시작 터미널 이름을 보이고 묶음에 터미널이 둘 이상이면 `>_ N`을 붙이며, 누르면 묶음에서 마지막으로 쓴
//! 터미널로 돌아가고 ×는 묶음 전체를 닫는다. 폴더 안 터미널 행에는 ×가 없다(탭 줄의 ×나 우클릭 메뉴로 닫는다).
use gpui_kit::{
    AppContext, ClickEvent, Context, DragMoveEvent, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, div,
};
use paddock_core::theme::{agent_row, drop_marker, row_remote_meta, sidebar, text};
use paddock_core::work_model::{DropPosition, drop_position};

use super::menu_model::{MenuKind, count_label};
use crate::agent_view::Status;
use crate::chrome::{agent_status, unseen_dot, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 행이 가리키는 대상.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTarget {
    /// 작업 폴더 안의 터미널.
    Terminal(u32),
    /// Unassigned 묶음. 값은 묶음의 시작 터미널.
    Group(u32),
}

impl RowTarget {
    /// 행의 우클릭 메뉴.
    pub fn menu(self) -> MenuKind {
        match self {
            Self::Terminal(id) => MenuKind::Terminal(id),
            Self::Group(root) => MenuKind::Group(root),
        }
    }

    /// 행의 터미널 번호(터미널 행은 그 터미널, 묶음 행은 시작 터미널).
    pub fn key(self) -> u32 {
        match self {
            Self::Terminal(id) | Self::Group(id) => id,
        }
    }

    fn element_id(self) -> (&'static str, usize) {
        match self {
            Self::Terminal(id) => ("work-row", id as usize),
            Self::Group(root) => ("work-group-row", root as usize),
        }
    }
}

/// 끌고 있는 터미널 행. 같은 폴더의 다른 행 위에 놓으면 순서가 바뀐다.
#[derive(Clone, Debug)]
pub struct DragRow {
    pub id: u32,
    /// 끄는 동안 마우스 곁에 보이는 이름.
    pub label: String,
}

impl Render for DragRow {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = colors();
        div()
            .h(zpx(sidebar::ROW_HEIGHT))
            .px(zpx(sidebar::TERMINAL_ROW_PADDING_BASE))
            .flex()
            .items_center()
            .rounded(zpx(sidebar::ACTION_RADIUS))
            .border_1()
            .border_color(hsla(palette.popup_border))
            .bg(hsla(palette.popup_bg))
            .text_color(hsla(palette.fg_strong))
            .text_size(zpx(text::UI))
            .child(self.label.clone())
    }
}

/// 행 하나를 그리는 데 필요한 값.
pub struct TerminalRowSpec {
    pub target: RowTarget,
    /// 이름과 같은 이름 번호(`·2`)를 이은 글자.
    pub label: String,
    /// 묶음의 터미널 수. 둘 이상일 때만 보인다.
    pub count: Option<usize>,
    pub selected: bool,
    pub exited: bool,
    pub unseen: bool,
    pub meta: String,
    /// 이 행의 터미널이 다른 창에 있다. `meta`가 `other window` 표지이고 일반 요약보다 작은 글자·넓은 최대 폭으로 그린다.
    pub remote: bool,
    pub status: Option<Status>,
    /// 마우스를 올리면 뜨는 설명. 묶음 행만 있다.
    pub tip: Option<String>,
    /// 끌어 온 행을 이 행에 놓을 자리를 알리는 선: 행 위(`Before`) 또는 아래(`After`)의 가로 전체 선이다.
    pub drop: Option<DropPosition>,
}

/// 터미널 행을 만든다. 왼쪽 누름은 선택, 오른쪽 누름은 메뉴, ×는 묶음 닫기다.
pub fn terminal_row(
    spec: TerminalRowSpec,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let target = spec.target;
    let selected = spec.selected;
    let (background, foreground) =
        if selected { (palette.tab_active_bg, palette.fg_strong) } else { (palette.sidebar_bg, palette.fg_muted) };
    // 미확인 완료 알림이 있는 행은 요약이 강조색이다.
    let meta_color = if spec.unseen { palette.accent } else { palette.fg_muted };
    let label = if spec.exited { format!("{} (exited)", spec.label) } else { spec.label };
    let marker = spec.drop;
    let drag_label = label.clone();
    let tip = spec.tip.clone();
    let mut row = div()
        .id(target.element_id())
        .relative()
        .h(zpx(sidebar::ROW_HEIGHT))
        .pl(zpx(sidebar::TERMINAL_ROW_PADDING_BASE + crate::look::sidebar_indent()))
        .pr(zpx(sidebar::ROW_PADDING_RIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(sidebar::ROW_GAP))
        .cursor_pointer()
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .text_size(zpx(text::UI))
        .hover(move |style| if selected { style } else { style.bg(hsla(colors().hover_bg)) })
        .child(div().flex_none().font_family(crate::look::terminal_family()).text_size(zpx(text::TINY)).child(">_"))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .child(div().min_w_0().truncate().child(label))
                .children(spec.count.filter(|count| *count > 1).map(|count| {
                    div()
                        .flex_none()
                        .ml(zpx(sidebar::ROW_GAP))
                        .text_size(zpx(text::SMALL))
                        .text_color(hsla(palette.fg_muted))
                        .child(count_label(count))
                }))
                .children(spec.unseen.then(unseen_dot)),
        )
        .children((!spec.meta.is_empty()).then(|| {
            let (max_width, text_size) = if spec.remote {
                (row_remote_meta::MAX_WIDTH, row_remote_meta::TEXT_SIZE)
            } else {
                (agent_row::META_MAX_WIDTH, text::SMALL)
            };
            div()
                .flex_none()
                .max_w(zpx(max_width))
                .ml(zpx(agent_row::META_MARGIN_LEFT))
                .truncate()
                .text_size(zpx(text_size))
                .text_color(hsla(meta_color))
                .child(spec.meta)
        }))
        .children(spec.status.as_ref().map(|status| agent_status(status, spec.target.key())));
    if let Some(tip) = tip {
        row = row.tooltip(crate::usage::bar::tooltip(tip));
    }
    if let RowTarget::Group(root) = target {
        row = row.child(
            div()
                .flex_none()
                .w(zpx(sidebar::CLOSE_SIZE))
                .h(zpx(sidebar::CLOSE_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(zpx(sidebar::ACTION_RADIUS))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
                .child("\u{2715}")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.request_close_group(root, window, cx);
                    }),
                ),
        );
    }
    // 터미널 행은 끌기를 시작하는 누름이 선택을 바꾸지 않게 선택을 뗄 때(끌지 않았을 때)로 미룬다. 묶음 행은 끌지 않으므로 누를 때 선택한다.
    let is_terminal = matches!(target, RowTarget::Terminal(_));
    row = row
        .children(marker.map(|position| {
            let line = div().absolute().left_0().right_0().h(zpx(drop_marker::LINE_WIDTH)).bg(hsla(colors().accent));
            match position {
                DropPosition::Before => line.top_0(),
                DropPosition::After => line.bottom_0(),
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if !is_terminal {
                    this.click_row(target, event.click_count, window, cx)
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_menu(target.menu(), event.position, window, cx);
            }),
        );
    // 작업 폴더 안의 터미널 행만 끌어 순서를 바꾼다(Unassigned 묶음 행은 폴더가 없다).
    if let RowTarget::Terminal(id) = target {
        row = row
            .on_drag(DragRow { id, label: drag_label }, |dragged, _offset, _window, cx| cx.new(|_| dragged.clone()))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                let click_count = match event {
                    ClickEvent::Mouse(click) => click.up.click_count,
                    _ => 1,
                };
                this.click_row(target, click_count, window, cx)
            }))
            .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<DragRow>, _window, cx| {
                let inside = event.bounds.contains(&event.event.position);
                let position = drop_position(
                    f32::from(event.event.position.y),
                    f32::from(event.bounds.origin.y),
                    f32::from(event.bounds.size.height),
                );
                this.hover_dragged_row(event.drag(cx).id, id, inside.then_some(position), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DragRow, window, cx| this.drop_row(dragged.id, id, window, cx)));
    }
    row
}
