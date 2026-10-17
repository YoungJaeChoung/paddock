//! 이름 바꾸기 창: 터미널 이름(`Rename terminal`)이나 목록에 보이는 폴더 이름(`Rename in list`)을 입력칸 하나로 바꾼다.
//!
//! 입력칸은 gpui-component의 입력칸이다 — 한글 조합 중 Enter가 마지막 글자와 함께 한 번만 확정되는 동작을 검증한 부품이라
//! 이 창은 Enter를 직접 가로채지 않고 입력칸이 알리는 확정 사건을 받는다. 창 전체의 키 처리기는 이 창이 떠 있는 동안 Esc만 다룬다.
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{
    Context, Entity, FocusHandle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    Window, div,
};
use paddock_core::theme::{dialog as dialog_size, files_header, rename, text};

use crate::chrome::{ButtonKind, button, dialog, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::{RenameTarget, WorkWindow};

/// 이름 바꾸기 창의 제목.
pub fn rename_title(target: &RenameTarget) -> &'static str {
    match target {
        RenameTarget::Terminal(_) => "Rename terminal",
        RenameTarget::Folder(_) => "Rename in list",
    }
}

/// 이름 바꾸기 창. 입력칸에는 지금 이름이 들어 있고 전체가 선택돼 있다.
pub fn rename_dialog(
    target: &RenameTarget,
    input: &Entity<InputState>,
    focus: &FocusHandle,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    name_dialog(rename_title(target), input, None, true, focus, WorkWindow::commit_rename, cx)
}

/// 입력칸 하나로 이름을 받는 창(이름 바꾸기·새 파일·새 폴더). `reason`이 있으면 입력칸 아래에 보인다. `confirm_enabled`가 거짓이면 OK 단추가 흐려져 눌리지 않는다.
/// OK 단추는 `on_confirm`을 부른다.
pub fn name_dialog(
    title: impl Into<gpui_kit::SharedString>,
    input: &Entity<InputState>,
    reason: Option<&'static str>,
    confirm_enabled: bool,
    focus: &FocusHandle,
    on_confirm: fn(&mut WorkWindow, &mut Window, &mut Context<WorkWindow>),
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let cancel = button("Cancel", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
    );
    let mut confirm = button("OK", ButtonKind::Primary);
    confirm = if confirm_enabled {
        confirm.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| on_confirm(this, window, cx)),
        )
    } else {
        confirm.opacity(files_header::DISABLED_OPACITY).cursor_default()
    };
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
                .child(Input::new(input).appearance(false)),
        )
        .children(
            reason.map(|reason| div().text_size(zpx(text::SMALL)).text_color(hsla(palette.warning)).child(reason)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .pt(zpx(dialog_size::CONTENT_PADDING))
                .gap(zpx(dialog_size::BUTTON_GAP))
                .child(cancel)
                .child(confirm),
        );
    dialog(focus, title, body)
}
