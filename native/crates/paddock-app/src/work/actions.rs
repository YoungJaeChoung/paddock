//! Work 목록·탭 줄·메뉴의 동기화 동작: 행 더블클릭 이름 바꾸기, 행 끌어 순서 바꾸기, `Show to the Side`,
//! 메뉴의 ↑·↓·Home·End·Enter, 탭 줄 초점의 ←·→·Home·End.
//!
//! 규칙(어느 키가 무엇을 옮기는지, 어느 행에 놓을 수 있는지)은 core와 `menu_model`이 정하고, 여기서는 창 상태에 반영하기만 한다.
use gpui_kit::{Context, Window};
use paddock_core::menu_nav::NavKey;
use paddock_core::tab_focus::tab_bar_key;
use paddock_core::work_model::DropPosition;

use super::menu_model::{MenuItem, MenuKind, RowClick, row_click};
use super::terminal_row::RowTarget;
use crate::shell_choice::available_environments;
use crate::window_view::{Active, RenameTarget, WorkWindow};

impl WorkWindow {
    /// 메뉴 `kind`의 항목들. `Show to the Side`는 그 터미널이 지금 보는 묶음의 터미널이고 기준이 될 다른 칸이 있을 때만 들어간다.
    pub(crate) fn menu_entries(
        &self,
        kind: &MenuKind,
    ) -> Vec<MenuItem> {
        let work = self.workspace.work();
        let movable = match kind {
            MenuKind::Terminal(id) => Some(*id),
            MenuKind::Group(root) => work.groups.iter().find(|group| group.root == *root).map(|group| group.last_used),
            _ => None,
        };
        // 다른 창에 있는 터미널은 이 창의 선택한 터미널 칸 옆으로 돌아올 수 있을 때 `Show to the Side`가 있다.
        let beside = movable.filter(|id| {
            if self.workspace.is_detached(*id) {
                self.beside_reference_for_detached(*id).is_some()
            } else {
                self.workspace.beside_reference(*id).is_some()
            }
        });
        let items = super::menu_model::menu_items(kind, &available_environments(), &self.registered_accounts(), beside);
        super::menu_model::with_window_move(items, self.window_move_action(kind))
    }

    /// `Show to the Side`: 터미널을 새로 만들지 않고 옆 칸으로 옮겨 선택한다. 실행 중인 프로그램과 출력은 그대로다.
    pub(crate) fn show_beside_terminal(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.show_beside(id) {
            self.set_active(Active::Terminal);
            self.done.acknowledge(id);
            self.reveal_selected.set(true);
            self.focus_selected(window, cx);
            self.persist();
            cx.notify();
        }
    }

    /// 사이드바 행·탭을 왼쪽으로 눌렀을 때: 선택하고, 더블클릭이면 이름 바꾸기 창을 연다.
    pub(crate) fn click_row(
        &mut self,
        target: RowTarget,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown_elsewhere = match target {
            RowTarget::Terminal(id) => Some(id),
            RowTarget::Group(root) => {
                self.workspace.work().groups.iter().find(|group| group.root == root).map(|group| group.last_used)
            }
        }
        .is_some_and(|id| self.workspace.is_detached(id));
        let renaming = row_click(click_count) == RowClick::SelectAndRename;
        if shown_elsewhere && renaming {
            // 다른 창에 있는 터미널의 이름 바꾸기: 입력칸은 이 창에 열리므로 분리 창이 아니라 이 창을 앞으로 가져온다.
            self.activate_main(window);
        } else {
            self.select_row(target, window, cx);
        }
        if renaming {
            let id = match target {
                RowTarget::Terminal(id) => id,
                RowTarget::Group(root) => root,
            };
            self.begin_rename(RenameTarget::Terminal(id), window, cx);
        }
    }

    /// `id` 행 위에 끌어 온 `dragged` 행이 지나가는 중이다. `position`은 마우스가 이 행 안에 있을 때의 놓을 자리(밖이면 None)이고,
    /// 같은 폴더의 다른 터미널일 때만 표식이 생긴다.
    pub(crate) fn hover_dragged_row(
        &mut self,
        dragged: u32,
        id: u32,
        position: Option<DropPosition>,
        cx: &mut Context<Self>,
    ) {
        let allowed = self.workspace.work().can_reorder(dragged, id);
        let next = match position {
            Some(position) if allowed => Some((id, position)),
            _ if self.row_drop.is_some_and(|(over, _)| over == id) => None,
            _ => self.row_drop,
        };
        if next != self.row_drop {
            self.row_drop = next;
            cx.notify();
        }
    }

