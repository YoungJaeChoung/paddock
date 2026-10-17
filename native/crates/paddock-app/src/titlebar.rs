//! 제목줄: 앱 이름과 창 이동 영역, 최소화·최대화·닫기 버튼. Windows에서만 직접 그린다(다른 OS는 시스템 제목줄을 쓴다).
//!
//! 버튼은 눌림 처리를 하지 않는다. `window_control_area` 표시만 달아 두면 운영체제가 그 영역의 누름을 창 동작으로 처리한다.
use gpui_kit::{
    Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled, Window, WindowControlArea, div,
};
use paddock_core::theme::{font, text, titlebar};

use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;

/// 창 버튼 하나(글리프 한 글자).
fn control(
    id: &'static str,
    glyph: &'static str,
    area: WindowControlArea,
) -> impl IntoElement {
    let palette = colors();
    let (hover_bg, hover_fg) = if area == WindowControlArea::Close {
        (palette.control_close_hover_bg, palette.control_close_hover_fg)
    } else {
        (palette.control_hover_bg, palette.fg_strong)
    };
    div()
        .id(id)
        .w(zpx(titlebar::CONTROL_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_color(hsla(palette.titlebar_fg))
        .font_family(font::CONTROL_ICON_FAMILY)
        .text_size(zpx(text::CONTROL_ICON))
        .window_control_area(area)
        .hover(move |style| style.bg(hsla(hover_bg)).text_color(hsla(hover_fg)))
        .child(glyph)
}

/// 제목줄. 시스템 제목줄을 쓰는 OS에서는 None이다.
pub fn titlebar_view(window: &Window) -> Option<Div> {
    let palette = colors();
    cfg!(windows).then(|| {
        let maximize = if window.is_maximized() {
            control("window-restore", font::CONTROL_RESTORE, WindowControlArea::Max)
        } else {
            control("window-maximize", font::CONTROL_MAXIMIZE, WindowControlArea::Max)
        };
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .h(zpx(titlebar::HEIGHT))
            .border_b_1()
            .border_color(hsla(palette.border))
            .bg(hsla(palette.titlebar_bg))
            .text_color(hsla(palette.titlebar_fg))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(zpx(titlebar::BRAND_PADDING_X))
                    .window_control_area(WindowControlArea::Drag)
                    .text_size(zpx(text::TITLEBAR_BRAND))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Paddock"),
            )
            .child(control("window-minimize", font::CONTROL_MINIMIZE, WindowControlArea::Min))
            .child(maximize)
            .child(control("window-close", font::CONTROL_CLOSE, WindowControlArea::Close))
    })
}
