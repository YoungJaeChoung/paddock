//! Source control 사이드바: 머리줄·저장소 줄·커밋 입력칸과 `Commit` 단추·구획별 변경 행. 저장소가 없으면 안내만 보인다.
use gpui_kit::component::input::Textarea;
use gpui_kit::{
    Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
    uniform_list,
};
use paddock_core::file_marks::{color_of, git_color};
use paddock_core::git_status::{Change, Section, mark_of};
use paddock_core::theme::{scm as size, sidebar, text, work_empty};
use paddock_core::work_model::folder_name;

use super::model::{CommitState, Group, Item, commit_state, items, split_path};
use crate::chrome::zpx;
use crate::files::row::file_glyph;
use crate::git::state::Repo;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 머리줄과 목록 사이에 놓는 단추(`+`·`−`·되돌리기): `size::ACTION_SIZE` 정사각형. 누름 처리는 호출자가 붙인다.
fn action_button(
    id: impl Into<gpui_kit::ElementId>,
    glyph: &'static str,
) -> gpui_kit::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(zpx(size::ACTION_SIZE))
        .h(zpx(size::ACTION_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::ACTION_RADIUS))
        .text_size(zpx(text::UI))
        .text_color(hsla(colors().fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child(glyph)
}

/// 변경 행의 상태 글자 색(병합 충돌 행은 글자가 `D`여도 충돌 색).
fn change_color(change: &Change) -> u32 {
    git_color(colors(), color_of(change))
}

/// 저장소가 없거나 읽지 못했을 때의 안내(`No Git repository`·폴더 경로·고르라는 말).
fn empty_notice(
    headline: &str,
    path: Option<&str>,
    hint: &str,
) -> Div {
    let palette = colors();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mt(zpx(work_empty::TITLE_MARGIN_TOP))
                .mx(zpx(work_empty::TITLE_MARGIN_X))
                .mb(zpx(work_empty::TITLE_MARGIN_BOTTOM))
                .text_color(hsla(palette.fg_strong))
                .font_weight(FontWeight::SEMIBOLD)
                .child(headline.to_string()),
        )
        .children(path.map(|path| {
            div()
                .py(zpx(work_empty::TEXT_PADDING_Y))
                .px(zpx(work_empty::TEXT_PADDING_X))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(path.to_string())
        }))
        .child(
            div()
                .py(zpx(work_empty::TEXT_PADDING_Y))
                .px(zpx(work_empty::TEXT_PADDING_X))
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(hint.to_string()),
        )
}

/// 변경 행 하나.
fn change_row(
    index: usize,
    group: Group,
    change: &Change,
    current: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let (name, dir) = split_path(&change.path);
    let letter = mark_of(change);
    let group_name = format!("scm-row-{index}");
    let open = change.clone();
    let mut actions = div().flex_none().flex().flex_row().items_center().gap(zpx(size::ACTION_GAP));
    let discard_change = change.clone();
    let stage_path = change.path.clone();
    if group == Group::Changes {
        actions = actions.child(action_button(("scm-discard", index), "\u{21b6}").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.scm_ask_discard(discard_change.clone(), window, cx);
            }),
        ));
    }
    let toggle_path = stage_path.clone();
    actions = match group {
        Group::Staged => actions.child(action_button(("scm-unstage", index), "\u{2212}").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.scm_unstage(&toggle_path, window, cx);
            }),
        )),
        Group::Merge | Group::Changes => actions.child(action_button(("scm-stage", index), "+").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.scm_stage(&toggle_path, window, cx);
            }),
        )),
    };
    let row = div()
        .id(("scm-row", index))
        .group(group_name.clone())
        .h(zpx(size::ROW_HEIGHT))
        .px(zpx(size::ROW_PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::ROW_GAP))
        .cursor_pointer()
        .text_size(zpx(text::UI))
        .text_color(hsla(palette.fg))
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(div().flex_none().child(file_glyph(palette.fg_muted)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .overflow_hidden()
                .child(div().flex_none().max_w_full().truncate().child(name))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .ml(zpx(size::PATH_MARGIN_LEFT))
                        .text_size(zpx(text::SMALL))
                        .opacity(size::PATH_OPACITY)
                        .child(dir),
                ),
        )
        .child(actions.invisible().group_hover(group_name, |style| style.visible()))
        .child(
            div()
                .flex_none()
                .w(zpx(size::STATUS_WIDTH))
                .mr(zpx(size::STATUS_MARGIN_RIGHT))
                .text_center()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(change_color(change)))
                .child(letter.glyph().to_string()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.scm_open_row(&open, window, cx)),
        );
    if current { row.bg(hsla(palette.current_bg)) } else { row }
}

