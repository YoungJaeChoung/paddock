//! 본문 위 탭 줄: 사이드바 접기 단추, 선택한 터미널이 속한 묶음(작업 폴더 또는 Unassigned 묶음)의 탭, 파일 탭 열, 칸 나누기 단추 둘.
//!
//! 칸이 나뉘어 있으면 탭은 그 터미널이 든 칸 바로 위에 놓인다. 왼쪽 가장자리가 같은 칸들이 한 열이고, 열마다 탭 묶음이
//! 그 열의 가로 구간에 놓이며, 열의 맨 위 칸이 아닌 칸은 자기 위에 탭 줄을 따로 가진다(`pane_tab_row`). 탭이 열의 폭을
//! 넘치면 묶음이 가로로 스크롤되고, 가장자리 흐림·넘김 단추·가려진 쪽에 읽지 않은 완료가 있을 때의 점이 붙는다.
//! 묶음에 열린 파일이 있으면 파일 칸이 터미널 열들 오른쪽의 마지막 열이 되어 그 열에 파일 탭이 놓인다.
use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, ScrollHandle,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, div, linear_color_stop, linear_gradient, point, px,
};
use paddock_core::file_kind::{FileKind, file_name_of};
use paddock_core::layout_model::{Layout, Orientation};
use paddock_core::markdown_view::View;
use paddock_core::pane_columns::{Column, column_spans, columns_with_file, scroll_step_offset, tab_columns};
use paddock_core::theme::{
    account_picker, branch_button, external_editor_button, file_tab, markdown_view_actions, pdf_view, sidebar_toggle,
    split_buttons, tab, tab_overflow, text,
};

use crate::chrome::{unseen_dot, zpx};
use crate::look::{colors, scaled};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;
use crate::work::menu_model::{MenuKind, row_label};
use crate::workspace::TabRef;

/// 파일 칸의 탭 묶음이 쓰는 스크롤 상태 번호. 터미널 열의 번호(0부터)와 칸 위 탭 줄 번호(1000부터)와 겹치지 않는다.
const FILE_STRIP_KEY: usize = 2000;

/// 탭 줄이 쓸 본문의 크기(화면 크기 단위). 칸 위치와 탭 묶음의 가로 구간을 정하는 데 쓴다.
#[derive(Clone, Copy, Debug)]
pub struct StripGeometry {
    /// 터미널 배치가 차지하는 본문 폭(사이드바와 파일 칸을 뺀 것).
    pub width: f32,
    /// 탭 줄 아래 본문 높이.
    pub height: f32,
    /// 파일 칸이 차지하는 폭. 파일 칸이 없으면 0이다. 탭 줄은 `width + file_width` 전체에 걸쳐 놓인다.
    pub file_width: f32,
}

/// 탭 하나의 강조 상태.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TabLook {
    /// 포커스를 쥔 칸의 선택 탭.
    Focused,
    /// 포커스가 없는 칸의 선택 탭.
    Current,
    Inactive,
}

/// 탭에 보이는 이름. 작업 폴더 안에서는 사이드바 행과 같이 같은 이름에 `·2`가 붙는다.
fn tab_label(
    window_view: &WorkWindow,
    id: u32,
) -> String {
    let work = window_view.workspace.work();
    let name = window_view.workspace.name(id);
    let suffix = work
        .folder_of(id)
        .and_then(|folder| work.terminal_rows(folder).into_iter().find(|row| row.id == id))
        .map(|row| row.suffix)
        .unwrap_or_default();
    row_label(name, &suffix)
}

