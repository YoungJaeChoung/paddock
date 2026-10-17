//! PDF 탭: 세로로 이어진 쪽 목록. 보이는 쪽과 그 앞뒤 한 쪽만 작업 스레드에서 그려 캐시에 두고, 나머지 쪽은 빈 종이로 둔다.
//!
//! 문서(`PdfDocument`)는 한 번 열어 쪽마다 작업 스레드에 빌려 준다. 그려 둔 쪽은 개수(3쪽)와 픽셀 합(1,600만)으로 묶여 있어 쪽 수가
//! 많은 파일이나 큰 배율에서도 메모리가 쪽 수에 비례해 늘지 않는다. 보기가 사라지거나 쪽을 캐시에서 내보낼 때 이미지는 `Graveyard`에 맡긴다.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui_kit::{
    Context, InteractiveElement, IntoElement, ObjectFit, ParentElement, Pixels, Render, RenderImage, ScrollHandle,
    ScrollWheelEvent, Size, StatefulInteractiveElement, Styled, StyledImage, Window, canvas, div, img, point, px,
};
use paddock_core::pdf_view::{
    CACHE_PAGES, PAGE_GAP, PageSlot, fit_percent, is_still_wanted, page_pixels, page_text, pages_to_drop,
    pages_to_hold, should_request, visible_pages, zoom_in, zoom_out,
};
use paddock_core::theme::pdf_view as size;

use super::image_load::Pixels as Bitmap;
use super::images::{Graveyard, render_image};
use super::pdf_render::PdfDocument;
use crate::app_log;
use crate::chrome::zpx;
use crate::look::{colors, zoom};
use crate::terminal::paint::hsla;

/// 그려 둔 쪽 하나.
struct DrawnPage {
    image: Arc<RenderImage>,
    /// 이 그림을 그릴 때 쓴 화면 배율(%, 화면 픽셀 기준).
    drawn_percent: u32,
    pixels: u64,
}

/// 열어 둔 PDF 하나.
pub struct PdfView {
    document: Arc<PdfDocument>,
    file_size: u64,
    /// 사용자가 고른 배율(%). None이면 칸 폭 맞춤이다.
    chosen_percent: Option<u32>,
    /// 마지막으로 잰 보기 영역의 크기(논리 px).
    pane: Size<Pixels>,
    scroll: ScrollHandle,
    pages: HashMap<usize, DrawnPage>,
    /// 그리지 못한 쪽과 그때 쓴 배율. 같은 배율에서는 다시 요청하지 않는다(배율이 바뀌면 한 번 더 시도한다).
    failed: HashMap<usize, u32>,
    /// 그리는 중인 (쪽, 그릴 배율).
    pending: HashSet<(usize, u32)>,
    /// 작업 스레드가 그리기 직전에 읽는 "아직 필요한 쪽"(그려 둘 쪽 목록, 그릴 배율). 창이 그릴 때마다 갱신한다.
    wanted: Arc<Mutex<(Vec<usize>, u32)>>,
    /// 마지막 그리기에서 보인 쪽(0부터).
    visible: Vec<usize>,
    /// 마지막 그리기에서 그려 두기로 한 쪽과 쪽을 그릴 때 쓴 배율. 이 밖의 결과는 도착해도 버린다.
    hold: Vec<usize>,
    drawing_percent: u32,
    /// 지금까지 쪽을 그린 횟수와 마지막으로 그리는 데 걸린 시간(ms).
    n_renders: usize,
    last_render_ms: u128,
    graveyard: Graveyard,
}

impl PdfView {
    pub fn new(
        document: Arc<PdfDocument>,
        file_size: u64,
        graveyard: Graveyard,
    ) -> Self {
        Self {
            document,
            file_size,
            chosen_percent: None,
            pane: Size::default(),
            scroll: ScrollHandle::new(),
            pages: HashMap::new(),
            failed: HashMap::new(),
            pending: HashSet::new(),
            wanted: Arc::new(Mutex::new((Vec::new(), 0))),
            visible: Vec::new(),
            hold: Vec::new(),
            drawing_percent: 0,
            n_renders: 0,
            last_render_ms: 0,
            graveyard,
        }
    }