/// 구획 머리 한 줄: 접기 표지·이름·변경 수와 구획 전체 단추(`Stage All`·`Unstage All`).
fn group_header(
    index: usize,
    group: Group,
    n_rows: usize,
    collapsed: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let all = match group {
        Group::Staged => Some(action_button(("scm-unstage-all", index), "\u{2212}").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.scm_unstage_all(window, cx);
            }),
        )),
        Group::Changes | Group::Merge => Some(action_button(("scm-stage-all", index), "+").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.scm_stage_group(group, window, cx);
            }),
        )),
    };
    div()
        .id(("scm-header", index))
        .h(zpx(size::SECTION_HEIGHT))
        .px(zpx(size::SECTION_PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::ROW_GAP))
        .cursor_pointer()
        .text_size(zpx(text::TINY))
        .font_weight(FontWeight::BOLD)
        .text_color(hsla(palette.fg_strong))
        .child(div().flex_none().w(zpx(size::CHEVRON_WIDTH)).child(if collapsed { "\u{25b8}" } else { "\u{25be}" }))
        .child(div().flex_1().min_w_0().truncate().child(group.title().to_uppercase()))
        .children(all)
        .child(
            div()
                .flex_none()
                .min_w(zpx(size::COUNT_MIN_WIDTH))
                .h(zpx(size::COUNT_HEIGHT))
                .px(zpx(size::COUNT_PADDING_X))
                .flex()
                .items_center()
                .justify_center()
                .rounded(zpx(size::COUNT_HEIGHT / 2.0))
                .bg(hsla(palette.badge_bg))
                .text_color(hsla(palette.badge_fg))
                .child(n_rows.to_string()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.scm_toggle_group(group, window, cx)),
        )
}

