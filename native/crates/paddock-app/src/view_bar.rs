//! 사이드바 맨 위의 보기 줄: 왼쪽에 지금 보는 보기(Work) 단추, 오른쪽 끝에 빠른 설정을 여는 톱니.
//!
//! 둘째 단추는 Source control 보기이고 변경 수 배지가 붙는다. 기존 앱의 Extensions 보기 단추 자리는 비워 둔다(해당 보기가 아직 없다).
//! 사이드바를 접으면 이 줄도 함께 접힌다.
use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
};
use paddock_core::theme::{view_badge, view_bar as size};

use crate::chrome::zpx;
use crate::look::colors;
use crate::scm::SidebarView;
use crate::scm::model::badge_text;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 보기 단추 하나의 바깥 상자(30x28). 안 그림은 호출자가 넣는다.
fn view_button() -> Div {
    div()
        .flex_none()
        .w(zpx(size::BUTTON_WIDTH))
        .h(zpx(size::BUTTON_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::BUTTON_RADIUS))
}

/// Work 보기를 뜻하는 목록 모양 그림: 첫 줄은 왼쪽 끝, 나머지 두 줄은 들여쓴 줄.
fn list_icon(color: u32) -> Div {
    let line =
        |indent: f32, width: f32| div().ml(zpx(indent)).w(zpx(width)).h(zpx(size::LIST_LINE_HEIGHT)).bg(hsla(color));
    div()
        .w(zpx(size::ICON_SIZE))
        .h(zpx(size::ICON_SIZE))
        .flex()
        .flex_col()
        .justify_center()
        .gap(zpx(size::LIST_LINE_GAP))
        .child(line(0.0, size::ICON_SIZE))
        .child(line(size::LIST_INDENT, size::ICON_SIZE - size::LIST_INDENT))
        .child(line(size::LIST_INDENT, size::ICON_SIZE - size::LIST_INDENT))
}

/// 가지 모양 그림(왼쪽 세로선 위아래의 동그라미 둘과 그 선에 이어진 오른쪽 위 동그라미)을 `icon_size` 한 변의 상자에 그린다.
/// 보기 줄 단추 그림(`size::ICON_SIZE`)을 기준으로 모든 자리·굵기를 비례해 줄이고 늘린다.
pub fn branch_icon(
    color: u32,
    icon_size: f32,
) -> Div {
    let scale = icon_size / size::ICON_SIZE;
    let circle_size = size::BRANCH_CIRCLE * scale;
    let thickness = size::LIST_LINE_HEIGHT / 2.0 * scale;
    let circle = |left: f32, top: f32| {
        div()
            .absolute()
            .left(zpx(left * scale))
            .top(zpx(top * scale))
            .w(zpx(circle_size))
            .h(zpx(circle_size))
            .rounded(zpx(circle_size / 2.0))
            .border_1()
            .border_color(hsla(color))
    };
    let line = |left: f32, top: f32, width: f32, height: f32| {
        div()
            .absolute()
            .left(zpx(left * scale))
            .top(zpx(top * scale))
            .w(zpx(width * scale))
            .h(zpx(height * scale))
            .bg(hsla(color))
    };
    let middle = |left: f32| left + size::BRANCH_CIRCLE / 2.0;
    div()
        .relative()
        .flex_none()
        .w(zpx(icon_size))
        .h(zpx(icon_size))
        .child(circle(size::BRANCH_LEFT, 0.0))
        .child(circle(size::BRANCH_LEFT, size::ICON_SIZE - size::BRANCH_CIRCLE))
        .child(circle(size::BRANCH_RIGHT, 0.0))
        .child(line(
            middle(size::BRANCH_LEFT),
            size::BRANCH_CIRCLE,
            thickness / scale,
            size::ICON_SIZE - 2.0 * size::BRANCH_CIRCLE,
        ))
        .child(line(middle(size::BRANCH_RIGHT), size::BRANCH_CIRCLE, thickness / scale, size::BRANCH_CIRCLE))
        .child(line(
            middle(size::BRANCH_LEFT),
            size::ICON_SIZE / 2.0,
            size::BRANCH_RIGHT - size::BRANCH_LEFT,
            thickness / scale,
        ))
}

/// 보기 단추 안에 그리는 가지 모양 그림.
fn branch_icon_16(color: u32) -> Div {
    branch_icon(color, size::ICON_SIZE)
}

/// 보기 단추 하나: 켜진 보기는 `current_bg` 바탕에 강조색 그림이고, 아니면 흐린 그림이며 위에 올리면 바탕이 생긴다.
fn view_choice(
    id: &'static str,
    active: bool,
    icon: impl Fn(u32) -> Div,
    badge: Option<String>,
    view: SidebarView,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let button = view_button()
        .id(id)
        .relative()
        .cursor_pointer()
        .child(icon(if active { palette.accent } else { palette.fg_muted }))
        .children(badge.map(|text| {
            div()
                .absolute()
                .top(zpx(view_badge::TOP))
                .left(zpx(view_badge::LEFT))
                .min_w(zpx(view_badge::MIN_WIDTH))
                .h(zpx(view_badge::HEIGHT))
                .px(zpx(view_badge::PADDING_X))
                .rounded(zpx(view_badge::HEIGHT / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(palette.badge_bg))
                .text_color(hsla(palette.badge_fg))
                .text_size(zpx(view_badge::TEXT_SIZE))
                .font_weight(gpui_kit::FontWeight::BOLD)
                .child(text)
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.show_sidebar_view(view, window, cx);
            }),
        );
    if active { button.bg(hsla(palette.current_bg)) } else { button.hover(|style| style.bg(hsla(colors().hover_bg))) }
}

/// 보기 줄. 왼쪽부터 Work·Source control 단추(변경 수 배지 포함)이고, 톱니를 누르면 빠른 설정 패널이 열리거나 닫힌다.
pub fn view_bar(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let current = window_view.scm.view;
    let badge = badge_text(window_view.git.n_changes());
    div()
        .flex_none()
        .h(zpx(size::HEIGHT))
        .px(zpx(size::PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::GAP))
        .border_b_1()
        .border_color(hsla(palette.border_subtle))
        .child(view_choice("view-work", current == SidebarView::Work, list_icon, None, SidebarView::Work, cx))
        .child(view_choice(
            "view-scm",
            current == SidebarView::SourceControl,
            branch_icon_16,
            badge,
            SidebarView::SourceControl,
            cx,
        ))
        .child(div().flex_1())
        .child(
            view_button()
                .id("quick-settings-button")
                .text_size(zpx(size::ICON_SIZE))
                .text_color(hsla(palette.fg_muted))
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg)))
                .child("\u{2699}")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.toggle_quick_settings(window, cx);
                    }),
                ),
        )
}