    pub fn n_pages(&self) -> usize {
        self.document.n_pages()
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// 지금 보이는 쪽(0부터).
    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    /// 그려 둔 쪽(0부터, 오름차순).
    pub fn cached(&self) -> Vec<usize> {
        let mut cached: Vec<usize> = self.pages.keys().copied().collect();
        cached.sort_unstable();
        cached
    }

    /// 세로 스크롤 위치(논리 px, 아래로 갈수록 커진다). 자동 점검용이다.
    pub fn scroll_y(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    pub fn n_renders(&self) -> usize {
        self.n_renders
    }

    pub fn last_render_ms(&self) -> u128 {
        self.last_render_ms
    }

    /// 지금의 배율(%). 고르지 않았으면 첫 쪽의 폭을 칸 폭에 맞춘 값이다.
    pub fn percent(&self) -> u32 {
        self.chosen_percent.unwrap_or_else(|| {
            let first_width = self.document.sizes().first().map_or(0.0, |size| size.0);
            fit_percent(first_width, f32::from(self.pane.width) / zoom())
        })
    }

    pub fn is_fit(&self) -> bool {
        self.chosen_percent.is_none()
    }

    /// `+`: 한 단계 크게.
    pub fn zoom_in(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.chosen_percent = Some(zoom_in(self.percent()));
        cx.notify();
    }

    /// `−`: 한 단계 작게.
    pub fn zoom_out(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.chosen_percent = Some(zoom_out(self.percent()));
        cx.notify();
    }

    /// 칸 폭 맞춤으로 되돌린다.
    pub fn zoom_fit(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.chosen_percent = None;
        cx.notify();
    }

    /// `index`번째(0부터) 쪽의 윗부분이 보이는 영역 맨 위에 오게 스크롤한다.
    pub fn scroll_to_page(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let heights = self.page_heights();
        let top: f32 = PAGE_GAP + heights.iter().take(index).map(|height| height + PAGE_GAP).sum::<f32>();
        self.scroll.set_offset(point(px(0.0), px(-top * zoom())));
        cx.notify();
    }

    fn set_pane(
        &mut self,
        pane: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.pane != pane {
            self.pane = pane;
            cx.notify();
        }
    }

    /// 쪽마다 지금 배율에서 보이는 높이(`zoom` 배율을 곱하기 전, 논리 px).
    fn page_heights(&self) -> Vec<f32> {
        let percent = self.percent();
        self.document.sizes().iter().map(|size| page_pixels(*size, percent).1 as f32).collect()
    }

    /// 쪽을 그릴 때 쓰는 배율(%): 화면 배율과 창 배율을 반영해 화면 픽셀 하나가 그림 픽셀 하나가 되게 한다.
    fn drawing_percent(
        &self,
        device_scale: f32,
    ) -> u32 {
        ((self.percent() as f32 * zoom() * device_scale).round() as u32).max(1)
    }

    /// 쪽 하나를 작업 스레드에 그리게 한다. 작업이 차례가 됐을 때 그 쪽이 더는 필요 없으면(스크롤이 지나갔거나 배율이 바뀜) 그리지 않고 넘어간다.
    fn request(
        &mut self,
        page: usize,
        drawing_percent: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.insert((page, drawing_percent)) {
            let (document, wanted) = (self.document.clone(), self.wanted.clone());
            cx.spawn_in(window, async move |this, cx| {
                let started = Instant::now();
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        let still_wanted = {
                            let wanted = wanted.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                            is_still_wanted(page, drawing_percent, &wanted.0, wanted.1)
                        };
                        if still_wanted {
                            document.render(page, drawing_percent).map_or(Outcome::Failed, Outcome::Drawn)
                        } else {
                            Outcome::Skipped
                        }
                    })
                    .await;
                let elapsed = started.elapsed().as_millis();
                let _ =
                    this.update_in(cx, |this, _window, cx| this.finish(page, drawing_percent, outcome, elapsed, cx));
            })
            .detach();
        }
    }

    /// 그려 낸 쪽을 캐시에 넣는다. 그리지 못했으면 실패로 기억하고, 건너뛴 쪽은 아무것도 남기지 않는다.
    fn finish(
        &mut self,
        page: usize,
        drawing_percent: u32,
        outcome: Outcome,
        elapsed_ms: u128,
        cx: &mut Context<Self>,
    ) {
        self.pending.remove(&(page, drawing_percent));
        // 그리는 사이 배율이 바뀌었거나 쪽이 멀어졌으면 이 결과는 필요 없다.
        let wanted = is_still_wanted(page, drawing_percent, &self.hold, self.drawing_percent);
        match outcome {
            Outcome::Skipped => {}
            Outcome::Failed => {
                self.n_renders += 1;
                app_log::event(&format!("pdf page could not be drawn: page={} percent={drawing_percent}", page + 1));
                if wanted {
                    self.failed.insert(page, drawing_percent);
                }
            }
            Outcome::Drawn(bitmap) => {
                self.n_renders += 1;
                self.last_render_ms = elapsed_ms;
                app_log::event(&format!(
                    "pdf page rendered: page={} percent={drawing_percent} ms={elapsed_ms}",
                    page + 1
                ));
                let pixels = u64::from(bitmap.width) * u64::from(bitmap.height);
                let image = render_image(Bitmap { width: bitmap.width, height: bitmap.height, bgra: bitmap.bgra });
                match image {
                    Some(image) if wanted => {
                        self.failed.remove(&page);
                        if let Some(old) =
                            self.pages.insert(page, DrawnPage { image, drawn_percent: drawing_percent, pixels })
                        {
                            self.graveyard.bury(old.image);
                        }
                    }
                    Some(image) => self.graveyard.bury(image),
                    None if wanted => {
                        self.failed.insert(page, drawing_percent);
                    }
                    None => {}
                }
            }
        }
        cx.notify();
    }

