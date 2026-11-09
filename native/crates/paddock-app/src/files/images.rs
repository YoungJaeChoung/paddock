//! 미리보기들이 함께 쓰는 화면용 이미지 보조: 픽셀 → GPUI 이미지, 쓰고 난 이미지의 그래픽 메모리 반납.
//!
//! GPUI는 그린 이미지를 창의 텍스처 보관소에 올려 두고, 이미지를 버려도 보관소의 텍스처는 `Window::drop_image`를 불러야 풀린다.
//! 탭을 닫거나 쪽을 캐시에서 내보낼 때 이미지를 `Graveyard`에 맡기면 창이 다음 그리기에서 한꺼번에 반납한다.
#[cfg(feature = "automation")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use gpui_kit::RenderImage;
use image::{Frame, ImageBuffer, Rgba};

use super::image_load::Pixels;

/// 자동 점검이 읽는 반납 기록: 맡긴 이미지 수·꺼낸 이미지 수·가장 최근 원본 크기 그림이 아직 살아 있는 사본 수.
#[cfg(feature = "automation")]
pub mod audit {
    use super::*;

    pub static BURIED: AtomicUsize = AtomicUsize::new(0);
    pub static DRAINED: AtomicUsize = AtomicUsize::new(0);
    pub static LAST_FULL: Mutex<Option<Weak<RenderImage>>> = Mutex::new(None);

    /// 가장 최근 원본 크기 그림을 아직 쥔 사본 수. 그런 그림이 없으면 0이다.
    pub fn last_full_strong() -> usize {
        LAST_FULL.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().map_or(0, Weak::strong_count)
    }

    pub fn counts() -> (usize, usize) {
        (BURIED.load(Ordering::Relaxed), DRAINED.load(Ordering::Relaxed))
    }
}

/// BGRA 픽셀로 GPUI 이미지를 만든다. 바이트 수가 가로×세로×4와 다르면 None이다.
pub fn render_image(pixels: Pixels) -> Option<Arc<RenderImage>> {
    let Pixels { width, height, bgra } = pixels;
    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bgra)
        .map(|buffer| Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

type Inbox = Arc<Mutex<Vec<Arc<RenderImage>>>>;
type WeakInbox = Weak<Mutex<Vec<Arc<RenderImage>>>>;

/// 더는 쓰지 않는 이미지를 모아 두는 곳. 복제본은 같은 목록을 가리킨다.
///
/// 텍스처 보관소는 창마다 따로라서 `drop_image`는 이미지를 그린 창에서 불러야 풀린다. 본 창이 비우는 목록 말고, 분리 창이 자기 보관소에서
/// 풀 사본 목록(`ImageMirror`)을 더 둘 수 있다 — `bury`가 둘 모두에 올린다.
#[derive(Clone, Default)]
pub struct Graveyard {
    images: Inbox,
    mirrors: Arc<Mutex<Vec<WeakInbox>>>,
}

/// 분리 창이 쓰는 반납 대기 목록: 본 창의 `Graveyard`에 올라온 이미지의 사본을 받는다. 창이 닫혀 이 값이 버려지면 더는 받지 않는다.
pub struct ImageMirror {
    images: Inbox,
}

impl ImageMirror {
    /// 대기 중인 이미지를 모두 꺼낸다.
    pub fn drain(&self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut *self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

impl Graveyard {
    /// 이미지를 반납 대기 목록에 올린다.
    pub fn bury(
        &self,
        image: Arc<RenderImage>,
    ) {
        #[cfg(feature = "automation")]
        audit::BURIED.fetch_add(1, Ordering::Relaxed);
        {
            let mut mirrors = self.mirrors.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            mirrors.retain(|mirror| match mirror.upgrade() {
                Some(inbox) => {
                    inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(image.clone());
                    true
                }
                None => false,
            });
        }
        self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(image);
    }

    /// 대기 중인 이미지를 모두 꺼낸다.
    pub fn drain(&self) -> Vec<Arc<RenderImage>> {
        let taken = std::mem::take(&mut *self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        #[cfg(feature = "automation")]
        audit::DRAINED.fetch_add(taken.len(), Ordering::Relaxed);
        taken
    }

    /// 분리 창이 자기 보관소를 비울 사본 목록을 만든다.
    pub fn mirror(&self) -> ImageMirror {
        let images = Inbox::default();
        self.mirrors.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(Arc::downgrade(&images));
        ImageMirror { images }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_pixel() -> Arc<RenderImage> {
        render_image(Pixels { width: 1, height: 1, bgra: vec![0, 0, 0, 255] }).expect("1×1 bgra is valid")
    }

    #[test]
    fn test_c_d86_an_image_buried_while_a_detached_window_is_open_reaches_that_windows_list_too() {
        let graveyard = Graveyard::default();
        let mirror = graveyard.mirror();
        let image = one_pixel();
        graveyard.bury(image.clone());
        let main: Vec<_> = graveyard.drain().iter().map(|image| image.id).collect();
        let detached: Vec<_> = mirror.drain().iter().map(|image| image.id).collect();
        assert_eq!(main, vec![image.id], "the main window still drops it from its own atlas");
        assert_eq!(detached, vec![image.id], "the detached window drops it from its own atlas");
        assert!(mirror.drain().is_empty(), "drained once");
    }

    #[test]
    fn test_c_d86_a_closed_detached_windows_list_stops_collecting_images() {
        let graveyard = Graveyard::default();
        let mirror = graveyard.mirror();
        let image = one_pixel();
        drop(mirror);
        graveyard.bury(image.clone());
        assert_eq!(Arc::strong_count(&image), 2, "only the caller and the main list hold it");
        assert_eq!(graveyard.drain().len(), 1);
        assert!(graveyard.mirrors.lock().unwrap().is_empty(), "the dead list is forgotten");
    }
}