/// 탭 하나. 이름과 ×가 있고, 누르면 선택하거나(이름) 닫기 확인을 연다(×). `shrinks`면 줄 폭이 모자랄 때 최소 폭까지 줄어든다.
fn tab_item(
    window_view: &WorkWindow,
    id: u32,
    look: TabLook,
    shrinks: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let label = tab_label(window_view, id);
    let (background, foreground) = match look {
        TabLook::Focused => (palette.tab_active_bg, palette.tab_active_fg),
        TabLook::Current => (palette.tab_active_bg, palette.fg),
        TabLook::Inactive => (palette.tab_inactive_bg, palette.tab_inactive_fg),
    };
    let close = div()
        .flex_none()
        .w(zpx(tab::CLOSE_SIZE))
        .h(zpx(tab::CLOSE_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(tab::CLOSE_RADIUS))
        .text_size(zpx(tab::CLOSE_GLYPH))
        .text_color(hsla(palette.fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child("\u{2715}")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                // 탭 선택으로 번지지 않게 한다.
                cx.stop_propagation();
                this.request_close(id, window, cx);
            }),
        );
    div()
        .id(("tab", id))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .w(zpx(tab::WIDTH))
        .min_w(zpx(tab::MIN_WIDTH))
        .flex_shrink(if shrinks { 1.0 } else { 0.0 })
        .h(zpx(tab::HEIGHT))
        .pl(zpx(tab::TEXT_PADDING_LEFT))
        .pr(zpx(tab::PADDING_RIGHT))
        .rounded_t(zpx(tab::RADIUS_TOP))
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .cursor_pointer()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .child(div().min_w_0().truncate().child(label))
                .children(window_view.done.is_unseen(id).then(unseen_dot)),
        )
        .child(close)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.select_tab(id, window, cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.open_menu(MenuKind::Terminal(id), event.position, window, cx)
            }),
        )
}

/// 파일 탭 하나. 파일 그림과 이름이 있고, 누르면 선택하고 ×는 닫는다(확인 없이). 터미널 탭과 같은 폭·강조를 쓴다.
fn file_tab_item(
    index: usize,
    path: &str,
    look: TabLook,
    shrinks: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let (background, foreground) = match look {
        TabLook::Focused => (palette.tab_active_bg, palette.tab_active_fg),
        TabLook::Current => (palette.tab_active_bg, palette.fg),
        TabLook::Inactive => (palette.tab_inactive_bg, palette.tab_inactive_fg),
    };
    let close = div()
        .flex_none()
        .w(zpx(tab::CLOSE_SIZE))
        .h(zpx(tab::CLOSE_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(tab::CLOSE_RADIUS))
        .text_size(zpx(tab::CLOSE_GLYPH))
        .text_color(hsla(palette.fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child("\u{2715}")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                // 탭 선택으로 번지지 않게 한다.
                cx.stop_propagation();
                this.close_file_tab(index, window, cx);
            }),
        );
    let icon = div()
        .flex_none()
        .w(zpx(file_tab::ICON_WIDTH))
        .h(zpx(file_tab::ICON_HEIGHT))
        .border_1()
        .border_color(hsla(palette.fg_muted))
        .rounded(zpx(file_tab::ICON_RADIUS));
    div()
        .id(("file-tab", index))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .w(zpx(tab::WIDTH))
        .min_w(zpx(tab::MIN_WIDTH))
        .flex_shrink(if shrinks { 1.0 } else { 0.0 })
        .h(zpx(tab::HEIGHT))
        .pl(zpx(file_tab::PADDING_LEFT))
        .pr(zpx(tab::PADDING_RIGHT))
        .rounded_t(zpx(tab::RADIUS_TOP))
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .cursor_pointer()
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(zpx(file_tab::ICON_GAP))
                .child(icon)
                .child(div().min_w_0().truncate().child(file_name_of(path))),
        )
        .child(close)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.select_file_tab(index, window, cx)),
        )
}

/// 24×22 글자 단추(＋·‹·›). 누름 처리는 호출자가 붙인다.
fn glyph_button(
    id: impl Into<gpui_kit::ElementId>,
    glyph: &'static str,
    width: f32,
    height: f32,
    radius: f32,
) -> gpui_kit::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(zpx(width))
        .h(zpx(height))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(radius))
        .text_size(zpx(text::UI))
        .text_color(hsla(colors().fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child(glyph)
}

/// 새 터미널 ＋ 단추. 누르면 그 칸의 선택 탭을 고른 뒤 같은 자리에 새 터미널을 연다.
fn add_button(
    id: impl Into<gpui_kit::ElementId>,
    pane_current: Option<u32>,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    glyph_button(id, "+", tab::ADD_WIDTH, tab::ADD_HEIGHT, tab::ADD_RADIUS)
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if let Some(id) = pane_current {
                    this.select_tab(id, window, cx);
                }
                // 등록 계정이 있으면 일반/계정 터미널을 고르는 메뉴를, 없으면 바로 새 터미널을 연다.
                let place = this.workspace.new_place();
                this.open_new_terminal_menu(place, event.position, false, window, cx)
            }),
        )
}

