//! 파일 칸: 선택한 파일 탭의 내용(텍스트 보기 또는 안내 탭)과 터미널 배치 사이의 경계.
//!
//! 칸은 묶음 본문의 오른쪽 열이다. 위에는 이름표 줄을 따로 두지 않는다 — 파일 탭은 위쪽 탭 줄의 마지막 열에 놓인다.
use std::rc::Rc;

use gpui_kit::component::diff::DiffState;
use gpui_kit::component::input::EditorState;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Styled, div, relative,
};
#[cfg(feature = "automation")]
use paddock_core::file_kind::FileKind;
use paddock_core::file_kind::format_size;
use paddock_core::file_pane::{FilePane, FileTab};
use paddock_core::theme::{split, text, untracked_bar};

use super::editing_view::ChangedHandler;
use super::image_view::ImageView;
use super::markdown_view::{MarkdownBody, MarkdownModel, markdown_model_view};
use super::notice_view::{NoticeHandler, NoticeView, notice_view};
use super::open::{NoticeReason, notice_actions, notice_message};
use super::pdf_view::PdfView;
use super::text_view::{TextBody, text_view_of};
use crate::chrome::zpx;
use crate::look::colors;
use crate::scm::diff_view::{DiffBody, diff_view_of};
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
    /// 내용이 그대로인지 파일 해시와 함께 비교할 수 있게 한다. 해시는 편집기 전체 글자를 훑어 비싸므로 `detailed`일 때만 구하고, 아니면 `-`다.
    #[cfg(feature = "automation")]
    pub fn summary(
        &self,
        kind: FileKind,
        detailed: bool,
        cx: &App,
    ) -> String {
        let hash_text = |text: &str| if detailed { format!("{:016x}", text_hash(text)) } else { "-".to_string() };
        match self {
            Self::Loading => format!("kind={} state=loading", kind.name()),
            Self::Text(body) => format!(
                "kind={} state=text lines={} language={} size={} content-hash={} scroll-y={:.0} line={} readonly={}",
                kind.name(),
                body.n_lines,
                body.language,
                body.size,
                hash_text(&body.editor.read(cx).value()),
                f32::from(body.editor.read(cx).scroll_offset().y),
                body.editor.read(cx).cursor_position().line + 1,
                if body.editable { "no" } else { "yes" }
            ),
            Self::Markdown(body) => {
                let (n_loaded, n_images) = body.image_counts();
                let (n_lines, value) = body.source_summary(detailed, cx);
                format!(
                    "kind={} state=markdown view={} lines={} size={} content-hash={} preview-width={:.0} images={n_loaded}/{n_images} scroll-y={:.0} readonly={}",
                    kind.name(),
                    body.view.name(),
                    n_lines,
                    body.file_size(),
                    hash_text(&value),
                    body.preview_width(),
                    body.source_scroll_y(cx),
                    if body.is_editable() { "no" } else { "yes" }
                )
            }
            Self::Image(view) => {
                let view = view.read(cx);
                let (shown_width, shown_height) = view.shown();
                format!(
                    "kind={} state=image size={} decoded={} bytes={} zoom={} full={} drawn-tiles={} shown={:.0}x{:.0}",
                    kind.name(),
                    paddock_core::image_view::size_text(view.original().0, view.original().1).replace(" × ", "x"),
                    paddock_core::image_view::size_text(view.decoded().0, view.decoded().1).replace(" × ", "x"),
                    view.file_size(),
                    view.zoom().name(),
                    if view.has_full() { "yes" } else { "no" },
                    view.n_drawn_tiles(),
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

/// 파일 탭 하나의 내용을 그리는 데 필요한 것의 사본(핸들·글자·표시). 본 창(`WorkWindow`)이 쥔 내용을 빌리지 않고 그릴 수 있어서, 본 창을
/// 그리지 않는 창(파일 분리 창)이 그릴 때 쓴다 — 창이 그리는 동안 본 창 엔티티를 건드리면 gpui가 그 엔티티의 "현재 창"을 그 창으로 바꿔
/// 본 창의 비동기 이어받기(`update_in`)가 엉뚱한 창으로 간다.
#[derive(Clone)]
pub struct FileBodyModel {
    /// 탭의 파일 경로(안내 탭 단추·디스크 변경 띠가 쓴다).
    path: Option<String>,
    content: ContentModel,
    /// 추적하지 않는 새 파일이다(`Untracked` 줄).
    untracked: bool,
    /// 고친 내용이 있는데 디스크가 바뀌었다(`Reload`·`Keep mine` 띠).
    changed_on_disk: bool,
}

/// 파일 내용의 종류별 사본.
#[derive(Clone)]
enum ContentModel {
    /// 읽는 중이거나 내용이 없다.
    Empty,
    Text {
        editor: Entity<EditorState>,
        editable: bool,
    },
    Markdown(MarkdownModel),
    Image(Entity<ImageView>),
    Pdf(Entity<PdfView>),
    Diff(Entity<DiffState>),
    Notice {
        reason: NoticeReason,
        size: u64,
    },
    Failed(String),
}

impl FileBodyModel {
    /// `tab`의 내용 사본을 만든다. 탭이 없으면 빈 칸이다.
    pub fn of(
        window_view: &WorkWindow,
        tab: Option<&FileTab>,
    ) -> Self {
        let content = match tab.and_then(|tab| window_view.file_views.get(&tab.key())) {
            Some(FileContent::Text(body)) => {
                ContentModel::Text { editor: body.editor.clone(), editable: body.editable }
            }
            Some(FileContent::Markdown(body)) => ContentModel::Markdown(body.model()),
            Some(FileContent::Image(view)) => ContentModel::Image(view.clone()),
            Some(FileContent::Pdf(view)) => ContentModel::Pdf(view.clone()),
            Some(FileContent::Diff(body)) => ContentModel::Diff(body.state.clone()),
            Some(FileContent::Notice { reason, size }) => ContentModel::Notice { reason: *reason, size: *size },
            Some(FileContent::Failed(message)) => ContentModel::Failed(message.clone()),
            Some(FileContent::Loading) | None => ContentModel::Empty,
        };
        Self {
            path: tab.map(|tab| tab.path.clone()),
            content,
            untracked: tab.is_some_and(|tab| window_view.git.is_untracked(&tab.path)),
            changed_on_disk: tab.is_some_and(|tab| window_view.disk_changed.contains(&tab.path)),
        }
    }
}

/// 파일 내용 위 단추(안내 탭 단추·디스크 변경 띠)가 부를 동작. 내용을 그리는 보기(`T`)가 정한다.
pub struct BodyHandlers<T> {
    pub on_notice: NoticeHandler<T>,
    pub on_reload: ChangedHandler<T>,
    pub on_keep: ChangedHandler<T>,
}

/// 본 창이 파일 칸에 그릴 때의 동작: 본 창의 메서드를 그대로 부른다.
fn main_handlers() -> BodyHandlers<WorkWindow> {
    BodyHandlers {
        on_notice: Rc::new(|this, action, path, window, cx| this.run_notice_action(action, path, window, cx)),
        on_reload: Rc::new(|this, path, window, cx| {
            this.reload_from_disk(path, window, cx);
            this.focus_selected(window, cx);
        }),
        on_keep: Rc::new(|this, path, window, cx| {
            this.keep_mine(path, cx);
            this.focus_selected(window, cx);
        }),
    }
}

/// 파일 칸 안쪽: 선택한 탭의 내용. 칸을 누르면 파일 탭이 활성이 되어 키가 이쪽으로 온다.
pub fn file_pane(
    window_view: &WorkWindow,
    pane: &FilePane,
    cx: &mut Context<WorkWindow>,
) -> gpui_kit::Stateful<Div> {
    let palette = colors();
    let model = FileBodyModel::of(window_view, pane.tabs.get(pane.selected));
    let body = file_body_element(&model, cx, &main_handlers());
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

/// 파일 탭 하나의 내용 사본을 요소로 그린다(칸 안쪽과 파일 분리 창이 같이 쓴다): 읽은 내용, 추적하지 않는 새 파일 줄, 디스크 변경 띠.
/// 안내 탭 단추·띠 단추는 `handlers`를 부른다.
pub fn file_body_element<T: 'static>(
    model: &FileBodyModel,
    cx: &mut Context<T>,
    handlers: &BodyHandlers<T>,
) -> AnyElement {
    let path = model.path.clone().unwrap_or_default();
    let body = match &model.content {
        ContentModel::Text { editor, editable } if model.untracked => {
            with_untracked_bar(text_view_of(editor, *editable)).into_any_element()
        }
        ContentModel::Markdown(markdown) if model.untracked => {
            with_untracked_bar(markdown_model_view(markdown)).into_any_element()
        }
        ContentModel::Text { editor, editable } => text_view_of(editor, *editable).into_any_element(),
        ContentModel::Markdown(markdown) => markdown_model_view(markdown).into_any_element(),
        ContentModel::Image(view) => view.clone().into_any_element(),
        ContentModel::Pdf(view) => view.clone().into_any_element(),
        ContentModel::Diff(state) => diff_view_of(state).into_any_element(),
        ContentModel::Notice { reason, size } => notice_view(
            &NoticeView {
                name: &paddock_core::file_kind::file_name_of(&path),
                size: Some(format_size(*size)),
                message: notice_message(*reason),
                actions: &notice_actions(*reason),
            },
            &path,
            cx,
            &handlers.on_notice,
        )
        .into_any_element(),
        ContentModel::Failed(message) => notice_view(
            &NoticeView { name: &paddock_core::file_kind::file_name_of(&path), size: None, message, actions: &[] },
            &path,
            cx,
            &handlers.on_notice,
        )
        .into_any_element(),
        ContentModel::Empty => div().size_full().into_any_element(),
    };
    // 고친 내용이 있는데 디스크가 바뀐 파일은 내용 위에 `Reload`·`Keep mine` 띠를 붙인다.
    if model.changed_on_disk {
        crate::files::editing_view::with_changed_bar(body, &path, cx, &handlers.on_reload, &handlers.on_keep)
            .into_any_element()
    } else {
        body
    }
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
