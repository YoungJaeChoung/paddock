//! Work 목록의 저장소 묶음 행: 같은 git 저장소의 작업 폴더 둘 이상을 한 줄로 묶는다. 펼침 표지·저장소 이름·폴더 수만 있다.
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use paddock_core::theme::{folder_row as size, sidebar, text};

use crate::chrome::{unseen_dot, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 행 하나를 그리는 데 필요한 값.
pub struct RepoRowSpec {
    pub index: usize,
    /// 첫 구성원 폴더가 저장한 저장소 key. 누르면 이 묶음이 접히고 펼쳐진다.
    pub key: String,
    /// 저장소 루트 폴더의 이름.
    pub name: String,
    pub expanded: bool,
    pub n_folders: usize,
    /// 선택한 터미널이 묶음 안 폴더에 있다.
    pub current: bool,
    /// 묶음 안 폴더에 읽지 않은 완료 알림이 있다.
    pub unseen: bool,
    /// 마우스를 올리면 뜨는 설명(저장소 경로·읽지 않은 알림·입력을 기다리는 에이전트 수).
    pub tip: String,
}

/// 묶음 행의 폴더 수 글자.
pub fn folders_label(n_folders: usize) -> String {
    format!("{n_folders} folders")
}

/// 저장소 묶음 행을 만든다. 행을 누르면 묶음을 접고 펼친다.
pub fn repo_row(
    spec: RepoRowSpec,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let (background, foreground, weight) = if spec.current {
        (palette.current_bg, palette.fg_strong, FontWeight::SEMIBOLD)
    } else {
        (palette.sidebar_bg, palette.fg, FontWeight::NORMAL)
    };
    let toggle_key = spec.key.clone();
    let chevron = if spec.expanded { "\u{25be}" } else { "\u{25b8}" };
    div()
        .id(("work-repo-row", spec.index))
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
        .tooltip(crate::usage::bar::tooltip(spec.tip))
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
                .text_color(hsla(palette.fg_muted))
                .child(folders_label(spec.n_folders)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.toggle_repo(&toggle_key, window, cx)),
        )
}
