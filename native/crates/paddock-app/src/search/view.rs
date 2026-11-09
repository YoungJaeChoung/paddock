//! 폴더 검색 보기: 입력칸과 옵션 단추, 범위 안내, 결과 개수, 결과 트리(파일 행 + 일치 줄 행), 결과 상한 안내.
//!
//! 결과 행은 높이가 모두 같아서 화면에 보이는 행만 그린다 — 결과가 2,000건이어도 그리기 비용은 보이는 행 수에 비례한다.
use gpui_kit::component::input::Input;
use gpui_kit::{
    Context, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Stateful, Styled, div,
    uniform_list,
};
use paddock_core::search_model::{LIMIT_NOTICE, NO_FOLDER_NOTICE, ResultRow, count_text, scope_text};
use paddock_core::theme::{files_header, scm, search as size, search_replace as replace_glyph, text};
use paddock_core::work_model::folder_name;

use crate::chrome::zpx;
use crate::files::row::file_glyph;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 입력칸 오른쪽의 옵션 단추 하나(`Aa`·`ab`·`.*`). 켜져 있으면 강조 테두리와 바탕이 붙는다.
fn option_button(
    id: &'static str,
    label: &'static str,
    on: bool,
    name: &'static str,
    cx: &mut Context<WorkWindow>,
) -> Stateful<Div> {
    let palette = colors();
    let button = div()
        .id(id)
        .flex_none()
        .w(zpx(size::OPTION_SIZE))
        .h(zpx(size::OPTION_SIZE))
        .mx(zpx(size::OPTION_MARGIN_X))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::INPUT_RADIUS))
        .border_1()
        .border_color(hsla(if on { palette.accent } else { palette.control_bg }));
    let button = if on { button.bg(hsla(palette.search_option_on_bg)) } else { button };
    button
        .text_size(zpx(text::TINY))
        .font_weight(FontWeight::BOLD)
        .text_color(hsla(if on { palette.fg_strong } else { palette.fg }))
        .opacity(if on { 1.0 } else { size::OPTION_OFF_OPACITY })
        .cursor_pointer()
        .hover(|style| style.opacity(1.0))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| this.toggle_search_option(name, window, cx)),
        )
}

/// 바꾸기 줄을 여닫는 단추(`⇄`). 켜져 있으면 옵션 단추처럼 강조 테두리와 바탕이 붙는다.
fn replace_toggle(
    on: bool,
    cx: &mut Context<WorkWindow>,
) -> Stateful<Div> {
    let palette = colors();
    let button = div()
        .id("search-replace-toggle")
        .flex_none()
        .w(zpx(size::OPTION_SIZE))
        .h(zpx(size::OPTION_SIZE))
        .mx(zpx(size::OPTION_MARGIN_X))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::INPUT_RADIUS))
        .border_1()
        .border_color(hsla(if on { palette.accent } else { palette.control_bg }));
    let button = if on { button.bg(hsla(palette.search_option_on_bg)) } else { button };
    button
        .text_size(zpx(text::SMALL))
        .text_color(hsla(if on { palette.fg_strong } else { palette.fg }))
        .opacity(if on { 1.0 } else { size::OPTION_OFF_OPACITY })
        .cursor_pointer()
        .hover(|style| style.opacity(1.0))
        .child(replace_glyph::TOGGLE_GLYPH)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.toggle_replace(window, cx)),
        )
}

/// 글자 단추 하나(전체 테두리, 올리면 바탕). 누름 처리는 호출자가 붙인다.
fn replace_button(
    id: impl Into<gpui_kit::ElementId>,
    label: &'static str,
    _cx: &mut Context<WorkWindow>,
) -> Stateful<Div> {
    let palette = colors();
    div()
        .id(id)
        .flex_none()
        .h(zpx(replace_glyph::BUTTON_HEIGHT))
        .px(zpx(replace_glyph::BUTTON_PADDING_X))
        .flex()
        .items_center()
        .rounded(zpx(replace_glyph::BUTTON_RADIUS))
        .border_1()
        .border_color(hsla(palette.border))
        .text_size(zpx(text::SMALL))
        .text_color(hsla(palette.fg))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(label)
}

/// 일치 줄 행: 미리보기 글(일치 부분만 바탕색)과 오른쪽 줄 번호.
fn match_row(
    index: usize,
    file: usize,
    match_index: usize,
    line: u64,
    preview: &paddock_core::search_model::Preview,
    cx: &mut Context<WorkWindow>,
) -> Stateful<Div> {
    let palette = colors();
    let (before, rest) = preview.text.split_at(preview.highlight.start);
    let (matched, after) = rest.split_at(preview.highlight.len());
    div()
        .id(("search-match", index))
        .w_full()
        .h(zpx(size::ROW_HEIGHT))
        .pl(zpx(size::MATCH_INDENT))
        .pr(zpx(size::ROW_PADDING_RIGHT))
        .flex()
        .flex_row()
        .items_center()
        .cursor_pointer()
        .text_size(zpx(text::UI))
        .text_color(hsla(palette.file_row_fg))
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(div().flex_none().child(before.to_string()))
                .child(div().flex_none().bg(hsla(palette.search_match_bg)).child(matched.to_string()))
                .child(div().flex_none().child(after.to_string())),
        )
        .child(
            div()
                .flex_none()
                .ml(zpx(size::LINE_NUMBER_MARGIN_LEFT))
                .mr(zpx(size::LINE_NUMBER_MARGIN_RIGHT))
                .text_size(zpx(text::UI * size::LINE_NUMBER_SCALE))
                .opacity(size::LINE_NUMBER_OPACITY)
                .child(line.to_string()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                this.open_search_match(file, match_index, window, cx)
            }),
        )
}

