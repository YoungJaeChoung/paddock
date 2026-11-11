//! Work 목록(사이드바): 머리줄(＋ 새 터미널, ▾ 실행 환경 고르기), 작업 폴더 구획(폴더 행·그 아래 터미널 행·`Show N more`),
//! 작업 폴더가 없을 때의 첫 실행 안내, Unassigned 묶음 구획.
//!
//! 행의 이름·프로그램 요약·상태·미확인 점은 에이전트 상태 규칙(`agent_view`)을 그대로 쓴다. 이 파일은 순서와 배치만 정한다.
use std::sync::atomic::Ordering;

use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use paddock_core::theme::{folder_row, more_row as more_size, row_remote_meta, sidebar, text};

use paddock_core::work_model::{Group, Row, folder_name};
use paddock_protocol::message::TerminalState;

use super::empty_state::empty_state;
use super::folder_row::{FolderRowSpec, folder_row};
use super::menu_model::{MenuKind, more_label, row_label};
use super::repo_row::{RepoRowSpec, repo_row};
use super::terminal_row::{RowTarget, TerminalRowSpec, terminal_row};
use crate::agent_view::{meta_text, need_input_text, needs_input, status_of};
use crate::chrome::zpx;
use crate::look::colors;
use crate::scm::SidebarView;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 이 사용자의 홈 폴더 경로. 폴더 행이 홈을 `~`로 보이는 데 쓴다.
fn home_dir() -> Option<String> {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()
}

