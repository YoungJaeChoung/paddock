//! Work 목록 메뉴를 그린다: 창 전체를 덮는 투명 막(바깥을 누르면 닫힘)과 항목 판. 항목 구성은 `menu_model`이 정한다.
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
    Styled, div, px,
};
use paddock_core::theme::menu as size;
use paddock_core::theme::text;

use super::menu_model::{ContextMenu, MenuKind, folder_note, switch_hint};
use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 열린 메뉴를 그린다: 창 전체를 덮는 투명 막(바깥을 누르면 닫힘)과 항목 판.
pub fn menu_panel(
    window_view: &WorkWindow,
    menu: &ContextMenu,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let items = window_view.menu_entries(&menu.kind);
    let min_width = if matches!(menu.kind, MenuKind::NewTerminal(_) | MenuKind::AccountSwitch(_)) {
        size::ACCOUNT_WIDTH
    } else {
        size::MIN_WIDTH
    };
    // 계정 고르기 메뉴는 맨 위에 이 메뉴가 하는 일을 한 줄로 적는다(폴더 메뉴의 안내는 아래에 있다).
    let header = match &menu.kind {
        MenuKind::AccountSwitch(switch) => Some(switch_hint(switch, &window_view.registered_accounts())),
        _ => None,
    };
    let note = match &menu.kind {
        MenuKind::Folder(key) => Some(folder_note(window_view.workspace.work().terminals_of(key).len())),
        _ => None,
    };
    let mut panel = div()
        .id("work-menu")
        .absolute()
        .left(px(menu.x))
        .top(px(menu.y))
        .min_w(zpx(min_width))
        .p(zpx(size::PADDING))
        .flex()
        .flex_col()
        .rounded(zpx(size::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg))
        .text_size(zpx(size::FONT_SIZE))
        .shadow_lg()
        .occlude();
    if let Some(header) = header {
        panel = panel.child(
            div()
                .pb(zpx(size::NOTE_PADDING_BOTTOM))
                .px(zpx(size::NOTE_PADDING_X))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(header),
        );
    }
    for (index, menu_item) in items.into_iter().enumerate() {
        let action = menu_item.action.clone();
        let color = if menu_item.danger { palette.danger } else { palette.fg_strong };
        let highlighted = menu.nav.highlight == Some(index);
        let mut row = div().id(("work-menu-item", index));
        if highlighted {
            row = row.bg(hsla(colors().selected_bg));
        }
        if menu_item.separated {
            row = row.mt(zpx(size::SEPARATOR_MARGIN_TOP)).border_t_1().border_color(hsla(palette.border));
        }
        panel = panel.child(
            row.flex()
                .flex_row()
                .items_center()
                .gap(zpx(size::ITEM_GAP))
                .px(zpx(size::ITEM_PADDING_X))
                .py(zpx(size::ITEM_PADDING_Y))
                .rounded(zpx(size::ITEM_RADIUS))
                .text_color(hsla(color))
                .cursor_pointer()
                .on_mouse_move(
                    cx.listener(move |this, _event: &MouseMoveEvent, _window, cx| this.menu_hover(index, cx)),
                )
                .child(div().flex_1().min_w_0().truncate().child(menu_item.label))
                .children((!menu_item.meta.is_empty()).then(|| {
                    div()
                        .flex_none()
                        .text_size(zpx(size::ITEM_META_SIZE))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(hsla(colors().fg_muted))
                        .child(menu_item.meta)
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.run_menu_action(action.clone(), window, cx);
                    }),
                ),
        );
    }
    if let Some(note) = note {
        panel = panel.child(
            div()
                .pt(zpx(size::NOTE_PADDING_TOP))
                .pb(zpx(size::NOTE_PADDING_BOTTOM))
                .px(zpx(size::NOTE_PADDING_X))
                .text_size(zpx(text::SMALL))
                .line_height(gpui_kit::relative(size::NOTE_LINE_HEIGHT))
                .text_color(hsla(palette.fg_muted))
                .child(note),
        );
    }
    div()
        .id("work-menu-scrim")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_menu(window, cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_menu(window, cx)),
        )
        .child(panel)
}
