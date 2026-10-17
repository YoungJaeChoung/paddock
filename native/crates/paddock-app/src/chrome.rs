//! 창 틀(제목줄·Work 목록·탭 줄·상태 줄·묻기 창·안내)이 함께 쓰는 크기 변환과 부품. 값은 모두 core의 `theme`에서 온다.
//! 터미널 격자 자체의 색은 `terminal::palette`가 정한다.
//!
//! 1px 선(`border_*_1`)은 확대 배율을 곱하지 않고 GPUI가 장치 픽셀에 맞춰 얇게 그린다. 선 굵기 값을 따로 두지 않는 근거다.
use gpui_kit::{Div, FontWeight, InteractiveElement, ParentElement, Pixels, Styled, div, px};
use paddock_core::agent_model::ActivityState;
use paddock_core::theme::{agent_row, dialog as dialog_size, text};

use crate::look::{colors, scaled};
use crate::terminal::paint::hsla;

/// 100% 배율의 크기를 이 실행의 배율로 키운 화면 크기.
pub fn zpx(size: f32) -> Pixels {
    px(scaled(size))
}

/// 버튼의 종류. 색은 종류가 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// 기본 선택(Enter가 누르는 버튼).
    Primary,
    Secondary,
    /// 되돌릴 수 없는 선택.
    Destructive,
}

/// 묻기 창과 안내 창의 버튼 하나. 누름 처리는 호출자가 `on_mouse_down`으로 붙인다.
pub fn button(
    label: &'static str,
    kind: ButtonKind,
) -> Div {
    let palette = colors();
    let (background, foreground) = match kind {
        ButtonKind::Primary => (palette.button_primary_bg, palette.button_primary_fg),
        ButtonKind::Secondary => (palette.button_secondary_bg, palette.button_secondary_fg),
        ButtonKind::Destructive => (palette.button_destructive_bg, palette.button_destructive_fg),
    };
    div()
        .min_w(zpx(dialog_size::BUTTON_MIN_WIDTH))
        .px(zpx(dialog_size::BUTTON_PADDING_X))
        .py(zpx(dialog_size::BUTTON_PADDING_Y))
        .flex()
        .justify_center()
        .rounded(zpx(dialog_size::BUTTON_RADIUS))
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .text_size(zpx(text::DIALOG_BUTTON))
        .cursor_pointer()
        .child(label)
}

