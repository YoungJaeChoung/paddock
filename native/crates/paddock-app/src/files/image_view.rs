//! 이미지 탭: 칸에 맞춰 보이는 그림(더블클릭하면 1:1)과 아래 상태 줄(가로 × 세로 · 파일 크기 · 보기 방식).
//!
//! 그림 픽셀은 이 보기가 쥔 하나뿐이다. 탭을 닫아 보기가 사라지면 `Graveyard`에 맡겨 창이 그래픽 메모리까지 반납한다.
use std::sync::Arc;

use gpui_kit::{
    Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement, Pixels, Render,
    RenderImage, ScrollHandle, Size, StatefulInteractiveElement, Styled, StyledImage, Window, canvas, div, img, px,
};
use paddock_core::file_kind::format_size;
use paddock_core::image_view::{Zoom, decode_size, display_size, size_text};
use paddock_core::theme::image_view as size;

use super::image_load::{RasterLoad, load_raster};
use super::images::{Graveyard, render_image};
use crate::chrome::zpx;
use crate::look::{colors, scaled};
use crate::terminal::paint::hsla;

/// 열어 둔 이미지 하나.
///
/// 칸에 맞춰 보는 동안(`Fit`)은 칸 크기로 줄여 올린 그림(`fit_image`)만 쥔다. 1:1로 바꾸면 원본 크기(올릴 수 있는 상한까지)로 작업 스레드에서
/// 다시 읽어(`full_image`) 보여 주고, 맞춤으로 돌아오면 그 그림을 버린다.
pub struct ImageView {
    path: String,
    fit_image: Arc<RenderImage>,
    /// 1:1로 보는 동안만 쥐는 원본 크기 그림.
    full_image: Option<Arc<RenderImage>>,
    /// 원본 크기 그림을 읽는 중이다.
    loading_full: bool,
    /// 읽기 요청마다 올리는 번호. 늦게 도착한 결과를 알아보는 데 쓴다.
    generation: u64,
    /// 파일 속 이미지의 가로세로(올린 픽셀이 줄어 있어도 이 크기로 보인다).
    original: (u32, u32),
    /// 칸에 맞춰 올린 그림의 가로세로.
    decoded: (u32, u32),
    file_size: u64,
    zoom: Zoom,
    /// 마지막으로 잰 보기 영역의 크기(논리 px).
    pane: Size<Pixels>,
    /// 마지막으로 그린 창의 화면 배율(논리 px 하나가 화면 픽셀 몇 개인지). 1:1은 이미지 픽셀 하나가 화면 픽셀 하나다.
    device_scale: f32,
    scroll: ScrollHandle,
    graveyard: Graveyard,
}

impl ImageView {
    pub fn new(
        path: String,
        image: Arc<RenderImage>,
        original: (u32, u32),
        file_size: u64,
        graveyard: Graveyard,
    ) -> Self {
        let decoded = Self::dims_of(&image);
        Self {
            path,
            fit_image: image,
            full_image: None,
            loading_full: false,
            generation: 0,
            original,
            decoded,
            file_size,
            zoom: Zoom::Fit,
            pane: Size::default(),
            device_scale: 1.0,
            scroll: ScrollHandle::new(),
            graveyard,
        }
    }

    fn dims_of(image: &RenderImage) -> (u32, u32) {
        let dims = image.size(0);
        (u32::try_from(i32::from(dims.width)).unwrap_or(1), u32::try_from(i32::from(dims.height)).unwrap_or(1))
    }

    /// 디스크의 파일이 바뀌어 다시 읽은 그림으로 바꾼다. 보기 방식(맞춤·1:1)과 스크롤 위치는 그대로 두고, 1:1이면 원본 크기 그림을 다시 읽는다.
    pub fn replace(
        &mut self,
        image: Arc<RenderImage>,
        original: (u32, u32),
        file_size: u64,
        cx: &mut Context<Self>,
    ) {
        self.graveyard.bury(std::mem::replace(&mut self.fit_image, image));
        if let Some(full) = self.full_image.take() {
            self.graveyard.bury(full);
        }
        self.generation += 1;
        self.loading_full = false;
        self.original = original;
        self.decoded = Self::dims_of(&self.fit_image);
        self.file_size = file_size;
        if self.zoom == Zoom::Actual {
            self.load_full(cx);
        }
        cx.notify();
    }

    pub fn original(&self) -> (u32, u32) {
        self.original
    }

