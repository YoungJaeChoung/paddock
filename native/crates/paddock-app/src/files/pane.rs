//! 파일 칸: 선택한 파일 탭의 내용(텍스트 보기 또는 안내 탭)과 터미널 배치 사이의 경계.
//!
//! 칸은 묶음 본문의 오른쪽 열이다. 위에는 이름표 줄을 따로 두지 않는다 — 파일 탭은 위쪽 탭 줄의 마지막 열에 놓인다.
use gpui_kit::{
    App, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, div, relative,
};
use paddock_core::file_kind::{FileKind, format_size};
use paddock_core::file_pane::FilePane;
use paddock_core::theme::{split, text, untracked_bar};

use super::image_view::ImageView;
use super::markdown_view::{MarkdownBody, markdown_view};
use super::notice_view::{NoticeView, notice_view};
use super::open::{NoticeReason, notice_actions, notice_message};
use super::pdf_view::PdfView;
use super::text_view::{TextBody, text_view};
use crate::chrome::zpx;
use crate::look::colors;
use crate::scm::diff_view::{DiffBody, diff_view};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 글자의 FNV-1a 64비트 해시(자동 점검의 내용 비교용).
#[cfg(feature = "automation")]
fn text_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// 파일 하나를 읽은 뒤 창이 쥐는 내용.
pub enum FileContent {
    /// 작업 스레드가 아직 읽는 중이다.
    Loading,
    Text(TextBody),
    /// Markdown: 원문·미리보기·나란히 보기.
    Markdown(MarkdownBody),
    /// 이미지(래스터·SVG).
    Image(Entity<ImageView>),
    /// PDF 문서.
    Pdf(Entity<PdfView>),
    Notice {
        reason: NoticeReason,
        size: u64,
    },
    /// 열지 못했다. 값은 사용자에게 보일 문장이다.
    Failed(String),
    /// Git 변경 비교.
    Diff(DiffBody),
}

impl FileContent {
    /// 이 내용이 키를 받을 자리. 텍스트 보기는 편집기, 나머지는 없다(창 자신이 받는다).
    pub fn focus_handle(
        &self,
        cx: &App,
    ) -> Option<FocusHandle> {
        match self {
            Self::Text(body) => Some(body.editor.focus_handle(cx)),
            Self::Markdown(body) => Some(body.focus_handle(cx)),
            Self::Loading | Self::Image(_) | Self::Pdf(_) | Self::Notice { .. } | Self::Failed(_) | Self::Diff(_) => {
                None
            }
        }
    }

    /// 자동 점검이 읽는 `file-view` 줄의 내용. 텍스트 보기는 편집기에 지금 든 글자의 해시(`content-hash`)를 더해, 키 입력을 흘려도
    /// 내용이 그대로인지 파일 해시와 함께 비교할 수 있게 한다.
    #[cfg(feature = "automation")]
    pub fn summary(
        &self,
        kind: FileKind,
        cx: &App,
    ) -> String {
        match self {
            Self::Loading => format!("kind={} state=loading", kind.name()),
            Self::Text(body) => format!(
                "kind={} state=text lines={} language={} size={} content-hash={:016x} scroll-y={:.0} readonly=yes",
                kind.name(),
                body.n_lines,
                body.language,
                body.size,
                text_hash(&body.editor.read(cx).value()),
                f32::from(body.editor.read(cx).scroll_offset().y)
            ),
            Self::Markdown(body) => {
                let (n_loaded, n_images) = body.image_counts();
                let (n_lines, value) = body.source_summary(cx);
                format!(
                    "kind={} state=markdown view={} lines={} size={} content-hash={:016x} preview-width={:.0} images={n_loaded}/{n_images} scroll-y={:.0} readonly=yes",
                    kind.name(),
                    body.view.name(),
                    n_lines,
                    body.file_size(),
                    text_hash(&value),
                    body.preview_width(),
                    body.source_scroll_y(cx)
                )
            }
            Self::Image(view) => {
                let view = view.read(cx);
                let (shown_width, shown_height) = view.shown();
                format!(
                    "kind={} state=image size={} decoded={} bytes={} zoom={} full={} shown={:.0}x{:.0}",
                    kind.name(),
                    paddock_core::image_view::size_text(view.original().0, view.original().1).replace(" × ", "x"),
                    paddock_core::image_view::size_text(view.decoded().0, view.decoded().1).replace(" × ", "x"),
                    view.file_size(),
                    view.zoom().name(),
                    if view.has_full() { "yes" } else { "no" },
                    shown_width,
                    shown_height
                )
            }
            Self::Pdf(view) => {
                let view = view.read(cx);
                format!(
                    "kind={} state=pdf pages={} visible={} cached={} zoom={} percent={} renders={} render-ms={} bytes={} scroll-y={:.0}",
                    kind.name(),
                    view.n_pages(),
                    paddock_core::pdf_view::visible_text(view.visible()),
                    paddock_core::pdf_view::visible_text(&view.cached()),
                    if view.is_fit() { "fit" } else { "set" },
                    view.percent(),
                    view.n_renders(),
                    view.last_render_ms(),
                    view.file_size(),
                    view.scroll_y()
                )
            }
            Self::Notice { reason, size } => {
                format!("kind={} state=notice reason={reason:?} size={}", kind.name(), format_size(*size))
            }
            Self::Failed(message) => format!("kind={} state=failed message={}", kind.name(), message.replace(' ', "_")),
            Self::Diff(body) => format!(
                "kind={} state=diff files={} added={} removed={} readonly=yes",
                kind.name(),
                body.n_files,
                body.n_added,
                body.n_removed
            ),
        }
    }
}

