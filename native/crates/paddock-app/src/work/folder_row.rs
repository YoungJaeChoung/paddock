//! Work 목록의 작업 폴더 행: 펼침 표지·폴더 그림·이름·`>_ N`과, 마우스를 올렸을 때 나오는 ＋·⋯ 단추.
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
};
use paddock_core::theme::{folder_row as size, sidebar, text};
use paddock_core::work_model::NewPlace;

use super::menu_model::{MenuKind, count_label};
use crate::chrome::{unseen_dot, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 행 하나를 그리는 데 필요한 값.
pub struct FolderRowSpec {
    pub index: usize,
    pub key: String,
    /// 목록에 보이는 이름(사용자가 바꾼 이름, 홈이면 `~`, 아니면 폴더 이름).
    pub name: String,
    pub expanded: bool,
    pub n_terminals: usize,
    /// 선택한 터미널이 이 폴더에 있다.
    pub current: bool,
    pub unseen: bool,
}

/// 몸통과 귀를 가진 작은 폴더 그림. 아이콘 글꼴을 싣지 않으므로 도형으로 그린다.
pub(crate) fn folder_glyph(color: u32) -> impl IntoElement {
    div()
        .flex_none()
        .w(zpx(size::ICON_BOX))
        .h(zpx(size::ICON_BOX))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div().w(zpx(size::FOLDER_WIDTH)).flex().flex_row().child(
                div()
                    .w(zpx(size::FOLDER_TAB_WIDTH))
                    .h(zpx(size::FOLDER_TAB_HEIGHT))
                    .bg(hsla(color))
                    .rounded_t(zpx(size::FOLDER_RADIUS)),
            ),
        )
        .child(
            div()
                .w(zpx(size::FOLDER_WIDTH))
                .h(zpx(size::FOLDER_HEIGHT))
                .bg(hsla(color))
                .rounded_b(zpx(size::FOLDER_RADIUS)),
        )
}

/// 호버 때 보이는 20×20 단추.
fn action_button(label: &'static str) -> gpui_kit::Div {
    div()
        .flex_none()
        .w(zpx(size::ACTION_SIZE))
        .h(zpx(size::ACTION_SIZE))
        .ml(zpx(size::ACTION_MARGIN_LEFT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(sidebar::ACTION_RADIUS))
        .text_size(zpx(text::SMALL))
        .text_color(hsla(colors().fg_muted))
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child(label)
}

/// 작업 폴더 행을 만든다. 행을 누르면 접고 펼치며, 오른쪽 누름과 ⋯는 폴더 메뉴, ＋는 그 폴더에 새 터미널을 연다.
pub fn folder_row(
    spec: FolderRowSpec,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let (background, foreground, weight, count_color) = if spec.current {
        (palette.current_bg, palette.fg_strong, FontWeight::SEMIBOLD, palette.fg)
    } else {
        (palette.sidebar_bg, palette.fg, FontWeight::NORMAL, palette.fg_muted)
    };
    let (toggle_key, menu_key, add_key, more_key) =
        (spec.key.clone(), spec.key.clone(), spec.key.clone(), spec.key.clone());
    let chevron = if spec.expanded { "\u{25be}" } else { "\u{25b8}" };
    div()
        .id(("work-folder-row", spec.index))
        .group("work-folder-row")
        .h(zpx(sidebar::ROW_HEIGHT))
        .pl(zpx(size::PADDING_LEFT))
        .pr(zpx(sidebar::ROW_PADDING_RIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::GAP))
        .cursor_pointer()
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .text_size(zpx(text::UI))
        .font_weight(weight)
        .hover(move |style| if spec.current { style } else { style.bg(hsla(colors().hover_bg)) })
        .child(
            div()
                .flex_none()
                .w(zpx(size::ICON_BOX))
                .flex()
                .justify_center()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg))
                .child(chevron),
        )
        .child(folder_glyph(foreground))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .child(div().min_w_0().truncate().child(spec.name))
                .children(spec.unseen.then(unseen_dot)),
        )
        .child(
            div()
                .flex_none()
                .text_size(zpx(text::SMALL))
                .font_weight(FontWeight::NORMAL)
                .text_color(hsla(count_color))
                .child(count_label(spec.n_terminals)),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .opacity(0.0)
                .group_hover("work-folder-row", |style| style.opacity(1.0))
                .child(action_button("+").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_new_terminal_menu(
                            NewPlace::Folder(add_key.clone()),
                            event.position,
                            false,
                            window,
                            cx,
                        );
                    }),
                ))
                .child(action_button("\u{22ef}").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_menu(MenuKind::Folder(more_key.clone()), event.position, window, cx);
                    }),
                )),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.toggle_folder(&toggle_key, window, cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_menu(MenuKind::Folder(menu_key.clone()), event.position, window, cx);
            }),
        )
}