/// 사이드바 접기 단추: 사이드바가 왼쪽에 있는 화면 모양 그림.
fn sidebar_toggle_button(cx: &mut Context<WorkWindow>) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let icon = div()
        .flex_none()
        .w(zpx(sidebar_toggle::ICON_WIDTH))
        .h(zpx(sidebar_toggle::ICON_HEIGHT))
        .flex()
        .flex_row()
        .rounded(zpx(sidebar_toggle::ICON_RADIUS))
        .border_1()
        .border_color(hsla(palette.fg_muted))
        .child(
            div()
                .flex_none()
                .w(zpx(sidebar_toggle::ICON_SIDEBAR_WIDTH))
                .h_full()
                .border_r_1()
                .border_color(hsla(palette.fg_muted)),
        );
    div()
        .id("sidebar-toggle")
        .flex_none()
        .mx(zpx(sidebar_toggle::MARGIN_X))
        .w(zpx(sidebar_toggle::WIDTH))
        .h(zpx(sidebar_toggle::HEIGHT))
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(sidebar_toggle::RADIUS))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(icon)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.toggle_sidebar(window, cx)),
        )
}

/// 계정 고르기: 계정 그림, 지금 터미널의 `도구 · 계정` 이름(없으면 `Accounts`), 아래 화살표. 누르면 같은 자리에 열 터미널을 고르는 메뉴가 열린다.
fn account_picker_button(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let person = div()
        .flex_none()
        .w(zpx(account_picker::ICON_SIZE))
        .h(zpx(account_picker::ICON_SIZE))
        .flex()
        .flex_col()
        .items_center()
        .gap(zpx(account_picker::ICON_GAP))
        .child(
            div()
                .w(zpx(account_picker::HEAD_SIZE))
                .h(zpx(account_picker::HEAD_SIZE))
                .rounded(zpx(account_picker::HEAD_SIZE / 2.0))
                .border_1()
                .border_color(hsla(palette.fg_muted)),
        )
        .child(
            div()
                .w(zpx(account_picker::BODY_WIDTH))
                .h(zpx(account_picker::BODY_HEIGHT))
                .rounded_t(zpx(account_picker::BODY_WIDTH / 2.0))
                .border_1()
                .border_color(hsla(palette.fg_muted)),
        );
    div()
        .id("account-picker")
        .flex_none()
        .max_w(zpx(account_picker::MAX_WIDTH))
        .h(zpx(account_picker::HEIGHT))
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .px(zpx(account_picker::PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(account_picker::GAP))
        .rounded(zpx(account_picker::RADIUS))
        .text_size(zpx(text::UI))
        .text_color(hsla(palette.fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg)))
        .child(person)
        .child(div().min_w_0().truncate().child(window_view.picker_label()))
        .child(div().flex_none().child("\u{25be}"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                let place = this.workspace.new_place();
                this.open_new_terminal_menu(place, event.position, true, window, cx)
            }),
        )
}

/// 파일 탭이 활성일 때만 보이는 `Open in external editor` 단추(↗). 누르면 그 파일을 바깥 편집기로 연다.
fn external_editor_action(cx: &mut Context<WorkWindow>) -> gpui_kit::Stateful<Div> {
    glyph_button(
        "external-editor",
        "\u{2197}",
        external_editor_button::WIDTH,
        external_editor_button::HEIGHT,
        external_editor_button::RADIUS,
    )
    .mb(zpx(tab::ADD_MARGIN_BOTTOM))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.open_active_file_externally(window, cx)),
    )
}

/// Markdown 탭이 활성일 때 보이는 `File`·`Preview`·`Both` 단추 묶음. 지금 보는 방식의 단추가 눌린 모양이다.
fn markdown_view_actions_row(
    current: View,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let mut row = div()
        .id("markdown-view-actions")
        .flex_none()
        .self_center()
        .flex()
        .flex_row()
        .gap(zpx(markdown_view_actions::GAP))
        .mr(zpx(markdown_view_actions::MARGIN_RIGHT))
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .p(zpx(markdown_view_actions::PADDING))
        .border_1()
        .border_color(hsla(palette.border_subtle))
        .rounded(zpx(markdown_view_actions::RADIUS));
    for (index, view) in View::ALL.into_iter().enumerate() {
        let pressed = view == current;
        let button = div()
            .id(("markdown-view", index))
            .flex_none()
            .h(zpx(markdown_view_actions::BUTTON_HEIGHT))
            .px(zpx(markdown_view_actions::BUTTON_PADDING_X))
            .flex()
            .items_center()
            .rounded(zpx(markdown_view_actions::BUTTON_RADIUS))
            .text_size(zpx(markdown_view_actions::BUTTON_TEXT_SIZE))
            .text_color(hsla(if pressed { palette.fg_strong } else { palette.fg_muted }))
            .cursor_pointer()
            .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
            .child(view.label())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.set_markdown_view(view, window, cx)),
            );
        row = row.child(if pressed { button.bg(hsla(palette.selected_bg)) } else { button });
    }
    row
}

