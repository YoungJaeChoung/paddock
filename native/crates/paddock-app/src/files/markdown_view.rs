//! Markdown 탭: 원문(읽기 전용 텍스트 보기)·미리보기·나란히 보기.
//!
//! 미리보기는 네트워크에 닿지 않는다. 원격 이미지는 내려받지 않고(빈 자리로 둔다), 본문 속 로컬 이미지는 문서 폴더 기준으로 읽어
//! 작게 줄여 올리며, 링크는 사용자가 누를 때만 그리고 웹·메일 주소만 바깥 프로그램으로 연다. 원문 속 HTML 태그는 글자로 보인다.
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::base::{TextView, TextViewState, TextViewStyle};
use gpui_kit::{
    AppContext, ClickEvent, Context, Entity, FontWeight, HighlightStyle, ImageSource, InteractiveElement, IntoElement,
    MouseButton, ParentElement, RenderImage, ScrollHandle, SharedString, SharedUri, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, canvas, div, px, rems,
};
use paddock_core::markdown_view::{Segment, View, external_link, resolve_image_path, split_blockquotes};
use paddock_core::theme::{Mode, markdown as size};

use super::image_load::{RasterLoad, load_document_image};
use super::images::{Graveyard, render_image};
use super::text_view::{TextBody, build_text, text_view};
use crate::app_log;
use crate::chrome::zpx;
use crate::look::{colors, mode, scaled};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 미리보기 본문 속 이미지 하나의 상태.
enum Slot {
    Loading,
    Ready(Arc<RenderImage>),
    /// 읽지 못한 이미지(빈 자리로 둔다).
    Failed,
}

/// 문서 속 로컬 이미지를 모아 둔 곳. 이미지마다 작업 스레드에서 한 번만 읽는다.
pub struct DocumentImages {
    slots: Mutex<HashMap<String, Slot>>,
    /// 원격 주소나 열 수 없는 이미지 자리에 그리는 1×1 투명 그림.
    blank: Arc<RenderImage>,
    /// 이미지를 읽을 수 있는 폴더: 문서가 속한 작업 폴더(없으면 문서가 있는 폴더). 이 밖의 경로는 읽지 않는다.
    root_dir: String,
    graveyard: Graveyard,
}

impl DocumentImages {
    fn new(
        graveyard: Graveyard,
        root_dir: String,
    ) -> Arc<Self> {
        let blank = render_image(super::image_load::Pixels { width: 1, height: 1, bgra: vec![0; 4] })
            .expect("a 1x1 pixel image always builds");
        Arc::new(Self { slots: Mutex::new(HashMap::new()), blank, root_dir, graveyard })
    }