    /// 지금 쥐고 있는 그림의 가로세로(원본 크기 그림이 있으면 그 크기).
    pub fn decoded(&self) -> (u32, u32) {
        self.full_image.as_ref().map_or(self.decoded, |full| Self::dims_of(full))
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    /// 원본 크기 그림을 쥐고 있는지.
    pub fn has_full(&self) -> bool {
        self.full_image.is_some()
    }

    /// 지금 칸에서 그림이 차지하는 크기(논리 px). 칸 크기를 아직 모르면 (0, 0)이다.
    pub fn shown(&self) -> (f32, f32) {
        let (width, height) = self.available();
        if width <= 0.0 || height <= 0.0 {
            (0.0, 0.0)
        } else {
            let (shown_width, shown_height) = display_size(self.original, (width, height), self.zoom);
            match self.zoom {
                Zoom::Fit => (shown_width, shown_height),
                Zoom::Actual => (shown_width / self.device_scale, shown_height / self.device_scale),
            }
        }
    }

    /// 여백을 뺀, 그림을 놓을 수 있는 칸 크기(논리 px).
    fn available(&self) -> (f32, f32) {
        let padding = 2.0 * scaled(size::PADDING);
        (f32::from(self.pane.width) - padding, f32::from(self.pane.height) - padding)
    }

    /// 맞춤 ↔ 1:1을 바꾼다(더블클릭). 1:1이 되면 원본 크기 그림을 읽고, 맞춤으로 돌아오면 버린다.
    pub fn toggle_zoom(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.zoom = self.zoom.toggled();
        match self.zoom {
            Zoom::Actual => self.load_full(cx),
            Zoom::Fit => {
                self.generation += 1;
                self.loading_full = false;
                if let Some(full) = self.full_image.take() {
                    self.graveyard.bury(full);
                }
            }
        }
        cx.notify();
    }

    /// 칸에 맞춰 올린 그림이 원본 크기와 다르면 원본 크기 그림을 작업 스레드에서 읽는다.
    fn load_full(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.decoded != decode_size(self.original.0, self.original.1)
            && !self.path.to_lowercase().ends_with(".svg")
            && !self.loading_full
        {
            self.loading_full = true;
            let (path, generation) = (self.path.clone(), self.generation);
            cx.spawn(async move |this, cx| {
                let loaded = cx.background_executor().spawn(async move { load_raster(&path) }).await;
                let _ = this.update(cx, |this, cx| this.finish_full(generation, loaded, cx));
            })
            .detach();
        }
    }

    fn finish_full(
        &mut self,
        generation: u64,
        loaded: RasterLoad,
        cx: &mut Context<Self>,
    ) {
        if generation == self.generation && self.zoom == Zoom::Actual {
            self.loading_full = false;
            if let RasterLoad::Ready { pixels, .. } = loaded
                && let Some(image) = render_image(pixels)
                && let Some(old) = self.full_image.replace(image)
            {
                self.graveyard.bury(old);
            }
            cx.notify();
        }
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
}

impl Drop for ImageView {
    fn drop(&mut self) {
        self.graveyard.bury(self.fit_image.clone());
        if let Some(full) = self.full_image.take() {
            self.graveyard.bury(full);
        }
    }
}

impl Render for ImageView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = colors();
        self.device_scale = window.scale_factor();
        let (width, height) = self.shown();
        let entity = cx.entity();
        let status = format!(
            "{} · {} · {}",
            size_text(self.original.0, self.original.1),
            format_size(self.file_size),
            match self.zoom {
                Zoom::Fit => "Fit",
                Zoom::Actual => "1:1",
            }
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(palette.terminal_bg))
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
                            .id("image-scroll")
                            .size_full()
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .child(
                                div()
                                    .min_w_full()
                                    .min_h_full()
                                    .p(zpx(size::PADDING))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        img(self
                                            .full_image
                                            .clone()
                                            .filter(|_| self.zoom == Zoom::Actual)
                                            .unwrap_or_else(|| self.fit_image.clone()))
                                        .flex_none()
                                        .w(px(width))
                                        .h(px(height))
                                        .object_fit(ObjectFit::Fill),
                                    ),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    if event.click_count == 2 {
                                        this.toggle_zoom(cx);
                                    }
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .h(zpx(size::STATUS_HEIGHT))
                    .px(zpx(size::STATUS_PADDING_X))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(hsla(palette.border))
                    .text_size(zpx(size::STATUS_TEXT_SIZE))
                    .text_color(hsla(palette.fg_muted))
                    .child(status),
            )
    }
}
