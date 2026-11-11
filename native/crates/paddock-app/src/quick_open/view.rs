//! 빠른 열기 보기: 대화상자 틀 안에 입력칸과 점수 순 목록(이름 + 폴더), 색인 중·일치 없음·상한 안내.
use gpui_kit::component::input::Input;
use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use paddock_core::quick_open::{CAPPED_NOTICE, NO_MATCH, display};
use paddock_core::theme::{dialog as dialog_size, quick_open as size, rename, text};

use crate::chrome::{dialog_sized, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 목록 한 줄: 이름과 폴더(흐림). 강조한 줄은 `hover_bg` 바탕이다.
fn row(
    index: usize,
    relative: &str,
    highlighted: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let (name, folder) = display(relative);
    let line = div()
        .id(("quick-open-row", index))
        .w_full()
        .h(zpx(size::ROW_HEIGHT))
        .flex_none()
        .px(zpx(size::ROW_PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::NAME_GAP))
        .rounded(zpx(size::ROW_RADIUS))
        .cursor_pointer()
        .text_size(zpx(text::UI))
        .text_color(hsla(palette.fg_strong));
    let line = if highlighted { line.bg(hsla(palette.hover_bg)) } else { line };
    line.child(div().flex_none().max_w_full().truncate().child(name))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(folder),
        )
        .on_mouse_move(
            cx.listener(move |this, _event: &gpui_kit::MouseMoveEvent, _window, cx| this.quick_open_hover(index, cx)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.quick_open_pick(Some(index), window, cx)),
        )
}

/// 빠른 열기 창. 열려 있지 않으면 None이다.
pub fn quick_open_dialog(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Option<impl IntoElement> {
    let palette = colors();
    window_view.quick_open.as_ref().map(|quick| {
        let highlight = quick.nav.highlight;
        let rows: Vec<_> = quick
            .shown()
            .enumerate()
            .map(|(index, relative)| row(index, relative, highlight == Some(index), cx))
            .collect();
        let empty = rows.is_empty();
        let note = if empty && quick.indexing {
            Some("Indexing\u{2026}")
        } else if empty {
            Some(NO_MATCH)
        } else {
            None
        };
        let list = div()
            .id("quick-open-list")
            .max_h(zpx(size::ROW_HEIGHT * size::VISIBLE_ROWS as f32))
            .overflow_y_scroll()
            .track_scroll(&quick.scroll)
            .flex()
            .flex_col()
            .children(rows);
        let body = div()
            .flex()
            .flex_col()
            .gap(zpx(dialog_size::CONTENT_GAP))
            .child(
                div()
                    .h(zpx(rename::INPUT_HEIGHT))
                    .rounded(zpx(rename::INPUT_RADIUS))
                    .border_1()
                    .border_color(hsla(palette.accent))
                    .child(Input::new(&quick.input).appearance(false)),
            )
            .child(list)
            .children(note.map(|note| div().text_size(zpx(text::SMALL)).text_color(hsla(palette.fg_muted)).child(note)))
            .children(
                quick
                    .capped
                    .then(|| div().text_size(zpx(text::SMALL)).text_color(hsla(palette.warning)).child(CAPPED_NOTICE)),
            );
        dialog_sized(&window_view.overlay_focus, "Go to file", size::WIDTH, body)
    })
}
