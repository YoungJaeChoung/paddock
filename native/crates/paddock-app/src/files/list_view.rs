//! 파일 목록 구획: 머리줄(`<폴더> FILES`, 새 파일·새 폴더·새로 고침·검색 단추)과 그 아래 가상 목록(검색이 열려 있으면 검색 보기).
//!
//! 행의 높이가 모두 같아서 화면에 보이는 행만 그린다 — 항목이 천 개여도 그리기 비용은 보이는 행 수에 비례한다.
use gpui_kit::{
    Context, ExternalPaths, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Styled, div, uniform_list,
};
use paddock_core::file_tree::ListFocusEvent;
use paddock_core::theme::{drop_target, file_row, files_header as size, text};
use paddock_core::work_model::folder_name;

use super::row::{MarkLook, file_glyph, file_row};
use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;
use crate::work::folder_row::folder_glyph;

/// 머리줄 오른쪽의 24×24 단추. 누름 처리는 호출자가 붙인다.
fn action_button(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex_none()
        .w(zpx(size::ACTION_SIZE))
        .h(zpx(size::ACTION_SIZE))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .rounded(zpx(size::ACTION_RADIUS))
        .text_size(zpx(text::UI))
        .text_color(hsla(colors().fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
}

/// 단추 안의 작은 `+` 표지(그림 옆에 붙어 "새로 만들기"를 뜻한다).
fn plus() -> impl IntoElement {
    div().flex_none().text_size(zpx(size::PLUS_SIZE)).child("+")
}

/// 머리줄의 검색 단추. 검색 보기가 열려 있는 동안은 눌린 모양이다.
fn search_button(
    open: bool,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let palette = colors();
    let button = action_button("files-search").child("\u{2315}");
    let button = if open { button.bg(hsla(palette.hover_bg)).text_color(hsla(palette.fg_strong)) } else { button };
    button.on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.toggle_search(window, cx)),
    )
}

/// 행의 Git 변경 표시. 안내 행과 바뀌지 않은 행은 표시가 없다.
fn mark_look(
    marks: &paddock_core::file_marks::FileMarks,
    row: &paddock_core::file_tree::FileRow,
) -> MarkLook {
    let palette = colors();
    let color_of = |color| paddock_core::file_marks::git_color(palette, color);
    let (name_color, letter) = if row.notice.is_some() {
        (None, None)
    } else if row.is_dir {
        (marks.folder(&row.path).map(color_of), None)
    } else {
        let mark = marks.file(&row.path);
        (mark.map(|mark| color_of(mark.color)), mark.map(|mark| (mark.letter.glyph(), color_of(mark.color))))
    };
    MarkLook { name_color, letter }
}

/// 파일 목록 구획. `folder`는 목록이 보여 주는 폴더다.
pub fn files_section(
    window_view: &WorkWindow,
    folder: &str,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let files = &window_view.files;
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    let work = window_view.workspace.work();
    let title = format!("{} FILES", folder_name(folder, work.folder_label(folder), home.as_deref()).to_uppercase());
    let expanded = files.files_expanded;
    let header = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .h(zpx(size::HEIGHT))
        .pl(zpx(size::PADDING_LEFT))
        .pr(zpx(size::PADDING_RIGHT))
        .gap(zpx(size::GAP))
        .border_b_1()
        .border_color(hsla(palette.border_subtle))
        .child(
            div()
                .id("files-title")
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(zpx(size::GAP))
                .cursor_pointer()
                .text_size(zpx(text::TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(hsla(palette.fg_muted))
                .child(div().flex_none().child(if expanded { "\u{25be}" } else { "\u{25b8}" }))
                .child(div().min_w_0().truncate().child(title))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| this.toggle_files_section(window, cx)),
                ),
        )
        .child(action_button("files-new-file").child(file_glyph(palette.fg_muted)).child(plus()).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.begin_new_entry(None, false, window, cx)),
        ))
        .child(action_button("files-new-folder").child(folder_glyph(palette.fg_muted)).child(plus()).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.begin_new_entry(None, true, window, cx)),
        ))
        .child(action_button("files-refresh").child("\u{21bb}").on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.refresh_files(window, cx)),
        ))
        .child(search_button(window_view.search.open, cx));
    let n_rows = files.rows().len();
    let body = if !expanded {
        None
    } else if window_view.search.open {
        // 검색 보기가 열려 있으면 파일 목록 자리를 검색이 쓴다.
        Some(div().flex_1().min_h_0().child(crate::search::view::search_body(window_view, cx)).into_any_element())
    } else if files.is_loaded() {
        // 목록의 빈 자리에 놓으면 머리줄 새 파일이 만드는 폴더에 받는다(행 위에 놓은 것은 그 행이 먼저 받는다).
        Some(
            div()
                .id("files-body")
                .flex_1()
                .min_h_0()
                .drag_over::<ExternalPaths>(|style, _, _, _| {
                    style
                        .bg(hsla(colors().hover_bg))
                        .border(zpx(drop_target::BORDER_WIDTH))
                        .border_color(hsla(colors().accent))
                })
                .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                    this.drop_into_files(dropped, None, window, cx)
                }))
                .child(
                    uniform_list(
                        "files-list",
                        n_rows,
                        cx.processor(|this: &mut WorkWindow, range: std::ops::Range<usize>, _window, cx| {
                            let current = this.selected_file_path().filter(|_| this.file_active());
                            range
                                .filter_map(|index| {
                                    this.files.rows().get(index).cloned().map(|row| {
                                        let is_current = !row.is_dir && current.as_deref() == Some(row.path.as_str());
                                        let mark = mark_look(this.git.marks(), &row);
                                        let is_selected = this.files.list_focused()
                                            && this.files.selected_row() == Some(row.path.as_str());
                                        file_row(index, &row, is_current, is_selected, mark, cx)
                                    })
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .size_full(),
                )
                .into_any_element(),
        )
    } else {
        Some(
            div()
                .h(zpx(file_row::HEIGHT))
                .pl(zpx(file_row::PADDING_LEFT + file_row::NOTICE_PADDING_LEFT))
                .flex()
                .items_center()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.fg_muted))
                .child(files.failure().map_or_else(|| "Reading\u{2026}".to_string(), str::to_string))
                .into_any_element(),
        )
    };
    // 목록 밖을 누르면 목록이 키를 받지 않는다(F2·Ctrl+V가 터미널로 간다).
    let section = div()
        .id("files-section")
        .flex()
        .flex_col()
        .on_mouse_down_out(cx.listener(|this, _event: &MouseDownEvent, _window, _cx| {
            this.files.focus_event(ListFocusEvent::OtherAreaPressed)
        }))
        .child(header)
        .children(body);
    if expanded { section.flex_1().min_h_0() } else { section.flex_none() }
}