/// PDF 탭이 활성일 때 보이는 `−`·`+` 배율 단추와 `Open with default app`.
fn pdf_actions_row(cx: &mut Context<WorkWindow>) -> gpui_kit::Stateful<Div> {
    let zoom_button = |id: &'static str, glyph: &'static str| {
        glyph_button(id, glyph, pdf_view::ZOOM_BUTTON_WIDTH, pdf_view::ZOOM_BUTTON_HEIGHT, pdf_view::ZOOM_BUTTON_RADIUS)
    };
    div()
        .id("pdf-actions")
        .flex_none()
        .flex()
        .flex_row()
        .items_end()
        .gap(zpx(split_buttons::GAP))
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .child(zoom_button("pdf-zoom-out", "\u{2212}").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.zoom_pdf(Some(false), window, cx)),
        ))
        .child(zoom_button("pdf-zoom-in", "+").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.zoom_pdf(Some(true), window, cx)),
        ))
        .child(
            div()
                .id("pdf-open-default")
                .flex_none()
                .h(zpx(pdf_view::OPEN_BUTTON_HEIGHT))
                .px(zpx(pdf_view::OPEN_BUTTON_PADDING_X))
                .flex()
                .items_center()
                .rounded(zpx(pdf_view::OPEN_BUTTON_RADIUS))
                .text_size(zpx(pdf_view::OPEN_BUTTON_TEXT_SIZE))
                .text_color(hsla(colors().fg_muted))
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
                .child("Open with default app")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.open_active_file_with_default_app(window, cx)
                    }),
                ),
        )
}

/// 지금 보는 폴더의 Git 브랜치 이름을 보이는 단추. 이름 표시만 하고 누르는 동작은 없다(체크아웃 메뉴 없음).
fn branch_label_button(label: String) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    div()
        .id("branch-button")
        .flex_none()
        .max_w(zpx(branch_button::MAX_WIDTH))
        .h(zpx(branch_button::HEIGHT))
        .px(zpx(branch_button::PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(branch_button::GAP))
        .rounded(zpx(branch_button::RADIUS))
        .text_size(zpx(branch_button::TEXT_SIZE))
        .text_color(hsla(palette.fg_muted))
        .child(crate::view_bar::branch_icon(palette.fg_muted, branch_button::ICON_SIZE))
        .child(div().min_w_0().truncate().child(label))
}

/// 칸 나누기 단추: 한 칸을 나눈 모양 그림. 가로면 옆 칸, 세로면 아래 칸을 연다.
fn split_button(
    id: &'static str,
    orientation: Orientation,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let divider = div().flex_none().bg(hsla(palette.fg_muted));
    let icon = div()
        .flex_none()
        .w(zpx(split_buttons::ICON_SIZE))
        .h(zpx(split_buttons::ICON_SIZE))
        .flex()
        .rounded(zpx(split_buttons::ICON_RADIUS))
        .border_1()
        .border_color(hsla(palette.fg_muted));
    let icon = match orientation {
        Orientation::Horizontal => icon
            .flex_row()
            .child(div().flex_1().h_full())
            .child(divider.w(px(split_buttons::DIVIDER_WIDTH)).h_full())
            .child(div().flex_1().h_full()),
        Orientation::Vertical => icon
            .flex_col()
            .child(div().flex_1().w_full())
            .child(divider.h(px(split_buttons::DIVIDER_WIDTH)).w_full())
            .child(div().flex_1().w_full()),
    };
    div()
        .id(id)
        .flex_none()
        .w(zpx(split_buttons::WIDTH))
        .h(zpx(split_buttons::HEIGHT))
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(split_buttons::RADIUS))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(icon)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.split_selected(orientation, window, cx)),
        )
}

/// 탭 묶음 하나(열 하나, 또는 한 칸 위의 탭 줄)에서 스크롤 상태가 알려 주는 넘침 모양.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverflowLook {
    /// 왼쪽에 가려진 탭이 있다.
    pub hidden_left: bool,
    /// 오른쪽에 가려진 탭이 있다.
    pub hidden_right: bool,
}