/// 머리줄의 24×24 단추. 누름 처리는 호출자가 붙인다.
fn header_button(
    id: &'static str,
    glyph: &'static str,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .w(zpx(sidebar::ACTION_SIZE))
        .h(zpx(sidebar::ACTION_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(sidebar::ACTION_RADIUS))
        .text_size(zpx(text::UI))
        .text_color(hsla(colors().fg_strong))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(glyph)
}

/// 터미널 번호의 지금 상태(데몬이 알린 값). 이 창에 그 터미널의 탭이 없으면 None이다.
pub(crate) fn terminal_state(
    window_view: &WorkWindow,
    id: u32,
) -> Option<TerminalState> {
    window_view.workspace.tab(id).and_then(|tab| tab.content.session.grid.shared.state.lock().unwrap().clone())
}

/// 작업 폴더 행의 도움말: 폴더 경로, 읽지 않은 완료 알림, 입력을 기다리는 에이전트 수(`N need input`)를 ` · `로 잇는다.
/// 접힌 폴더 안의 터미널 상태도 이 도움말이 알린다.
pub(crate) fn folder_tip(
    window_view: &WorkWindow,
    key: &str,
) -> String {
    let ids = window_view.workspace.work().terminals_of(key);
    let n_waiting = ids.iter().filter(|id| needs_input(terminal_state(window_view, **id).as_ref())).count();
    let unseen = ids.iter().any(|id| window_view.done.is_unseen(*id));
    [Some(key.to_string()), unseen.then(|| "Unseen activity".to_string()), need_input_text(n_waiting)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// 저장소 묶음 행의 도움말: 저장소 경로, 묶음 안 폴더의 읽지 않은 완료 알림, 입력을 기다리는 에이전트 수를 ` · `로 잇는다.
/// 접힌 묶음 안의 터미널 상태도 이 도움말이 알린다.
pub(crate) fn repo_tip(
    window_view: &WorkWindow,
    key: &str,
) -> String {
    let work = window_view.workspace.work();
    let ids: Vec<u32> = work.repo_folders(key).into_iter().flat_map(|folder| work.terminals_of(folder)).collect();
    let n_waiting = ids.iter().filter(|id| needs_input(terminal_state(window_view, **id).as_ref())).count();
    let unseen = ids.iter().any(|id| window_view.done.is_unseen(*id));
    [Some(key.to_string()), unseen.then(|| "Unseen activity".to_string()), need_input_text(n_waiting)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// Unassigned 묶음 행의 도움말: 앞쪽 프로그램·폴더, 읽지 않은 완료 알림, 입력을 기다리는 에이전트 수를 ` · `로 잇고 이름 바꾸기 방법을 붙인다.
pub(crate) fn group_tip(
    window_view: &WorkWindow,
    group: &Group,
) -> String {
    let state = terminal_state(window_view, group.root);
    let n_waiting =
        group.members.iter().filter(|member| needs_input(terminal_state(window_view, member.id).as_ref())).count();
    let unseen = group.members.iter().any(|member| window_view.done.is_unseen(member.id));
    let parts = [
        state.as_ref().and_then(|state| state.program.clone()),
        state.as_ref().and_then(|state| state.cwd.clone()),
        unseen.then(|| "Unseen activity in this group".to_string()),
        need_input_text(n_waiting),
    ];
    format!("{} \u{2014} double-click to rename", parts.into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} "))
}

/// 터미널 하나의 행 모양(요약·상태·미확인)을 정하는 값.
struct RowLook {
    exited: bool,
    meta: String,
    /// 터미널이 다른 창에 있어 `meta`가 `other window` 표지다.
    remote: bool,
    status: Option<crate::agent_view::Status>,
}

/// `id`는 요약·상태를 읽을 터미널이고, `shown_by`는 행을 누르면 열리는 터미널이다(터미널 행은 둘이 같고 묶음 행은 마지막으로 쓴 터미널).
/// `shown_by`가 다른 창에 있으면 행은 `other window` 표지를 보인다.
fn row_look(
    window_view: &WorkWindow,
    id: u32,
    shown_by: u32,
    name: &str,
) -> RowLook {
    match window_view.workspace.tab(id) {
        Some(tab) => {
            let shared = &tab.content.session.grid.shared;
            let state = terminal_state(window_view, id);
            let remote = window_view.workspace.is_detached(shown_by);
            RowLook {
                exited: shared.exited.load(Ordering::Relaxed),
                meta: if remote { row_remote_meta::TEXT.to_string() } else { meta_text(name, state.as_ref()) },
                remote,
                status: status_of(state.as_ref()),
            }
        }
        None => RowLook { exited: false, meta: String::new(), remote: false, status: None },
    }
}

/// 사이드바의 바깥 틀: 폭·바탕·오른쪽 선·글자색. 안은 호출자가 채운다.
fn sidebar_frame() -> gpui_kit::Div {
    let palette = colors();
    div()
        .flex_none()
        .flex()
        .flex_col()
        .w(zpx(sidebar::WIDTH))
        .h_full()
        .bg(hsla(palette.sidebar_bg))
        .border_r_1()
        .border_color(hsla(palette.sidebar_border))
        .text_color(hsla(palette.fg))
}

/// 사이드바를 만든다. Source control 보기가 켜져 있으면 보기 줄 아래를 그 보기가 채우고, 아니면 Work 목록과 파일 목록이다.
pub fn work_list(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    match window_view.scm.view {
        SidebarView::SourceControl => sidebar_frame()
            .child(crate::view_bar::view_bar(window_view, cx))
            .child(crate::scm::view::scm_content(window_view, cx)),
        SidebarView::Work => work_sidebar(window_view, cx),
    }
}

/// Work 보기: 머리줄·작업 폴더 목록·그 아래 파일 목록.
fn work_sidebar(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Div {
    let palette = colors();
    let work = window_view.workspace.work();
    let selected = window_view.workspace.selected();
    let header = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(zpx(sidebar::HEADER_HEIGHT))
        .pl(zpx(sidebar::HEADER_PADDING_LEFT))
        .pr(zpx(sidebar::HEADER_PADDING_RIGHT))
        .gap(zpx(sidebar::HEADER_GAP))
        .border_b_1()
        .border_color(hsla(palette.border_subtle))
        .child(
            div()
                .text_size(zpx(text::SMALL))
                .font_weight(FontWeight::BOLD)
                .text_color(hsla(palette.fg_muted))
                .child("WORK"),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .gap(zpx(sidebar::ACTIONS_GAP))
                .child(header_button("work-add", "+").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                        this.new_terminal_in_new_group(None, window, cx)
                    }),
                ))
                .child(header_button("work-environments", "\u{25be}").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        this.open_menu(MenuKind::Environments, event.position, window, cx)
                    }),
                )),
        );

    let mut list = div().id("work-list").flex().flex_col().py(zpx(sidebar::LIST_PADDING_Y)).overflow_y_scroll();
    if work.folders.is_empty() {
        list = list.child(empty_state(!work.groups.is_empty()));
    }
    let home = home_dir();
    let mut folder_index = 0;
    let mut repo_index = 0;
    // 지금 그리는 폴더가 저장소 묶음 행 아래의 구성원이다. 그 폴더의 터미널·`Show N more` 행도 같이 들여 쓴다.
    let mut nested = false;
    for row in work.visible_rows(selected, &window_view.show_all) {
        match row {
            Row::Repo { key, name, expanded, n_folders, current } => {
                let unseen = work
                    .repo_folders(&key)
                    .into_iter()
                    .flat_map(|folder| work.terminals_of(folder))
                    .any(|id| window_view.done.is_unseen(id));
                let tip = repo_tip(window_view, &key);
                list = list.child(repo_row(
                    RepoRowSpec { index: repo_index, key, name, expanded, n_folders, current, unseen, tip },
                    cx,
                ));
                repo_index += 1;
                nested = false;
            }
            Row::Folder { key, expanded, n_terminals, current, in_repo } => {
                nested = in_repo;
                let name = folder_name(&key, work.folder_label(&key), home.as_deref());
                let unseen = work.terminals_of(&key).into_iter().any(|id| window_view.done.is_unseen(id));
                let tip = folder_tip(window_view, &key);
                list = list.child(folder_row(
                    FolderRowSpec {
                        index: folder_index,
                        key,
                        name,
                        expanded,
                        n_terminals,
                        current,
                        unseen,
                        tip,
                        nested,
                    },
                    cx,
                ));
                folder_index += 1;
            }
            Row::Terminal(terminal) => {
                let look = row_look(window_view, terminal.id, terminal.id, &terminal.name);
                list = list.child(terminal_row(
                    TerminalRowSpec {
                        target: RowTarget::Terminal(terminal.id),
                        label: row_label(&terminal.name, &terminal.suffix),
                        count: None,
                        selected: selected == Some(terminal.id),
                        exited: look.exited,
                        unseen: window_view.done.is_unseen(terminal.id),
                        meta: look.meta,
                        remote: look.remote,
                        status: look.status,
                        tip: None,
                        drop: window_view.row_drop_marker(terminal.id, cx),
                        nested,
                    },
                    cx,
                ));
            }
            Row::More { folder, n_hidden } => {
                list = list.child(
                    div()
                        .id(("work-more", folder_index))
                        .h(zpx(sidebar::ROW_HEIGHT))
                        .pl(zpx(sidebar::TERMINAL_ROW_PADDING_BASE
                            + crate::look::sidebar_indent()
                            + if nested { folder_row::REPO_MEMBER_INDENT } else { 0.0 }))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(zpx(more_size::GAP))
                        .text_size(zpx(text::SMALL))
                        .text_color(hsla(palette.fg_muted))
                        .cursor_pointer()
                        .hover(|style| style.bg(hsla(colors().hover_bg)))
                        .child("\u{25be}")
                        .child(more_label(n_hidden))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                                this.show_more(&folder, window, cx)
                            }),
                        ),
                );
            }
        }
    }
    if !work.groups.is_empty() {
        list = list.child(
            div()
                .pt(zpx(sidebar::UNASSIGNED_PADDING_TOP))
                .pb(zpx(sidebar::UNASSIGNED_PADDING_BOTTOM))
                .px(zpx(sidebar::UNASSIGNED_PADDING_X))
                .text_size(zpx(text::TINY))
                .text_color(hsla(palette.fg_muted))
                .child("Unassigned"),
        );
        let selected_group = selected.and_then(|id| work.group_of(id));
        for group in &work.groups {
            let name = work.name_of(group.root).unwrap_or("terminal");
            let look = row_look(window_view, group.root, group.last_used, name);
            list = list.child(terminal_row(
                TerminalRowSpec {
                    target: RowTarget::Group(group.root),
                    label: name.to_string(),
                    count: Some(group.members.len()),
                    selected: selected_group == Some(group.root),
                    exited: look.exited,
                    unseen: group.members.iter().any(|member| window_view.done.is_unseen(member.id)),
                    meta: look.meta,
                    remote: look.remote,
                    status: look.status,
                    tip: Some(group_tip(window_view, group)),
                    drop: None,
                    nested: false,
                },
                cx,
            ));
        }
    }
    let files_folder = window_view.files.folder();
    // 파일 구획이 없는 폴더(홈 폴더의 터미널 등)에서도 검색이 열려 있으면 검색만 보이는 구획이 파일 구획 자리를 쓴다.
    let search_only = window_view.search.open && files_folder.is_none();
    sidebar_frame()
        .child(crate::view_bar::view_bar(window_view, cx))
        .child(header)
        .child(match files_folder {
            // 파일 구획이 펼쳐져 있으면 Work 목록은 내용 높이만큼만 차지하되 사이드바 높이의 절반을 넘지 않고, 나머지를 파일 구획이 쓴다.
            Some(_) if window_view.files.files_expanded => {
                list.flex_none().max_h(gpui_kit::relative(sidebar::WORK_MAX_HEIGHT_WITH_FILES))
            }
            None if search_only => list.flex_none().max_h(gpui_kit::relative(sidebar::WORK_MAX_HEIGHT_WITH_FILES)),
            // 파일 구획이 없거나 접혀 있으면 Work 목록이 남는 높이를 모두 쓴다.
            _ => list.flex_1().min_h_0(),
        })
        .children(files_folder.map(|folder| crate::files::list_view::files_section(window_view, folder, cx)))
        .children(search_only.then(|| crate::search::view::search_section(window_view, cx)))
}