/// 파일 행: 접기 표지·파일 그림·이름·폴더 경로(흐림)와 오른쪽 일치 수.
fn file_row(
    index: usize,
    file: usize,
    (name, dir): (&str, &str),
    n_matches: usize,
    collapsed: bool,
    can_replace: bool,
    cx: &mut Context<WorkWindow>,
) -> Stateful<Div> {
    let palette = colors();
    let group = format!("search-file-{index}");
    div()
        .id(("search-file", index))
        .group(group.clone())
        .w_full()
        .h(zpx(size::ROW_HEIGHT))
        .pl(zpx(size::ROW_PADDING_LEFT))
        .pr(zpx(size::ROW_PADDING_RIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::ROW_GAP))
        .cursor_pointer()
        .text_size(zpx(text::UI))
        .text_color(hsla(palette.fg))
        .hover(|style| style.bg(hsla(colors().hover_bg)))
        .child(div().flex_none().w(zpx(size::CHEVRON_WIDTH)).text_size(zpx(text::SMALL)).child(if collapsed {
            "\u{25b8}"
        } else {
            "\u{25be}"
        }))
        .child(file_glyph(palette.file_row_fg))
        .child(div().flex_none().max_w_full().truncate().child(name.to_string()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .ml(zpx(size::PATH_MARGIN_LEFT))
                .text_size(zpx(text::TINY))
                .opacity(scm::PATH_OPACITY)
                .child(dir.to_string()),
        )
        .child(
            div()
                .flex_none()
                .min_w(zpx(scm::COUNT_MIN_WIDTH))
                .h(zpx(scm::COUNT_HEIGHT))
                .px(zpx(scm::COUNT_PADDING_X))
                .flex()
                .items_center()
                .justify_center()
                .rounded(zpx(scm::COUNT_HEIGHT / 2.0))
                .bg(hsla(palette.badge_bg))
                .text_size(zpx(text::TINY))
                .text_color(hsla(palette.badge_fg))
                .child(n_matches.to_string()),
        )
        .children(can_replace.then(|| {
            // 마우스를 올린 파일 행에서만 보이는 `Replace`(그 파일 하나에 바꾸기).
            replace_button(("search-replace-file", index), "Replace", cx)
                .invisible()
                .group_hover(group, |style| style.visible())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.replace_file(file, window, cx)
                    }),
                )
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, _window, cx| this.toggle_search_file(file, cx)),
        )
}