/// 스크롤 상자의 위치로 넘침 모양을 읽는다. 아직 배치되지 않은 상자(크기 0)는 넘치지 않은 것으로 본다.
pub fn overflow_look(handle: &ScrollHandle) -> OverflowLook {
    let max = f32::from(handle.max_offset().x);
    let offset = f32::from(handle.offset().x);
    OverflowLook { hidden_left: max > 0.5 && offset < -0.5, hidden_right: max > 0.5 && max + offset > 0.5 }
}

/// 가려진 쪽 탭 중 읽지 않은 완료가 있는 탭이 있는지. `tabs`는 상자 안 탭 순서다(파일 탭에는 완료 알림이 없다).
fn hidden_has_unseen(
    window_view: &WorkWindow,
    handle: &ScrollHandle,
    tabs: &[TabRef],
    right: bool,
) -> bool {
    let viewport = handle.bounds();
    tabs.iter().enumerate().any(|(index, tab)| {
        matches!(tab, TabRef::Terminal(id) if window_view.done.is_unseen(*id))
            && handle
                .bounds_for_item(index)
                .is_some_and(|item| if right { item.right() > viewport.right() } else { item.left() < viewport.left() })
    })
}

/// 가로로 스크롤되는 탭 묶음과 그 끝의 흐림·넘김 단추. `tabs`는 (탭, 강조) 순서다.
fn scrolling_tabs(
    window_view: &WorkWindow,
    key: usize,
    handle: &ScrollHandle,
    tabs: &[(TabRef, TabLook)],
    shrinks: bool,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let look = overflow_look(handle);
    let ids: Vec<TabRef> = tabs.iter().map(|(tab, _)| *tab).collect();
    // 키보드로 탭을 옮긴 직후에는 활성 탭이 가려져 있어도 보이는 곳으로 스크롤한다. 그 밖의 다시 그리기는 스크롤을 건드리지 않는다.
    if window_view.reveal_pending()
        && let Some(index) = tabs.iter().position(|(tab, _)| window_view.is_active_tab(*tab))
    {
        handle.scroll_to_item(index);
    }
    let mut list = div()
        .id(("tab-list", key))
        .flex()
        .flex_row()
        .items_end()
        .gap(zpx(tab::GAP))
        .min_w_0()
        .overflow_x_scroll()
        .on_scroll_wheel(cx.listener(|this, _event: &ScrollWheelEvent, _window, _cx| this.cancel_reveal()))
        .track_scroll(handle);
    for (tab, tab_look) in tabs {
        list = match tab {
            TabRef::Terminal(id) => list.child(tab_item(window_view, *id, *tab_look, shrinks, cx)),
            TabRef::File(index) => {
                let path = window_view
                    .workspace
                    .file_pane()
                    .and_then(|pane| pane.tabs.get(*index))
                    .map(|tab| tab.path.as_str());
                list.child(file_tab_item(*index, path.unwrap_or(""), *tab_look, shrinks, cx))
            }
        };
    }
    let solid = hsla(palette.tab_bar_bg);
    let clear = solid.opacity(0.0);
    let fade = |at_right: bool| {
        let (from, to) = if at_right { (clear, solid) } else { (solid, clear) };
        let edge = div().absolute().top_0().bottom_0().w(zpx(tab_overflow::FADE_WIDTH)).bg(linear_gradient(
            90.0,
            linear_color_stop(from, 0.0),
            linear_color_stop(to, 1.0),
        ));
        if at_right { edge.right_0() } else { edge.left_0() }
    };
    let scroller = |at_right: bool, id: &'static str, glyph: &'static str| {
        let handle = handle.clone();
        let marked = hidden_has_unseen(window_view, &handle, &ids, at_right);
        let button = glyph_button(
            (id, key),
            glyph,
            tab_overflow::BUTTON_SIZE,
            tab_overflow::BUTTON_SIZE,
            tab_overflow::BUTTON_RADIUS,
        )
        .relative()
        .mb(zpx(tab::ADD_MARGIN_BOTTOM))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                this.scroll_tabs(&handle, at_right);
                cx.notify();
            }),
        );
        button.children(marked.then(|| {
            div()
                .absolute()
                .top(zpx(tab_overflow::MARK_OFFSET))
                .right(zpx(tab_overflow::MARK_OFFSET))
                .w(zpx(tab_overflow::MARK_SIZE))
                .h(zpx(tab_overflow::MARK_SIZE))
                .rounded(zpx(tab_overflow::MARK_SIZE / 2.0))
                .bg(hsla(colors().accent))
        }))
    };
    div()
        .flex()
        .flex_row()
        .items_end()
        .min_w_0()
        .children(look.hidden_left.then(|| scroller(false, "tab-scroll-left", "\u{2039}")))
        .child(
            div()
                .relative()
                .flex()
                .flex_row()
                .min_w_0()
                .child(list)
                .children(look.hidden_left.then(|| fade(false)))
                .children(look.hidden_right.then(|| fade(true))),
        )
        .children(look.hidden_right.then(|| scroller(true, "tab-scroll-right", "\u{203a}")))
}

