//! 안내 탭: 볼 수 없는 파일의 이름·크기·문구와 동작 단추. 가운데에 세로로 놓인다.
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
};
use paddock_core::theme::notice_tab as size;

use super::open::NoticeAction;
use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 안내 탭 하나의 내용.
pub struct NoticeView<'a> {
    pub name: &'a str,
    /// 크기 글자(`3.4 MB`). 크기를 모르면 줄을 뺀다.
    pub size: Option<String>,
    pub message: &'a str,
    pub actions: &'a [NoticeAction],
}

/// 안내 탭을 그린다. 단추를 누르면 `path`의 파일에 대해 그 동작을 실행한다.
pub fn notice_view(
    view: &NoticeView<'_>,
    path: &str,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let mut actions = div()
        .mt(zpx(size::ACTIONS_MARGIN_TOP))
        .flex()
        .flex_row()
        .flex_wrap()
        .justify_center()
        .gap(zpx(size::ACTIONS_GAP));
    for (index, action) in view.actions.iter().copied().enumerate() {
        let path = path.to_string();
        actions = actions.child(
            div()
                .id(("notice-action", index))
                .h(zpx(size::BUTTON_HEIGHT))
                .px(zpx(size::BUTTON_PADDING_X))
                .flex()
                .items_center()
                .rounded(zpx(size::BUTTON_RADIUS))
                .border_1()
                .border_color(hsla(palette.popup_border))
                .text_size(zpx(size::BUTTON_TEXT_SIZE))
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
                .child(action.label())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        this.run_notice_action(action, &path, window, cx)
                    }),
                ),
        );
    }
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(palette.terminal_bg))
        .text_color(hsla(palette.fg))
        .child(
            div()
                .max_w(zpx(size::MAX_WIDTH))
                .p(zpx(size::PADDING))
                .flex()
                .flex_col()
                .items_center()
                .gap(zpx(size::GAP))
                .text_center()
                .child(
                    div()
                        .text_size(zpx(size::NAME_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hsla(palette.fg_strong))
                        .child(view.name.to_string()),
                )
                .children(
                    view.size.clone().map(|text| {
                        div().text_size(zpx(size::BODY_SIZE)).text_color(hsla(palette.fg_muted)).child(text)
                    }),
                )
                .child(
                    div()
                        .mt(zpx(size::MESSAGE_MARGIN_TOP))
                        .text_size(zpx(size::BODY_SIZE))
                        .child(view.message.to_string()),
                )
                .child(actions),
        )
}
