//! 편집 흐름의 화면 조각: 상태 줄 오른쪽 저장 표시(`Unsaved changes` + `Save` 단추, `Saved`)와 파일 내용 위의 디스크 변경 띠(`Reload`·`Keep mine`).
use std::rc::Rc;

use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window, div,
};
use paddock_core::theme::{editor, text};
use paddock_core::unsaved::{DISK_CHANGED_BAR, STATUS_SAVED, STATUS_UNSAVED, SaveStatus};

use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 글자 단추 하나(전체 테두리, 올리면 바탕). 누름 처리는 호출자가 붙인다.
fn small_button(
    id: &'static str,
    label: &'static str,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    div()
        .id(id)
        .flex_none()
        .h(zpx(editor::BUTTON_HEIGHT))
        .px(zpx(editor::BUTTON_PADDING_X))
        .flex()
        .items_center()
        .rounded(zpx(editor::BUTTON_RADIUS))
        .border_1()
        .border_color(hsla(palette.border))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(label)
}

/// 상태 줄 오른쪽 저장 표시. 지금 파일 탭이 저장하지 않았으면 `Unsaved changes` + `Save` 단추, 방금 저장했으면 `Saved`, 아니면 None이다.
pub fn save_status_view(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Option<Div> {
    window_view.save_status().map(|status| match status {
        SaveStatus::Unsaved => {
            div().flex().flex_row().items_center().gap(zpx(editor::STATUS_GAP)).child(STATUS_UNSAVED).child(
                small_button("status-save", "Save").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.save_active_file(cx);
                        this.focus_selected(window, cx);
                    }),
                ),
            )
        }
        SaveStatus::Saved => div().child(STATUS_SAVED),
    })
}

/// 디스크 변경 띠 단추를 눌렀을 때 부를 동작: (그리는 보기, 파일 경로, 창, 컨텍스트).
pub type ChangedHandler<T> = Rc<dyn Fn(&mut T, &str, &mut Window, &mut Context<T>)>;

/// 디스크가 바뀐 파일 `path`의 내용 위에 띠를 붙인다: `This file changed on disk.` + `Reload`(고친 내용을 버리고 다시 읽기) + `Keep mine`.
/// 알림은 전체 테두리가 아니라 바탕색(경고색 연하게)으로 한다. 단추를 누르면 `on_reload`·`on_keep`을 부른다.
pub fn with_changed_bar<T: 'static>(
    content: impl IntoElement,
    path: &str,
    cx: &mut Context<T>,
    on_reload: &ChangedHandler<T>,
    on_keep: &ChangedHandler<T>,
) -> Div {
    let palette = colors();
    let (reload_path, keep_path) = (path.to_string(), path.to_string());
    let (on_reload, on_keep) = (on_reload.clone(), on_keep.clone());
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .h(zpx(editor::CHANGED_BAR_HEIGHT))
                .px(zpx(editor::CHANGED_BAR_PADDING_X))
                .flex()
                .flex_row()
                .items_center()
                .gap(zpx(editor::CHANGED_BAR_GAP))
                .bg(hsla(palette.warning).opacity(0.18))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg))
                .child(div().flex_1().min_w_0().truncate().child(DISK_CHANGED_BAR))
                .child(small_button("changed-reload", "Reload").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut T, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        on_reload(this, &reload_path, window, cx);
                    }),
                ))
                .child(small_button("changed-keep", "Keep mine").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut T, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        on_keep(this, &keep_path, window, cx);
                    }),
                )),
        )
        .child(div().flex_1().min_h_0().child(content))
}