/// 칸에서 선택된 터미널이 `focused`인지에 따라 칸의 탭들의 강조를 정한다.
fn pane_tab_looks(
    tabs: &[u32],
    current: u32,
    focused: Option<u32>,
) -> Vec<(TabRef, TabLook)> {
    tabs.iter()
        .map(|id| {
            let look = if *id != current {
                TabLook::Inactive
            } else if focused == Some(*id) {
                TabLook::Focused
            } else {
                TabLook::Current
            };
            (TabRef::Terminal(*id), look)
        })
        .collect()
}

/// 파일 칸의 탭들의 강조: 선택한 탭이 활성이면 `Focused`, 다른 칸이 활성이면 `Current`.
fn file_tab_looks(
    n_tabs: usize,
    selected: usize,
    active: bool,
) -> Vec<(TabRef, TabLook)> {
    (0..n_tabs)
        .map(|index| {
            let look = match (index == selected, active) {
                (false, _) => TabLook::Inactive,
                (true, true) => TabLook::Focused,
                (true, false) => TabLook::Current,
            };
            (TabRef::File(index), look)
        })
        .collect()
}

/// 칸의 선택된 탭 번호(칸이 비어 있으면 None).
fn current_of(
    tabs: &[u32],
    selected: usize,
) -> Option<u32> {
    tabs.get(selected.min(tabs.len().saturating_sub(1))).copied()
}

/// 위아래로 쌓인 열에서 맨 위가 아닌 칸이 자기 위에 두는 탭 줄. 칸의 탭과 ＋만 있다.
pub fn pane_tab_row(
    window_view: &WorkWindow,
    key: usize,
    tabs: &[u32],
    selected: usize,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let current = current_of(tabs, selected);
    let looks = pane_tab_looks(tabs, current.unwrap_or(0), window_view.focused_terminal());
    let handle = window_view.tab_scroll(key);
    div()
        .flex()
        .flex_row()
        .items_end()
        .flex_none()
        .h(zpx(tab::BAR_HEIGHT))
        .w_full()
        .pl(zpx(tab::BAR_PADDING_LEFT))
        .gap(zpx(tab::GAP))
        .border_b_1()
        .border_color(hsla(palette.border))
        .bg(hsla(palette.tab_bar_bg))
        .text_size(zpx(text::SMALL))
        .child(scrolling_tabs(window_view, key, &handle, &looks, true, cx))
        .child(add_button(("pane-add", key), current, cx))
}

