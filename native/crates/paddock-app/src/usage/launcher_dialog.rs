//! WSL 터미널에도 Claude 사용량을 보일지 묻는 창. `~/.bashrc`에 줄 하나를 넣어도 되는지 한 번 묻는다.
//!
//! 묻는 것은 얻는 것(사용량 표시·메모리 경고)과 바뀌는 것(`.bashrc` 한 줄)뿐이다. 사용자 파일을 고치는 선택이 Enter 한 번으로 되지 않게
//! 처음 초점과 Enter는 `Ask me later`에 있고, `Turn on`은 눌러야만 켜진다.
use gpui_kit::{
    Context, FocusHandle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
};
use paddock_core::theme::{dialog as dialog_size, wsl_launcher_dialog as size};

use crate::chrome::{ButtonKind, button, dialog_sized, zpx};
use crate::window_view::WorkWindow;

/// 창 제목.
pub const TITLE: &str = "Show Claude usage in WSL terminals too?";
/// 본문. 문구는 기존 앱의 같은 창과 같다.
pub const MESSAGE: &str = "When you run claude in a regular WSL terminal, Paddock can show your usage and tell Claude when memory runs low. This adds one line to your ~/.bashrc. You can undo it in Settings.";

/// 묻기 창을 그린다. 단추는 왼쪽부터 `Ask me later`·`No thanks`·`Turn on`이다.
pub fn wsl_launcher_dialog(
    focus: &FocusHandle,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let later = button("Ask me later", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
    );
    let decline = button("No thanks", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.wsl_launcher_answer(false, window, cx)),
    );
    let turn_on = button("Turn on", ButtonKind::Primary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.wsl_launcher_answer(true, window, cx)),
    );
    let body = div().flex().flex_col().gap(zpx(dialog_size::CONTENT_GAP)).child(MESSAGE).child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .pt(zpx(dialog_size::CONTENT_PADDING))
            .gap(zpx(dialog_size::BUTTON_GAP))
            .child(later)
            .child(decline)
            .child(turn_on),
    );
    dialog_sized(focus, TITLE, size::WIDTH, body)
}