/// 체크박스 표시. 켜져 있으면 강조색으로 채우고 체크 표시를 그린다. 누름 처리는 호출자가 붙인다.
pub fn checkbox(checked: bool) -> Div {
    let palette = colors();
    let mut box_ = div()
        .w(zpx(dialog_size::CHECKBOX_SIZE))
        .h(zpx(dialog_size::CHECKBOX_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(dialog_size::CHECKBOX_RADIUS))
        .border_1()
        .border_color(hsla(if checked { palette.accent } else { palette.fg_muted }))
        .text_size(zpx(text::SMALL))
        .text_color(hsla(palette.fg_strong));
    if checked {
        box_ = box_.bg(hsla(palette.accent)).child("\u{2713}");
    }
    box_
}

/// 창 위에 뜨는 묻기 창(어두운 막과 가운데 상자). 키를 받을 `focus`가 상자에 붙고,
/// 상자는 제목줄(`title`)과 본문(`body`)으로 이뤄진다.
pub fn dialog(
    focus: &gpui_kit::FocusHandle,
    title: impl Into<gpui_kit::SharedString>,
    body: impl gpui_kit::IntoElement,
) -> Div {
    dialog_sized(focus, title, dialog_size::WIDTH, body)
}

/// `dialog`와 같되 폭(100% 배율의 px)을 정한다.
pub fn dialog_sized(
    focus: &gpui_kit::FocusHandle,
    title: impl Into<gpui_kit::SharedString>,
    width: f32,
    body: impl gpui_kit::IntoElement,
) -> Div {
    let palette = colors();
    let panel = div()
        .id("dialog")
        .track_focus(focus)
        .w(zpx(width))
        .flex()
        .flex_col()
        .rounded(zpx(dialog_size::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg_strong))
        .text_size(zpx(text::DIALOG_BODY))
        .line_height(gpui_kit::relative(text::DIALOG_LINE_HEIGHT))
        .shadow_lg()
        .child(
            div()
                .min_h(zpx(dialog_size::TITLE_HEIGHT))
                .pl(zpx(dialog_size::TITLE_PADDING_LEFT))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(hsla(palette.border))
                .text_size(zpx(text::DIALOG_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(div().p(zpx(dialog_size::CONTENT_PADDING)).child(body));
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui_kit::rgba(palette.dialog_scrim))
        .occlude()
        .child(panel)
}

/// 읽지 않은 완료 알림 점. 행 이름과 탭 이름 뒤에 같은 모양이 붙는다.
pub fn unseen_dot() -> Div {
    div()
        .flex_none()
        .ml(zpx(agent_row::UNSEEN_DOT_MARGIN_LEFT))
        .w(zpx(agent_row::UNSEEN_DOT_SIZE))
        .h(zpx(agent_row::UNSEEN_DOT_SIZE))
        .rounded(zpx(agent_row::UNSEEN_DOT_SIZE / 2.0))
        .bg(hsla(colors().accent))
}

/// 에이전트 상태 칸: 같은 색의 점과 글자(`Sent`·`Output`·`Quiet`). 폭이 고정이라 글자가 바뀌어도 오른쪽 열이 흔들리지 않는다.
pub fn agent_status(
    label: &'static str,
    activity: ActivityState,
) -> Div {
    let palette = colors();
    let color = match activity {
        ActivityState::Working => palette.accent,
        ActivityState::Waiting => palette.warning,
        ActivityState::Quiet | ActivityState::Unknown => palette.fg_muted,
    };
    div()
        .flex_none()
        .w(zpx(agent_row::STATUS_WIDTH))
        .ml(zpx(agent_row::STATUS_MARGIN_LEFT))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(agent_row::STATUS_GAP))
        .text_size(zpx(text::SMALL))
        .text_color(hsla(color))
        .child(
            div()
                .flex_none()
                .w(zpx(agent_row::STATUS_DOT_SIZE))
                .h(zpx(agent_row::STATUS_DOT_SIZE))
                .rounded(zpx(agent_row::STATUS_DOT_SIZE / 2.0))
                .bg(hsla(color)),
        )
        .child(label)
}

/// 창 틀 전체의 글꼴 이름(빠른 설정에서 바꿀 수 있다).
pub fn ui_family() -> gpui_kit::SharedString {
    crate::look::interface_family().into()
}

/// 부품 라이브러리(입력칸·텍스트 보기 편집기 등)의 색 모드·글꼴·색을 앱의 설정에 맞춘다.
///
/// 라이트 테마에서 입력칸 글자가 밝은 바탕에 밝게 남지 않게 색 모드를 맞추고, 텍스트 보기 편집기가 터미널과 같은 글꼴·크기와
/// 앱의 구문 강조 색을 쓰게 한다. 설정(테마·배율·글꼴)이 바뀔 때마다 부른다.
pub fn sync_component_theme(cx: &mut gpui_kit::App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};
    use paddock_core::theme::Mode;

    let mode = match crate::look::mode() {
        Mode::Dark => ThemeMode::Dark,
        Mode::Light => ThemeMode::Light,
    };
    Theme::change(mode, None, cx);
    let palette = crate::look::colors();
    let highlight = crate::files::text_view::highlight_theme(palette, crate::look::mode());
    Theme::update(cx, |theme| {
        if let Some(highlight) = highlight {
            theme.highlight_theme = highlight;
        }
        theme.mono_font_family = crate::look::terminal_family().into();
        theme.mono_font_size = px(crate::look::scaled(crate::look::terminal_font_size()));
        theme.colors.foreground = hsla(palette.terminal_fg);
    });
}