/// 탭 줄을 만든다. 맨 왼쪽은 사이드바 접기 단추이고, 탭은 칸 열마다 그 열의 구간에 놓이며, 맨 오른쪽은 칸 나누기 단추 둘이다.
pub fn tab_bar(
    window_view: &WorkWindow,
    geometry: StripGeometry,
    layout: Option<&Layout>,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let focused = window_view.focused_terminal();
    let file_pane = window_view.workspace.file_pane();
    let file_active = window_view.file_active();
    let strip_left = scaled(tab::BAR_PADDING_LEFT + sidebar_toggle::WIDTH + 2.0 * sidebar_toggle::MARGIN_X);
    // 계정 고르기는 이름 길이에 따라 폭이 달라 최대 폭만큼 비워 두고, 그 안에서 줄어든다.
    let editor_button_width = if file_active { external_editor_button::WIDTH + split_buttons::GAP } else { 0.0 };
    let selected_kind = if file_active { window_view.selected_file_kind() } else { None };
    let view_actions_width = match selected_kind {
        Some(FileKind::Markdown) => markdown_view_actions::RESERVED_WIDTH + split_buttons::GAP,
        Some(FileKind::Pdf) => pdf_view::ACTIONS_RESERVED_WIDTH + split_buttons::GAP,
        _ => 0.0,
    };
    let branch = window_view.git.branch_label();
    let branch_width = if branch.is_some() { branch_button::MAX_WIDTH + split_buttons::GAP } else { 0.0 };
    let right_width = scaled(
        account_picker::MAX_WIDTH
            + split_buttons::GAP
            + 2.0 * split_buttons::WIDTH
            + split_buttons::GAP
            + split_buttons::PADDING_RIGHT
            + editor_button_width
            + view_actions_width
            + branch_width,
    );
    let strip_width = (geometry.width + geometry.file_width - strip_left - right_width).max(0.0);
    let mut strip = div().relative().flex_1().h_full().min_w_0().overflow_hidden();
    if let Some(layout) = layout {
        let boxes = layout.pane_boxes(geometry.width, geometry.height);
        let columns = tab_columns(layout, geometry.width, geometry.height);
        let generic: Vec<Column<usize>> =
            columns.iter().map(|column| Column { left: column.left, keys: column.panes.clone() }).collect();
        // 파일 칸이 있으면 터미널 배치 오른쪽의 마지막 열이다. 터미널 열들은 파일 칸이 시작하는 곳에서 끝난다.
        let all = match file_pane {
            Some(_) => columns_with_file(&generic, usize::MAX, geometry.width),
            None => generic,
        };
        let spans = column_spans(&all, strip_left, strip_width);
        let shrinks = columns.len() > 1 || file_pane.is_some();
        if let (Some(pane), Some(span)) = (file_pane, spans.get(columns.len())) {
            let looks = file_tab_looks(pane.tabs.len(), pane.selected, file_active);
            let handle = window_view.tab_scroll(FILE_STRIP_KEY);
            strip = strip.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(span.left))
                    .w(px(span.width))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(zpx(tab::GAP))
                    .child(scrolling_tabs(window_view, FILE_STRIP_KEY, &handle, &looks, true, cx)),
            );
        }
        for (index, (column, span)) in columns.iter().zip(spans).enumerate() {
            let Some(top) = column.panes.first().and_then(|pane| boxes.get(*pane)) else { continue };
            let current = current_of(&top.tabs, top.selected);
            let looks = pane_tab_looks(&top.tabs, current.unwrap_or(0), focused);
            let handle = window_view.tab_scroll(index);
            strip = strip.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(span.left))
                    .w(px(span.width))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(zpx(tab::GAP))
                    .child(scrolling_tabs(window_view, index, &handle, &looks, shrinks, cx))
                    .child(add_button(("tab-add", index), current, cx)),
            );
        }
    } else {
        strip =
            strip.child(div().absolute().top_0().bottom_0().left_0().child(add_button(("tab-add", 0_usize), None, cx)));
    }
    div()
        .flex()
        .flex_row()
        .items_end()
        .flex_none()
        .h(zpx(tab::BAR_HEIGHT))
        .w_full()
        .pl(zpx(tab::BAR_PADDING_LEFT))
        .border_b_1()
        .border_color(hsla(palette.border))
        .bg(hsla(palette.tab_bar_bg))
        .text_size(zpx(text::SMALL))
        .child(sidebar_toggle_button(cx))
        .child(strip)
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_end()
                .gap(zpx(split_buttons::GAP))
                .pr(zpx(split_buttons::PADDING_RIGHT))
                .children(
                    window_view
                        .selected_markdown_view()
                        .filter(|_| file_active)
                        .map(|view| markdown_view_actions_row(view, cx)),
                )
                .children((selected_kind == Some(FileKind::Pdf)).then(|| pdf_actions_row(cx)))
                .children(file_active.then(|| external_editor_action(cx)))
                .children(branch.map(branch_label_button))
                .child(account_picker_button(window_view, cx))
                .child(split_button("split-right", Orientation::Horizontal, cx))
                .child(split_button("split-down", Orientation::Vertical, cx)),
        )
}

/// 스크롤 상자를 `toward_right` 쪽으로 한 걸음 옮긴다.
pub fn scroll_by_step(
    handle: &ScrollHandle,
    toward_right: bool,
) {
    let step = scaled(tab_overflow::SCROLL_STEP);
    let x = scroll_step_offset(f32::from(handle.offset().x), f32::from(handle.max_offset().x), step, toward_right);
    handle.set_offset(point(px(x), px(0.0)));
}
