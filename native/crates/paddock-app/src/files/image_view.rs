//! 이미지 탭: 칸에 맞춰 보이는 그림(더블클릭하면 1:1)과 아래 상태 줄(가로 × 세로 · 파일 크기 · 보기 방식).
//!
//! 그림 픽셀은 이 보기가 쥔 하나뿐이다. 탭을 닫아 보기가 사라지면 `Graveyard`에 맡겨 창이 그래픽 메모리까지 반납한다.
//! 1:1로 볼 때 원본은 `TILE_SIDE` 조각으로 나눠 쥐고, 지금 보이는 조각만 그려(=그래픽 메모리에 올려) 큰 이미지를 통째로 올리지 않는다.
use std::collections::HashSet;
use std::sync::Arc;

#[cfg(feature = "automation")]
use gpui_kit::point;
use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement, Pixels,
    Render, RenderImage, ScrollHandle, Size, StatefulInteractiveElement, Styled, StyledImage, Window, canvas, div, img,
    px,
};
use paddock_core::file_kind::format_size;
use paddock_core::image_view::{Zoom, decode_size, display_size, size_text, tile_counts, tile_rect, visible_tiles};
use paddock_core::theme::image_view as size;

use super::image_load::{Pixels as Pixels_, RasterLoad, load_raster, split_tiles};
use super::images::{Graveyard, render_image};
use crate::chrome::zpx;
use crate::look::{colors, scaled};
use crate::terminal::paint::hsla;

/// 1:1로 볼 때 쥐는 원본 크기 그림: `TILE_SIDE` 조각들과, 그 가운데 그래픽 메모리에 올라가 있는 조각.
struct FullTiles {
    /// 원본 크기 그림의 가로세로.
    size: (u32, u32),
    /// 가로 칸 수.
    n_cols: u32,
    /// 위에서 아래, 왼쪽에서 오른쪽 순서(`row * n_cols + col`)의 조각.
    tiles: Vec<Arc<RenderImage>>,
    /// 마지막으로 그린 프레임에서 그려 그래픽 메모리에 올라간 조각의 번호.
    drawn: HashSet<usize>,
}

/// 열어 둔 이미지 하나.
///
/// 칸에 맞춰 보는 동안(`Fit`)은 칸 크기로 줄여 올린 그림(`fit_image`)만 쥔다. 1:1로 바꾸면 원본 크기(올릴 수 있는 상한까지)로 작업 스레드에서
/// 다시 읽어 조각으로 나눠(`full`) 쥐고 보이는 조각만 그리며, 맞춤으로 돌아오면 조각을 모두 버린다.
pub struct ImageView {
    path: String,
    fit_image: Arc<RenderImage>,
    /// 1:1로 보는 동안만 쥐는 원본 크기 그림.
    full: Option<FullTiles>,
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
            full: None,
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
        self.release_full();
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
        self.full.as_ref().map_or(self.decoded, |full| full.size)
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    /// 원본 크기 그림을 쥐고 있는지.
    pub fn has_full(&self) -> bool {
        self.full.is_some()
    }

    /// 지금 그래픽 메모리에 올라가 있는 원본 조각 수. 원본 크기 그림이 없으면 0이다.
    pub fn n_drawn_tiles(&self) -> usize {
        self.full.as_ref().map_or(0, |full| full.drawn.len())
    }

    /// 원본 크기 그림의 조각을 모두 버린다. 그래픽 메모리에 올라간 조각은 `Graveyard`에 맡겨 창이 반납하게 한다.
    fn release_full(&mut self) {
        if let Some(full) = self.full.take() {
            for index in full.drawn {
                self.graveyard.bury(full.tiles[index].clone());
            }
        }
    }

    /// 스크롤 위치를 바꾼다(자동 점검 전용): 원본 크기 그림의 왼쪽 위에서 (`x`, `y`) 논리 px 떨어진 곳이 보이게 한다.
    #[cfg(feature = "automation")]
    pub fn scroll_to(
        &mut self,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        self.scroll.set_offset(point(px(-x), px(-y)));
        cx.notify();
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
                self.release_full();
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
                let loaded = cx
                    .background_executor()
                    .spawn(async move {
                        match load_raster(&path) {
                            RasterLoad::Ready { pixels, .. } => {
                                Some(((pixels.width, pixels.height), split_tiles(pixels)))
                            }
                            _ => None,
                        }
                    })
                    .await;
                let _ = this.update(cx, |this, cx| this.finish_full(generation, loaded, cx));
            })
            .detach();
        }
    }

    fn finish_full(
        &mut self,
        generation: u64,
        loaded: Option<((u32, u32), Vec<Pixels_>)>,
        cx: &mut Context<Self>,
    ) {
        if generation == self.generation && self.zoom == Zoom::Actual {
            self.loading_full = false;
            if let Some((size, pieces)) = loaded {
                let (n_cols, n_rows) = tile_counts(size);
                let tiles: Vec<Arc<RenderImage>> = pieces.into_iter().filter_map(render_image).collect();
                if tiles.len() == (n_cols * n_rows) as usize {
                    #[cfg(feature = "automation")]
                    {
                        *super::images::audit::LAST_FULL.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                            Some(Arc::downgrade(&tiles[0]));
                    }
                    self.release_full();
                    self.full = Some(FullTiles { size, n_cols, tiles, drawn: HashSet::new() });
                    // 원본 크기 조각이 그려지는 동안 칸에 맞춘 그림은 쓰지 않는다: 그래픽 메모리에서 내린다.
                    self.graveyard.bury(self.fit_image.clone());
                }
            }
            cx.notify();
        }
    }

    /// 보기 영역의 크기를 기억한다. 바뀌었으면 true다.
    fn set_pane(
        &mut self,
        pane: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.pane != pane;
        if changed {
            self.pane = pane;
            cx.notify();
        }
        changed
    }
}

