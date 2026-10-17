//! 미리보기들이 함께 쓰는 화면용 이미지 보조: 픽셀 → GPUI 이미지, 쓰고 난 이미지의 그래픽 메모리 반납.
//!
//! GPUI는 그린 이미지를 창의 텍스처 보관소에 올려 두고, 이미지를 버려도 보관소의 텍스처는 `Window::drop_image`를 불러야 풀린다.
//! 탭을 닫거나 쪽을 캐시에서 내보낼 때 이미지를 `Graveyard`에 맡기면 창이 다음 그리기에서 한꺼번에 반납한다.
use std::sync::{Arc, Mutex};

use gpui_kit::RenderImage;
use image::{Frame, ImageBuffer, Rgba};

use super::image_load::Pixels;

/// BGRA 픽셀로 GPUI 이미지를 만든다. 바이트 수가 가로×세로×4와 다르면 None이다.
pub fn render_image(pixels: Pixels) -> Option<Arc<RenderImage>> {
    let Pixels { width, height, bgra } = pixels;
    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bgra)
        .map(|buffer| Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

/// 더는 쓰지 않는 이미지를 모아 두는 곳. 복제본은 같은 목록을 가리킨다.
#[derive(Clone, Default)]
pub struct Graveyard {
    images: Arc<Mutex<Vec<Arc<RenderImage>>>>,
}

impl Graveyard {
    /// 이미지를 반납 대기 목록에 올린다.
    pub fn bury(
        &self,
        image: Arc<RenderImage>,
    ) {
        self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(image);
    }

    /// 대기 중인 이미지를 모두 꺼낸다.
    pub fn drain(&self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut *self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}
