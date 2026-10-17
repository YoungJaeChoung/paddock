//! 파일 목록의 행 하나: 폴더는 펼침 표지와 폴더 그림, 파일은 쪽 모양 그림, 그리고 이름. 안내 행은 글자만 보인다.
use gpui_kit::{Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div};
use paddock_core::file_tree::FileRow;
use paddock_core::theme::{file_row as size, folder_row, text};

use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;
use crate::work::folder_row::folder_glyph;

/// 쪽 모양의 작은 파일 그림. 아이콘 글꼴을 싣지 않으므로 도형으로 그린다.
pub fn file_glyph(color: u32) -> impl IntoElement {
    div()
        .flex_none()
        .w(zpx(folder_row::ICON_BOX))
        .h(zpx(folder_row::ICON_BOX))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(zpx(size::FILE_WIDTH))
                .h(zpx(size::FILE_HEIGHT))
                .border_1()
                .border_color(hsla(color))
                .rounded(zpx(size::FILE_RADIUS)),
        )
}

/// 행에 붙는 Git 변경 표시: 이름 색(바뀐 파일·바뀐 파일을 품은 폴더)과 파일 행 오른쪽 상태 글자.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkLook {
    pub name_color: Option<u32>,
    pub letter: Option<(char, u32)>,
}

/// 행 하나를 만든다. 폴더 행을 왼쪽으로 누르면 접고 펼치며, 파일 행을 왼쪽으로 누르면 파일 탭으로 열고,
/// 오른쪽으로 누르면 메뉴가 열린다. 안내 행은 글자만 보인다. `is_current`는 지금 활성인 파일 탭의 파일이라는 뜻이다.
pub fn file_row(
    index: usize,
    row: &FileRow,
    is_current: bool,
    mark: MarkLook,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let left = size::PADDING_LEFT + row.depth as f32 * size::INDENT;
    match row.notice {
        Some(notice) => notice_row(index, notice, left),
        None => entry_row(index, row, left, is_current, mark, cx),
    }
}

/// 비었거나 읽지 못한 폴더 자리의 안내 글 행.
fn notice_row(
    index: usize,
    notice: &'static str,
    left: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(("file-notice", index))
        .h(zpx(size::HEIGHT))
        .pl(zpx(left + size::NOTICE_PADDING_LEFT))
        .pr(zpx(size::PADDING_RIGHT))
        .flex()
        .items_center()
        .text_size(zpx(text::SMALL))
        .text_color(hsla(colors().fg_muted))
        .child(notice)
}

/// 파일이나 폴더 하나의 행.
fn entry_row(
    index: usize,
    row: &FileRow,
    left: f32,
    is_current: bool,
    mark: MarkLook,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let palette = colors();
    let (path, is_dir) = (row.path.clone(), row.is_dir);
    let (toggle_path, menu_path) = (path.clone(), path);
    let text_color = mark.name_color.unwrap_or(if is_current { palette.fg_strong } else { palette.file_row_fg });
    let leading = if is_dir {
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(zpx(size::GAP))
            .child(
                div()
                    .flex_none()
                    .w(zpx(folder_row::ICON_BOX))
                    .flex()
                    .justify_center()
                    .text_size(zpx(text::SMALL))
                    .child(if row.expanded { "\u{25be}" } else { "\u{25b8}" }),
            )
            .child(folder_glyph(palette.file_row_fg))
    } else {
        div().flex_none().ml(zpx(size::FILE_ICON_MARGIN_LEFT)).child(file_glyph(palette.file_row_fg))
    };
    let row_box = div()
        .id(("file-row", index))
        .h(zpx(size::HEIGHT))
        .pl(zpx(left))
        .pr(zpx(size::PADDING_RIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::GAP))
        .cursor_pointer()
        .text_size(zpx(text::UI))
        .text_color(hsla(text_color))
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)));
    let row_box = if is_current { row_box.bg(hsla(palette.current_bg)) } else { row_box };
    row_box
        .child(leading)
        .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
        .children(mark.letter.map(|(letter, color)| {
            div()
                .flex_none()
                .w(zpx(size::MARK_WIDTH))
                .text_center()
                .text_size(zpx(size::MARK_SIZE))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(hsla(color))
                .child(letter.to_string())
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                if is_dir {
                    this.toggle_files_dir(&toggle_path, window, cx);
                } else {
                    this.open_file(&toggle_path, window, cx);
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_entry_menu(&menu_path, is_dir, event.position, window, cx);
            }),
        )
}