    /// 끌고 있는 중이면 `id` 행에 붙일 놓을 자리 표식.
    pub(crate) fn row_drop_marker(
        &self,
        id: u32,
        cx: &Context<Self>,
    ) -> Option<DropPosition> {
        self.row_drop.filter(|(over, _)| *over == id && cx.has_active_drag()).map(|(_, position)| position)
    }

    /// 끌어 온 행 `moved`를 `target` 행에 놓았다. 표식이 가리키던 자리(없으면 행 앞)에 넣고, 다른 폴더의 행이면 아무것도 바꾸지 않는다.
    pub(crate) fn drop_row(
        &mut self,
        moved: u32,
        target: u32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = self
            .row_drop
            .take()
            .filter(|(over, _)| *over == target)
            .map_or(DropPosition::Before, |(_, position)| position);
        self.reorder_terminal(moved, target, position, cx);
    }

    /// 터미널 행 `moved`를 같은 폴더의 `target` 행 앞이나 뒤로 옮기고 저장한다. 옮길 수 없는 쌍이면 false다.
    pub(crate) fn reorder_terminal(
        &mut self,
        moved: u32,
        target: u32,
        position: DropPosition,
        cx: &mut Context<Self>,
    ) -> bool {
        let allowed = self.workspace.work().can_reorder(moved, target);
        if allowed {
            self.workspace.work_mut().move_terminal(moved, target, position == DropPosition::After);
            self.persist();
        }
        self.row_drop = None;
        cx.notify();
        allowed
    }

    /// 메뉴가 열려 있을 때 받은 키 하나: Esc는 닫고, ↑·↓·Home·End는 강조를 옮기고, Enter는 강조 항목을 실행한다. 그 밖의 키는 무시한다.
    pub(crate) fn menu_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.menu.as_ref().map(|menu| self.menu_entries(&menu.kind)).unwrap_or_default();
        if key == "escape" {
            self.close_menu(window, cx);
        } else if let Some(nav_key) = NavKey::from_name(key) {
            if let Some(menu) = self.menu.as_mut() {
                menu.nav.step(nav_key, entries.len());
                cx.notify();
            }
        } else if key == "enter" {
            let chosen = self.menu.as_ref().and_then(|menu| menu.nav.chosen(entries.len()));
            if let Some(action) = chosen.and_then(|index| entries.get(index)).map(|item| item.action.clone()) {
                self.run_menu_action(action, window, cx);
            }
        }
    }

    /// 마우스가 메뉴의 `index`번째 항목 위로 왔다: 강조를 그리로 옮긴다.
    pub(crate) fn menu_hover(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.menu.as_mut()
            && menu.nav.highlight != Some(index)
        {
            menu.nav.hover(index);
            cx.notify();
        }
    }

    /// 탭 줄 빈 자리를 눌렀다: 탭 줄에 초점이 생긴다.
    pub(crate) fn focus_tab_bar(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if !self.tab_bar_focused {
            self.tab_bar_focused = true;
            cx.notify();
        }
    }

    /// 탭 줄 초점을 거둔다.
    pub(crate) fn leave_tab_bar(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.tab_bar_focused {
            self.tab_bar_focused = false;
            cx.notify();
        }
    }

    /// 탭 줄 바탕색. 초점이 있으면 호버 바탕이다.
    pub(crate) fn tab_bar_background(&self) -> u32 {
        if self.tab_bar_focused { crate::look::colors().hover_bg } else { crate::look::colors().tab_bar_bg }
    }

    /// 탭 줄에 초점이 있을 때 받은 키 하나. ←·→·Home·End는 탭을 고르고, Enter·Esc는 초점을 거두며 소비되고,
    /// 그 밖의 키는 초점을 거두고 터미널로 흘려보낸다. 키를 소비했으면 true다(탭 줄에 초점이 없어도 false).
    pub(crate) fn tab_bar_key_pressed(
        &mut self,
        key: &str,
        control: bool,
        alt: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let focused = self.tab_bar_focused;
        match tab_bar_key(key, control, alt) {
            Some(tab_key) if focused => {
                self.step_tab(tab_key.target(), window, cx);
                true
            }
            _ => {
                self.leave_tab_bar(cx);
                focused && matches!(key, "enter" | "escape")
            }
        }
    }
}