    /// 읽어 둔 이미지를 모두 버린다. 문서가 바뀌면 다음 그리기에서 이미지를 다시 읽는다.
    fn clear(&self) {
        let slots = std::mem::take(&mut *self.slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        for (_, slot) in slots {
            if let Slot::Ready(image) = slot {
                self.graveyard.bury(image);
            }
        }
    }

    /// 읽어 낸 이미지 수와 전체 수.
    pub fn counts(&self) -> (usize, usize) {
        let slots = self.slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (slots.values().filter(|slot| matches!(slot, Slot::Ready(_))).count(), slots.len())
    }

    /// 본문 속 이미지 주소 하나를 그릴 수 있는 이미지 원천으로 바꾼다.
    ///
    /// 로컬 이미지 파일이 아닌 주소(원격·`file:`·공유 폴더·확장자가 이미지가 아님)는 빈 그림이다. 로컬 파일은 처음 그릴 때
    /// 작업 스레드에 읽게 하고 읽는 동안은 아직 없는 이미지로 둔다.
    fn source_for(
        self: &Arc<Self>,
        base_dir: &str,
        url: &str,
    ) -> ImageSource {
        match resolve_image_path(base_dir, &self.root_dir, url) {
            None => ImageSource::Render(self.blank.clone()),
            Some(path) => {
                let images = self.clone();
                ImageSource::Custom(Arc::new(move |window, cx| {
                    let state = {
                        let mut slots = images.slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        match slots.get(&path) {
                            Some(Slot::Ready(image)) => Some(Some(image.clone())),
                            Some(Slot::Failed) => Some(None),
                            Some(Slot::Loading) => None,
                            None => {
                                slots.insert(path.clone(), Slot::Loading);
                                let (images, path) = (images.clone(), path.clone());
                                let read = cx.background_executor().spawn({
                                    let path = path.clone();
                                    async move { load_document_image(&path) }
                                });
                                window
                                    .spawn(cx, async move |cx| {
                                        let slot = match read.await {
                                            RasterLoad::Ready { pixels, .. } => {
                                                render_image(pixels).map_or(Slot::Failed, Slot::Ready)
                                            }
                                            RasterLoad::TooLarge { .. } | RasterLoad::Failed(_) => Slot::Failed,
                                        };
                                        images
                                            .slots
                                            .lock()
                                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                                            .insert(path, slot);
                                        let _ = cx.update(|window, _| window.refresh());
                                    })
                                    .detach();
                                None
                            }
                        }
                    };
                    state.map(|image| Ok(image.unwrap_or_else(|| images.blank.clone())))
                }))
            }
        }
    }
}

impl Drop for DocumentImages {
    fn drop(&mut self) {
        self.clear();
        self.graveyard.bury(self.blank.clone());
    }
}

/// 미리보기 조각 하나: 렌더러 상태와 인용 블록 여부.
struct PreviewPart {
    state: Entity<TextViewState>,
    quote: bool,
}

/// 원문을 조각 글자와 인용 여부 목록으로 나눈다.
fn part_texts(text: &str) -> Vec<(String, bool)> {
    split_blockquotes(text)
        .into_iter()
        .map(|segment| match segment {
            Segment::Body(text) => (text, false),
            Segment::Quote(text) => (text, true),
        })
        .collect()
}

/// 열어 둔 Markdown 하나.
pub struct MarkdownBody {
    /// 원문 보기. 미리보기만 보는 동안에는 만들지 않고(편집기 메모리를 아끼려고) 원문 글자만 쥔다. `File`·`Both`를 처음 고를 때 만든다.
    pub source: Option<TextBody>,
    /// 원문 보기를 아직 만들지 않았을 때의 원문 글자.
    pending_text: Option<String>,
    /// 파일 경로와 크기. 원문 보기를 늦게 만들 때 쓴다.
    path: String,
    file_size: u64,
    /// 미리보기 조각들. 인용 블록이 없으면 조각 하나(스크롤은 부품이 맡는다), 있으면 본문·인용 조각이 번갈아 있고 `column_scroll`이 스크롤한다.
    parts: Vec<PreviewPart>,
    column_scroll: ScrollHandle,
    base_dir: String,
    images: Arc<DocumentImages>,
    /// 지금 보이는 방식. 키를 받을 자리를 정하는 데도 쓴다.
    pub view: View,
    /// 마지막으로 그린 미리보기 본문 폭(논리 px). 자동 점검이 읽는다.
    preview_width: Rc<Cell<f32>>,
}

impl MarkdownBody {
    /// 디스크의 파일이 바뀌어 다시 읽은 글자로 바꾼다. 미리보기·원문의 스크롤 위치는 그대로 두고, 본문 속 이미지는 다시 읽는다.
    pub fn replace(
        &mut self,
        text: String,
        size: u64,
        window: &mut Window,
        cx: &mut Context<WorkWindow>,
    ) {
        self.file_size = size;
        self.set_preview_text(&text, cx);
        self.images.clear();
        match self.source.as_mut() {
            Some(source) => source.replace(text, size, window, cx),
            None => self.pending_text = Some(text),
        }
    }

    /// 미리보기 조각들의 글자를 바꾼다. 조각 구성(개수·인용 여부)이 같으면 각 조각의 글자만 바꿔 스크롤 위치를 지키고, 달라지면 조각을 새로 만든다.
    fn set_preview_text(
        &mut self,
        text: &str,
        cx: &mut Context<WorkWindow>,
    ) {
        let texts = part_texts(text);
        let same_shape = texts.len() == self.parts.len()
            && texts.iter().zip(&self.parts).all(|((_, quote), part)| *quote == part.quote);
        if same_shape {
            for ((text, _), part) in texts.iter().zip(&self.parts) {
                part.state.update(cx, |state, cx| state.set_text(text, cx));
            }
        } else {
            self.parts = texts
                .into_iter()
                .map(|(text, quote)| PreviewPart { state: cx.new(|cx| TextViewState::markdown(&text, cx)), quote })
                .collect();
        }
    }

    /// 원문 보기가 없으면 만든다. 원문을 보여 주는 방식(`File`·`Both`)으로 바꾸기 직전에 부른다.
    pub fn ensure_source(
        &mut self,
        window: &mut Window,
        cx: &mut Context<WorkWindow>,
    ) {
        if self.source.is_none()
            && let Some(text) = self.pending_text.take()
        {
            self.source = Some(build_text(text, self.file_size, &self.path, window, cx));
        }
    }