impl Drop for ImageView {
    fn drop(&mut self) {
        self.graveyard.bury(self.fit_image.clone());
        self.release_full();
    }
}

impl ImageView {
    /// 1:1로 볼 때 지금 보이는 원본 조각들을 놓은 상자. 보이는 조각만 그리고(=그래픽 메모리에 올리고), 이번 프레임에 보이지 않게 된 조각은 반납을 맡긴다.
    /// 원본 크기 그림이 아직 없으면 None이다. 함께 돌려주는 값은 이번 프레임에 반납을 맡긴 조각이 있는지다.
    fn tile_layer(
        &mut self,
        width: f32,
        height: f32,
    ) -> Option<(Div, bool)> {
        let scale = self.device_scale;
        let padding = scaled(size::PADDING);
        // 스크롤 위치는 그림이 움직일 수 있는 범위 [-(그림 + 여백 - 칸), 0] 안으로 먼저 자른다(끝을 넘어 당긴 값으로 조각을 고르지 않는다).
        let max_scroll = (
            (width + 2.0 * padding - f32::from(self.pane.width)).max(0.0),
            (height + 2.0 * padding - f32::from(self.pane.height)).max(0.0),
        );
        let offset = self.scroll.offset();
        let (scroll_x, scroll_y) =
            ((-f32::from(offset.x)).clamp(0.0, max_scroll.0), (-f32::from(offset.y)).clamp(0.0, max_scroll.1));
        // 조각은 줄여 올린 그림(`full.size`)의 좌표에 있고 상자는 원본 크기다: 원본 대비 줄인 비율을 곱해 둘을 맞춘다.
        let original = self.original;
        let graveyard = self.graveyard.clone();
        self.full.as_mut().map(|full| {
            let ratio = (original.0 as f32 / full.size.0.max(1) as f32, original.1 as f32 / full.size.1.max(1) as f32);
            let window = (
                (scroll_x - padding) * scale / ratio.0,
                (scroll_y - padding) * scale / ratio.1,
                f32::from(self.pane.width) * scale / ratio.0,
                f32::from(self.pane.height) * scale / ratio.1,
            );
            let (size_, n_cols) = (full.size, full.n_cols);
            let (cols, rows) = visible_tiles(size_, window);
            let mut wanted = HashSet::new();
            let mut layer = div().relative().flex_none().w(px(width)).h(px(height));
            for row in rows {
                for col in cols.clone() {
                    let index = (row * n_cols + col) as usize;
                    let (x, y, tile_width, tile_height) = tile_rect(size_, col, row);
                    wanted.insert(index);
                    layer = layer.child(
                        img(full.tiles[index].clone())
                            .absolute()
                            .left(px(x as f32 * ratio.0 / scale))
                            .top(px(y as f32 * ratio.1 / scale))
                            .w(px(tile_width as f32 * ratio.0 / scale))
                            .h(px(tile_height as f32 * ratio.1 / scale))
                            .object_fit(ObjectFit::Fill),
                    );
                }
            }
            let mut buried = false;
            for index in full.drawn.difference(&wanted) {
                graveyard.bury(full.tiles[*index].clone());
                buried = true;
            }
            full.drawn = wanted;
            (layer, buried)
        })
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
        let tile_layer = if self.zoom == Zoom::Actual { self.tile_layer(width, height) } else { None };
        // 보이지 않게 된 조각을 맡겼으면 한 프레임 더 그려 창이 그래픽 메모리를 바로 반납하게 한다.
        if tile_layer.as_ref().is_some_and(|(_, buried)| *buried) {
            window.on_next_frame(|window, _| window.refresh());
        }
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
                            move |bounds, window, cx| {
                                // 칸 크기는 배치가 끝나야 알 수 있다. 그리는 도중의 알림만으로는 다음 프레임이 보장되지 않으므로
                                // 크기가 바뀌었으면 다음 프레임에 다시 그리도록 맡긴다.
                                if entity.update(cx, |view, cx| view.set_pane(bounds.size, cx)) {
                                    window.on_next_frame(|window, _| window.refresh());
                                }
                            },
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
                                    // 안쪽 상자의 최소 크기는 max(칸, 그림 + 여백) 하나다: 칸보다 작은 그림은 칸 안에서 가운데에 놓이고,
                                    // 큰 그림은 상자가 그림만큼 넓어져 가로·세로로 끝까지 스크롤된다.
                                    .min_w(px(f32::from(self.pane.width).max(width + 2.0 * scaled(size::PADDING))))
                                    .min_h(px(f32::from(self.pane.height).max(height + 2.0 * scaled(size::PADDING))))
                                    .p(zpx(size::PADDING))
                                    .flex()
                                    // 자동 여백으로 가운데에 놓는다: 칸보다 큰 그림은 여백이 0이 되어 왼쪽·위에서 시작하고 끝까지 스크롤된다
                                    // (`justify_center`는 큰 그림을 양쪽으로 넘쳐 보내 왼쪽·위쪽을 스크롤로 닿지 못하게 한다).
                                    .child(match tile_layer {
                                        Some((layer, _)) => layer.mx_auto().my_auto().into_any_element(),
                                        None => img(self.fit_image.clone())
                                            .flex_none()
                                            .mx_auto()
                                            .my_auto()
                                            .w(px(width))
                                            .h(px(height))
                                            .object_fit(ObjectFit::Fill)
                                            .into_any_element(),
                                    }),
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