/// 파일 칸 안쪽: 선택한 탭의 내용. 칸을 누르면 파일 탭이 활성이 되어 키가 이쪽으로 온다.
pub fn file_pane(
    window_view: &WorkWindow,
    pane: &FilePane,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let tab = pane.tabs.get(pane.selected);
    let untracked = tab.is_some_and(|tab| window_view.git.is_untracked(&tab.path));
    let body = match tab.and_then(|tab| window_view.file_views.get(&tab.key()).map(|content| (tab, content))) {
        Some((_, FileContent::Text(text))) if untracked => with_untracked_bar(text_view(text)).into_any_element(),
        Some((_, FileContent::Markdown(markdown))) if untracked => {
            with_untracked_bar(markdown_view(markdown)).into_any_element()
        }
        Some((_, FileContent::Text(text))) => text_view(text).into_any_element(),
        Some((_, FileContent::Markdown(markdown))) => markdown_view(markdown).into_any_element(),
        Some((_, FileContent::Image(view))) => view.clone().into_any_element(),
        Some((_, FileContent::Pdf(view))) => view.clone().into_any_element(),
        Some((_, FileContent::Diff(body))) => diff_view(body).into_any_element(),
        Some((tab, FileContent::Notice { reason, size })) => notice_view(
            &NoticeView {
                name: &paddock_core::file_kind::file_name_of(&tab.path),
                size: Some(format_size(*size)),
                message: notice_message(*reason),
                actions: &notice_actions(*reason),
            },
            &tab.path,
            cx,
        )
        .into_any_element(),
        Some((tab, FileContent::Failed(message))) => notice_view(
            &NoticeView { name: &paddock_core::file_kind::file_name_of(&tab.path), size: None, message, actions: &[] },
            &tab.path,
            cx,
        )
        .into_any_element(),
        Some((_, FileContent::Loading)) | None => div().size_full().into_any_element(),
    };
    div()
        .id("file-pane")
        .size_full()
        .bg(hsla(palette.terminal_bg))
        .overflow_hidden()
        .child(div().size_full().child(body))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.activate_file(window, cx)),
        )
}

/// 추적하지 않는 새 파일의 보기 위에 `Untracked` 한 줄을 붙인다.
fn with_untracked_bar(content: impl IntoElement) -> Div {
    let palette = colors();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .h(zpx(untracked_bar::HEIGHT))
                .px(zpx(untracked_bar::PADDING_X))
                .flex()
                .items_center()
                .text_size(zpx(text::SMALL))
                .text_color(hsla(palette.git_untracked))
                .child("Untracked"),
        )
        .child(div().flex_1().min_h_0().child(content))
}

/// 터미널 배치와 파일 칸 사이의 경계: 1px 선과 그 위에 겹친 8px 잡는 영역. 마우스를 올리거나 끌면 가운데 2px만 강조색이 된다.
/// 이 상자는 파일 칸의 왼쪽 가장자리에 놓이며 끌기는 창 단위로 처리한다(`WorkWindow::drag_divider`).
pub fn file_divider(
    window_view: &WorkWindow,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let active = window_view.file_divider_is_dragged();
    let line = |width: f32, color: u32| div().w(zpx(width)).h_full().bg(hsla(color));
    let base = line(split::LINE_WIDTH, if active { palette.accent } else { palette.border });
    let line = if active { line(split::HOVER_WIDTH, palette.accent) } else { base };
    let line = line.group_hover("file-divider", |style| style.bg(hsla(colors().accent)));
    div()
        .absolute()
        .group("file-divider")
        .top_0()
        .bottom_0()
        .left(relative(0.0))
        .ml(zpx(-split::HIT_WIDTH / 2.0))
        .w(zpx(split::HIT_WIDTH))
        .flex()
        .items_center()
        .justify_center()
        .cursor_col_resize()
        .child(line)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                // 경계를 잡는 누름이 이웃 칸으로 번지지 않게 한다.
                cx.stop_propagation();
                this.begin_file_divider_drag(f32::from(event.position.x), window, cx);
            }),
        )
}