/// 검색 보기 본문. 파일 구획의 머리줄 아래(또는 검색 구획의 머리줄 아래)에 놓인다.
/// 검색할 폴더가 없으면(홈 폴더·파일 시스템 루트의 터미널) 입력칸 자리에 안내만 보인다.
pub fn search_body(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Div {
    match window_view.search.scope() {
        None => no_folder_body(),
        Some(_) => results_body(window_view, cx),
    }
}

/// 검색할 폴더가 없을 때의 안내 한 줄.
fn no_folder_body() -> Div {
    let palette = colors();
    div().size_full().flex().flex_col().child(
        div()
            .flex_none()
            .h(zpx(size::COUNT_HEIGHT))
            .pl(zpx(size::COUNT_PADDING_LEFT))
            .flex()
            .items_center()
            .text_size(zpx(text::SMALL))
            .text_color(hsla(palette.fg_muted))
            .child(NO_FOLDER_NOTICE),
    )
}

/// 입력칸·검색 폴더·상태 줄·결과 목록·상한 안내를 쌓은 본문. 검색 폴더가 있을 때만 부른다.
fn results_body(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let state = &window_view.search;
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    let work = window_view.workspace.work();
    let scope_name =
        state.scope().map(|scope| folder_name(scope, work.folder_label(scope), home.as_deref())).unwrap_or_default();
    let field = div()
        .h(zpx(size::INPUT_HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .pl(zpx(size::INPUT_PADDING_X))
        .rounded(zpx(size::INPUT_RADIUS))
        .border_1()
        .border_color(hsla(palette.accent))
        .bg(hsla(palette.control_bg))
        .text_size(zpx(text::UI))
        .children(
            state
                .input
                .as_ref()
                .map(|input| div().flex_1().min_w_0().child(Input::new(input).appearance(false).bordered(false))),
        )
        .child(option_button("search-case", "Aa", state.options.case_sensitive, "case", cx))
        .child(option_button("search-word", "ab", state.options.whole_word, "word", cx))
        .child(option_button("search-regex", ".*", state.options.regex, "regex", cx))
        .child(replace_toggle(state.replace_open, cx));
    let replace_field = state.replace_open.then(|| {
        div()
            .mt(zpx(size::SCOPE_PADDING_TOP))
            .h(zpx(size::INPUT_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .pl(zpx(size::INPUT_PADDING_X))
            .rounded(zpx(size::INPUT_RADIUS))
            .border_1()
            .border_color(hsla(palette.border))
            .bg(hsla(palette.control_bg))
            .text_size(zpx(text::UI))
            .children(
                state
                    .replace_input
                    .as_ref()
                    .map(|input| div().flex_1().min_w_0().child(Input::new(input).appearance(false).bordered(false))),
            )
    });
    let header = div()
        .flex_none()
        .pt(zpx(size::HEADER_PADDING_TOP))
        .pb(zpx(size::HEADER_PADDING_BOTTOM))
        .pl(zpx(size::HEADER_PADDING_LEFT))
        .pr(zpx(size::HEADER_PADDING_RIGHT))
        .child(field)
        .children(replace_field);
    let scope = div()
        .flex_none()
        .pt(zpx(size::SCOPE_PADDING_TOP))
        .px(zpx(size::SCOPE_PADDING_X))
        .pb(zpx(size::SCOPE_PADDING_BOTTOM))
        .truncate()
        .text_size(zpx(text::TINY))
        .text_color(hsla(palette.fg_muted))
        .child(scope_text(&scope_name));
    let results = state.results();
    let status = match (state.failure(), state.is_running(), state.is_finished()) {
        (Some(message), _, _) => Some((message.to_string(), palette.warning)),
        (None, true, _) if results.n_matches() == 0 => Some(("Searching\u{2026}".to_string(), palette.fg_muted)),
        (None, _, true) | (None, true, _) => {
            Some((count_text(results.n_matches(), results.files().len()), palette.fg_muted))
        }
        _ => None,
    };
    let status_line = status.map(|(message, color)| {
        div()
            .flex_none()
            .h(zpx(size::COUNT_HEIGHT))
            .pl(zpx(size::COUNT_PADDING_LEFT))
            .flex()
            .items_center()
            .text_size(zpx(text::SMALL))
            .text_color(hsla(color))
            .child(div().flex_1().min_w_0().truncate().child(message))
            .children((state.replace_open && results.n_matches() > 0).then(|| {
                replace_button("search-replace-all", "Replace All", cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| this.request_replace_all(window, cx)),
                )
            }))
    });
    let n_rows = state.rows().len();
    let list = div().flex_1().min_h_0().child(
        uniform_list(
            "search-list",
            n_rows,
            cx.processor(|this: &mut WorkWindow, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|index| {
                        this.search.rows().get(index).cloned().map(|row| match row {
                            ResultRow::File { file, name, dir, n_matches, collapsed } => {
                                file_row(index, file, (&name, &dir), n_matches, collapsed, this.search.replace_open, cx)
                            }
                            ResultRow::Match { file, index: match_index, line, preview } => {
                                match_row(index, file, match_index, line, &preview, cx)
                            }
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full(),
    );
    let notice = results.capped().then(|| {
        div()
            .flex_none()
            .m(zpx(size::NOTICE_MARGIN))
            .p(zpx(size::NOTICE_PADDING))
            .border_1()
            .border_color(hsla(palette.warning))
            .bg(hsla(palette.search_notice_bg))
            .text_size(zpx(text::SMALL))
            .text_color(hsla(palette.fg))
            .child(LIMIT_NOTICE)
    });
    div().size_full().flex().flex_col().child(header).child(scope).children(status_line).child(list).children(notice)
}

/// 파일 구획이 없을 때(홈 폴더의 터미널 등) 검색만 보이는 구획: 머리줄 `SEARCH`와 닫기 단추 아래에 검색 본문.
pub fn search_section(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let header = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .h(zpx(files_header::HEIGHT))
        .pl(zpx(files_header::PADDING_LEFT))
        .pr(zpx(files_header::PADDING_RIGHT))
        .gap(zpx(files_header::GAP))
        .border_b_1()
        .border_color(hsla(palette.border_subtle))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(zpx(text::TINY))
                .font_weight(FontWeight::BOLD)
                .text_color(hsla(palette.fg_muted))
                .child("SEARCH"),
        )
        .child(
            div()
                .id("search-close")
                .flex_none()
                .w(zpx(files_header::ACTION_SIZE))
                .h(zpx(files_header::ACTION_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(zpx(files_header::ACTION_RADIUS))
                .text_size(zpx(text::UI))
                .text_color(hsla(palette.fg_muted))
                .cursor_pointer()
                .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
                .child("\u{00d7}")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| this.close_search(window, cx)),
                ),
        );
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(header)
        .child(div().flex_1().min_h_0().child(search_body(window_view, cx)))
}
