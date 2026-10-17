//! Work 목록의 터미널 행: 작업 폴더 안의 터미널 하나, 또는 Unassigned 묶음 하나.
//!
//! 묶음 행은 묶음의 시작 터미널 이름을 보이고 묶음에 터미널이 둘 이상이면 `>_ N`을 붙이며, 누르면 묶음에서 마지막으로 쓴
//! 터미널로 돌아가고 ×는 묶음 전체를 닫는다. 폴더 안 터미널 행에는 ×가 없다(탭 줄의 ×나 우클릭 메뉴로 닫는다).
use gpui_kit::{Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div};
use paddock_core::theme::{agent_row, sidebar, text};

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

    fn element_id(self) -> (&'static str, usize) {
        match self {
            Self::Terminal(id) => ("work-row", id as usize),
            Self::Group(root) => ("work-group-row", root as usize),
        }
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
    pub status: Option<Status>,
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
    let mut row = div()
        .id(target.element_id())
        .h(zpx(sidebar::ROW_HEIGHT))
        .pl(zpx(sidebar::TERMINAL_ROW_PADDING_LEFT))
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
            div()
                .flex_none()
                .max_w(zpx(agent_row::META_MAX_WIDTH))
                .ml(zpx(agent_row::META_MARGIN_LEFT))
                .truncate()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(meta_color))
                .child(spec.meta)
        }))
        .children(spec.status.map(|status| agent_status(status.label, status.activity)));
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
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.select_row(target, window, cx)),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            this.open_menu(target.menu(), event.position, window, cx);
        }),
    )
}