    /// 디스크의 파일이 바뀌어 다시 읽은 문서로 바꾼다. 그려 둔 쪽은 모두 버리고(다음 그리기에서 다시 그린다) 배율과 스크롤 위치는 그대로 둔다.
    pub fn replace(
        &mut self,
        document: Arc<PdfDocument>,
        file_size: u64,
        cx: &mut Context<Self>,
    ) {
        for (_, page) in self.pages.drain() {
            self.graveyard.bury(page.image);
        }
        self.failed.clear();
        self.pending.clear();
        self.document = document;
        self.file_size = file_size;
        cx.notify();
    }
}

/// 작업 스레드가 쪽 하나를 다룬 결과.
enum Outcome {
    Drawn(super::pdf_render::PageBitmap),
    /// 그리지 못했다(렌더러가 쪽을 읽지 못함).
    Failed,
    /// 차례가 왔을 때 더는 필요 없어 그리지 않았다.
    Skipped,
}

impl Drop for PdfView {
    fn drop(&mut self) {
        for (_, page) in self.pages.drain() {
            self.graveyard.bury(page.image);
        }
    }
}

impl Render for PdfView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = colors();
        let zoom_factor = zoom();
        let device_scale = window.scale_factor();
        let percent = self.percent();
        let drawing_percent = self.drawing_percent(device_scale);
        let n_pages = self.document.n_pages();
        let sizes: Vec<(f32, f32)> = self
            .document
            .sizes()
            .iter()
            .map(|size| {
                let (width, height) = page_pixels(*size, percent);
                (width as f32, height as f32)
            })
            .collect();
        let heights: Vec<f32> = sizes.iter().map(|size| size.1).collect();
        let scroll_top = -f32::from(self.scroll.offset().y) / zoom_factor;
        let viewport = f32::from(self.pane.height) / zoom_factor;
        self.visible = visible_pages(scroll_top, viewport, &heights);
        // 그려 둘 쪽을 정하고, 필요 없어진 쪽은 내보낸 뒤, 빠진 쪽은 그리게 한다.
        let hold = pages_to_hold(&self.visible, n_pages, CACHE_PAGES);
        self.hold = hold.clone();
        self.drawing_percent = drawing_percent;
        *self.wanted.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = (hold.clone(), drawing_percent);
        self.failed.retain(|page, _| hold.contains(page));
        let cached: Vec<(usize, u64)> = self.pages.iter().map(|(page, drawn)| (*page, drawn.pixels)).collect();
        for page in pages_to_drop(&cached, &hold) {
            if let Some(old) = self.pages.remove(&page) {
                self.graveyard.bury(old.image);
            }
        }
        for page in hold {
            let slot = match (self.pages.get(&page), self.failed.get(&page)) {
                (Some(drawn), _) => Some(PageSlot::Drawn { percent: drawn.drawn_percent }),
                (None, Some(percent)) => Some(PageSlot::Failed { percent: *percent }),
                (None, None) => None,
            };
            if should_request(slot, drawing_percent, self.pending.contains(&(page, drawing_percent))) {
                self.request(page, drawing_percent, window, cx);
            }
        }
        let widest = sizes.iter().map(|size| size.0).fold(0.0_f32, f32::max);
        let entity = cx.entity();
        let first_visible = self.visible.first().copied().unwrap_or(0);
        let status = format!("{}  {}%", page_text(first_visible, n_pages), percent);
        let mut column = div()
            .min_w_full()
            .w(px((widest + 2.0 * PAGE_GAP) * zoom_factor))
            .py(zpx(PAGE_GAP))
            .flex()
            .flex_col()
            .items_center()
            .gap(zpx(PAGE_GAP));
        for (index, (width, height)) in sizes.iter().enumerate() {
            let paper = div()
                .flex_none()
                .w(px(width * zoom_factor))
                .h(px(height * zoom_factor))
                .bg(hsla(size::PAPER))
                .border(zpx(size::PAGE_BORDER))
                .border_color(hsla(palette.border));
            column = column.child(match (self.pages.get(&index), self.failed.contains_key(&index)) {
                (Some(drawn), _) => paper.child(img(drawn.image.clone()).size_full().object_fit(ObjectFit::Fill)),
                (None, true) => paper
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(zpx(size::STATUS_TEXT_SIZE))
                    .text_color(hsla(palette.fg_muted))
                    .child("Couldn't draw this page"),
                (None, false) => paper,
            });
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(palette.sidebar_bg))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _window, cx| entity.update(cx, |view, cx| view.set_pane(bounds.size, cx)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .id("pdf-scroll")
                            .size_full()
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .on_scroll_wheel(cx.listener(|_this, _event: &ScrollWheelEvent, _window, cx| cx.notify()))
                            .child(column),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .h(zpx(size::STATUS_HEIGHT))
                    .px(zpx(size::STATUS_PADDING_X))
                    .flex()
                    .items_center()
                    .gap(zpx(size::STATUS_GAP))
                    .border_t_1()
                    .border_color(hsla(palette.border))
                    .bg(hsla(palette.terminal_bg))
                    .text_size(zpx(size::STATUS_TEXT_SIZE))
                    .text_color(hsla(palette.fg_muted))
                    .child(status),
            )
    }
}