/// 커밋 입력칸과 단추 자리.
fn commit_box(
    window_view: &WorkWindow,
    n_staged: usize,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let message = window_view.commit_message(cx);
    let state = commit_state(n_staged, &message);
    let input = window_view.scm.commit_input.as_ref().map(|input| {
        div()
            .min_h(zpx(size::INPUT_LINES * size::INPUT_LINE_HEIGHT))
            .px(zpx(size::INPUT_PADDING_X))
            .rounded(zpx(size::ACTION_RADIUS))
            .border_1()
            .border_color(hsla(palette.control_border))
            .bg(hsla(palette.control_bg))
            .text_size(zpx(text::UI))
            .child(Textarea::new(input).appearance(false).bordered(false))
    });
    let action = match state {
        CommitState::NothingStaged => div()
            .h(zpx(size::BUTTON_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .text_size(zpx(text::SMALL))
            .text_color(hsla(palette.fg_muted))
            .child("Stage changes to commit")
            .into_any_element(),
        CommitState::NeedsMessage | CommitState::Ready => div()
            .id("scm-commit")
            .h(zpx(size::BUTTON_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .gap(zpx(size::BUTTON_GAP))
            .bg(hsla(palette.button_primary_bg))
            .text_color(hsla(palette.button_primary_fg))
            .text_size(zpx(text::UI))
            .cursor_pointer()
            .opacity(if state == CommitState::Ready { 1.0 } else { size::BUTTON_DISABLED_OPACITY })
            .child(if window_view.git.is_committing() { "\u{2026}" } else { "\u{2713}" })
            .child(if window_view.git.is_committing() { "Committing" } else { "Commit" })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| this.scm_commit(window, cx)),
            )
            .into_any_element(),
    };
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(zpx(size::COMMIT_PADDING))
        .pl(zpx(size::COMMIT_PADDING_LEFT))
        .pr(zpx(size::COMMIT_PADDING))
        .py(zpx(size::COMMIT_PADDING))
        .children(input)
        .children(
            window_view
                .scm
                .note
                .as_ref()
                .map(|note| div().text_size(zpx(text::SMALL)).text_color(hsla(palette.warning)).child(note.clone())),
        )
        .child(action)
}

/// Source control 사이드바의 내용(보기 줄 아래 전부).
pub fn scm_content(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    let header = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(zpx(sidebar::HEADER_HEIGHT))
        .pl(zpx(sidebar::HEADER_PADDING_LEFT))
        .pr(zpx(sidebar::HEADER_PADDING_RIGHT))
        .border_b_1()
        .border_color(hsla(palette.border_subtle))
        .child(
            div()
                .text_size(zpx(text::SMALL))
                .font_weight(FontWeight::BOLD)
                .text_color(hsla(palette.fg_muted))
                .child("SOURCE CONTROL"),
        )
        .child(action_button("scm-refresh", "\u{21bb}").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.scm_refresh(window, cx)),
        ));
    let folder = window_view.git.folder().map(str::to_string);
    let body = match window_view.git.repo() {
        Repo::Ready(repo) => {
            let name = folder_name(&repo.root, None, home.as_deref());
            let branch = repo.status.head_label().unwrap_or_default();
            let n_staged = repo.status.n_in(Section::Staged);
            let list = items(&repo.status, &window_view.scm.collapsed);
            let n_items = list.len();
            let current = window_view.selected_file_path();
            let repo_line = div()
                .flex_none()
                .h(zpx(size::REPO_HEIGHT))
                .px(zpx(size::PADDING_X))
                .flex()
                .flex_row()
                .items_center()
                .gap(zpx(size::ROW_GAP))
                .child(div().min_w_0().truncate().text_color(hsla(palette.fg_strong)).child(name))
                .child(div().flex_none().text_size(zpx(text::SMALL)).text_color(hsla(palette.fg_muted)).child(branch));
            let root = repo.root.clone();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(repo_line)
                .child(commit_box(window_view, n_staged, cx))
                .child(
                    div().flex_1().min_h_0().child(
                        uniform_list(
                            "scm-list",
                            n_items,
                            cx.processor(move |this: &mut WorkWindow, range: std::ops::Range<usize>, _window, cx| {
                                let collapsed = this.scm.collapsed.clone();
                                let list =
                                    this.git.ready().map(|repo| items(&repo.status, &collapsed)).unwrap_or_default();
                                range
                                    .filter_map(|index| {
                                        list.get(index).cloned().map(|item| match item {
                                            Item::Header { group, n_rows } => {
                                                group_header(index, group, n_rows, collapsed.contains(&group), cx)
                                            }
                                            Item::Row { group, change } => {
                                                let path = paddock_core::file_tree::child_path(&root, &change.path);
                                                let is_current = current.as_deref() == Some(path.as_str());
                                                change_row(index, group, &change, is_current, cx)
                                            }
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full(),
                    ),
                )
        }
        Repo::Reading => div()
            .px(zpx(work_empty::TEXT_PADDING_X))
            .py(zpx(work_empty::TEXT_PADDING_Y))
            .text_size(zpx(text::SMALL))
            .text_color(hsla(palette.fg_muted))
            .child("Reading\u{2026}"),
        Repo::NoRepository => {
            empty_notice("No Git repository", folder.as_deref(), "Select a work folder that contains a Git repository.")
        }
        Repo::Unavailable(message) => empty_notice(message, folder.as_deref(), "Check that Git is installed."),
    };
    let body = if folder.is_none() {
        empty_notice("No Git repository", None, "Select a work folder that contains a Git repository.")
    } else {
        body
    };
    div().flex_1().min_h_0().flex().flex_col().child(header).child(body)
}