    /// 원문의 줄 수·글자 해시(자동 점검용). 편집기가 있으면 편집기 안의 글자를, 없으면 읽어 둔 글자를 본다.
    pub fn source_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> (usize, String) {
        match (&self.source, &self.pending_text) {
            (Some(source), _) => (source.n_lines, source.editor.read(cx).value().to_string()),
            (None, Some(text)) => (text.lines().count().max(1), text.clone()),
            (None, None) => (0, String::new()),
        }
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// 원문 편집기의 세로 스크롤 위치(자동 점검용). 편집기가 없으면 0이다.
    pub fn source_scroll_y(
        &self,
        cx: &gpui_kit::App,
    ) -> f32 {
        self.source.as_ref().map_or(0.0, |source| f32::from(source.editor.read(cx).scroll_offset().y))
    }

    /// 미리보기 본문 폭(논리 px). 아직 그리지 않았으면 0이다.
    pub fn preview_width(&self) -> f32 {
        self.preview_width.get()
    }

    /// 읽어 낸 문서 속 이미지 수와 전체 수.
    pub fn image_counts(&self) -> (usize, usize) {
        self.images.counts()
    }

    /// 키 입력을 받을 자리: 원문이 보이면 편집기, 미리보기만 보이면 미리보기(선택·복사용).
    pub fn focus_handle(
        &self,
        cx: &gpui_kit::App,
    ) -> gpui_kit::FocusHandle {
        match (&self.source, self.view) {
            (Some(source), View::Source | View::Both) => gpui_kit::Focusable::focus_handle(&source.editor, cx),
            _ => self.parts[0].state.read(cx).focus_handle().clone(),
        }
    }
}

/// Markdown 파일이 어디에 있는지: 파일 경로와 본문 속 이미지를 읽을 수 있는 작업 폴더.
#[derive(Clone, Copy)]
pub struct Origin<'a> {
    pub path: &'a str,
    pub root_dir: &'a str,
}

/// 읽은 Markdown 글자로 원문 보기와 미리보기를 만든다.
pub fn build_markdown(
    text: String,
    size: u64,
    origin: Origin<'_>,
    view: View,
    graveyard: Graveyard,
    window: &mut Window,
    cx: &mut Context<WorkWindow>,
) -> MarkdownBody {
    let Origin { path, root_dir } = origin;
    let parts: Vec<PreviewPart> = part_texts(&text)
        .into_iter()
        .map(|(text, quote)| PreviewPart { state: cx.new(|cx| TextViewState::markdown(&text, cx)), quote })
        .collect();
    // 처음부터 원문을 보이는 방식이면 편집기를 바로 만들고, 미리보기만 보면 글자만 쥐어 둔다.
    let (source, pending_text) = match view {
        View::Preview => (None, Some(text)),
        View::Source | View::Both => (Some(build_text(text, size, path, window, cx)), None),
    };
    let base_dir = path.rsplit_once(['/', '\\']).map_or(String::new(), |(dir, _)| dir.to_string());
    MarkdownBody {
        source,
        pending_text,
        path: path.to_string(),
        file_size: size,
        parts,
        column_scroll: ScrollHandle::new(),
        base_dir,
        images: DocumentImages::new(graveyard, root_dir.to_string()),
        view,
        preview_width: Rc::new(Cell::new(0.0)),
    }
}

/// 미리보기 글꼴·색·간격: 색은 앱 색 묶음에서, 크기는 표준 값에서 가져온다.
fn preview_style() -> TextViewStyle {
    let palette = colors();
    let base = scaled(size::TEXT_SIZE);
    TextViewStyle::default()
        .with_foreground(hsla(palette.terminal_fg))
        .with_muted_foreground(hsla(palette.fg_muted))
        .with_link(hsla(palette.markdown_link))
        .with_selection(hsla(palette.terminal_selection))
        .with_code_background(hsla(palette.markdown_code_bg))
        .with_border(hsla(palette.border))
        .with_paragraph_gap(rems(size::PARAGRAPH_GAP))
        .with_heading(move |level| {
            let scale = size::HEADING_SCALE[usize::from(level.clamp(1, 6)) - 1];
            StyleRefinement::default().text_size(px(base * scale)).font_weight(FontWeight::SEMIBOLD)
        })
        .with_code_block({
            let mut style = StyleRefinement::default();
            style = style.p(zpx(size::CODE_BLOCK_PADDING)).rounded(zpx(size::CODE_BLOCK_RADIUS));
            style
        })
        .with_inline_code(HighlightStyle {
            background_color: Some(hsla(palette.markdown_code_bg)),
            ..Default::default()
        })
        .with_dark(matches!(mode(), Mode::Dark))
}

/// 미리보기 쪽 그리기: 본문은 읽을 수 있는 폭(`CONTENT_MAX_WIDTH`)으로 가운데에 놓인다.
fn preview_column(body: &MarkdownBody) -> impl IntoElement {
    let palette = colors();
    let measured = body.preview_width.clone();
    let make_view = |part: &PreviewPart| {
        let (images, base_dir) = (body.images.clone(), body.base_dir.clone());
        TextView::new(&part.state)
            .style(preview_style())
            .on_link_click(|url: &SharedString, event: &ClickEvent, _window, _cx| {
                if is_primary_click(event) {
                    open_link(url);
                }
            })
            .image_source(move |uri: &SharedUri| images.source_for(&base_dir, uri.as_ref()))
    };
    // 조각이 하나면 부품이 스크롤을 맡고, 인용이 있어 여러 조각이면 조각들을 세로로 쌓아 한 상자가 스크롤한다.
    let content = if body.parts.len() == 1 {
        make_view(&body.parts[0]).scrollable(true).into_any_element()
    } else {
        let mut column = div().w_full().flex().flex_col().gap(rems(size::PARAGRAPH_GAP)).pb(zpx(size::PADDING_Y));
        for part in &body.parts {
            let view = make_view(part);
            column = column.child(if part.quote {
                div()
                    .w_full()
                    .p(zpx(size::QUOTE_PADDING))
                    .rounded(zpx(size::CODE_BLOCK_RADIUS))
                    .border_1()
                    .border_color(hsla(palette.border))
                    .bg(hsla(palette.markdown_quote_bg))
                    .text_color(hsla(palette.fg_muted))
                    .child(view)
                    .into_any_element()
            } else {
                div().w_full().child(view).into_any_element()
            });
        }
        div()
            .id("markdown-preview-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&body.column_scroll)
            .child(column)
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .justify_center()
        .bg(hsla(palette.terminal_bg))
        .text_size(zpx(size::TEXT_SIZE))
        .line_height(gpui_kit::relative(size::LINE_HEIGHT))
        .child(
            div()
                .id("markdown-preview")
                .h_full()
                .w_full()
                .max_w(zpx(size::CONTENT_MAX_WIDTH + 2.0 * size::PADDING_X))
                .px(zpx(size::PADDING_X))
                .pt(zpx(size::PADDING_Y))
                .child(
                    // 본문 상자(좌우 패딩 안쪽)의 폭을 잰다.
                    div()
                        .relative()
                        .size_full()
                        .child(
                            canvas(
                                move |bounds, _window, _cx| measured.set(f32::from(bounds.size.width)),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(content),
                ),
        )
}

/// Markdown 탭 그리기. `Both`는 왼쪽 원문, 오른쪽 미리보기 두 열이다.
pub fn markdown_view(body: &MarkdownBody) -> impl IntoElement {
    let palette = colors();
    match (body.view, &body.source) {
        (View::Source, Some(source)) => text_view(source).into_any_element(),
        (View::Both, Some(source)) => div()
            .size_full()
            .flex()
            .flex_row()
            .child(div().flex_1().min_w_0().h_full().child(text_view(source)))
            .child(div().flex_none().w(zpx(1.0)).h_full().bg(hsla(palette.border)))
            .child(div().flex_1().min_w_0().h_full().child(preview_column(body)))
            .into_any_element(),
        // 원문이 아직 없으면(곧 만들어진다) 미리보기를 보인다.
        _ => preview_column(body).into_any_element(),
    }
}

/// 링크 열기를 일으키는 누름인지: 왼쪽·가운데 단추, 키보드(Enter), 터치만이다. 오른쪽 단추나 보조 단추는 열지 않는다.
fn is_primary_click(event: &ClickEvent) -> bool {
    match event {
        ClickEvent::Mouse(click) => matches!(click.up.button, MouseButton::Left | MouseButton::Middle),
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(click) => !click.long_press,
    }
}

/// 링크를 사용자가 눌렀을 때: 웹·메일 주소만 바깥 프로그램으로 연다. 그 밖의 주소는 열지 않는다.
fn open_link(url: &str) {
    match external_link(url) {
        Some(target) => {
            app_log::event("a markdown link was opened in another program");
            super::open::open_link_externally(&target);
        }
        None => app_log::event("a markdown link was ignored: not a web or mail address"),
    }
}
