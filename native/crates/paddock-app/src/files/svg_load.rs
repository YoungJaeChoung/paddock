//! SVG를 작업 스레드에서 그림으로 바꾼다: 원래 크기를 읽고, 그 크기의 두 배(최대 긴 변 2048px)로 래스터화한다.
//!
//! 파일 안의 `<image href>`는 글자 주소를 전부 거부하고 `data:` 주소만 받는다. 거부하지 않으면 렌더러가 그 주소를 파일 경로로 읽는다:
//! 공유 폴더(`\\서버\…`) 경로는 네트워크 인증을 부르고, 임의의 로컬 파일을 읽으며, 이름 붙은 파이프 같은 특수 파일에서는 영원히 멈춘다.
//! 글자(`<text>`)가 든 SVG만 시스템 글꼴 목록을 한 번 읽어 쓴다(읽는 데 시간·메모리가 든다).
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use usvg::{ImageHrefResolver, Options, Tree, fontdb};

use super::image_load::Pixels;

/// 읽을 수 있는 SVG 파일 크기의 상한(5 MiB). 넘으면 이미지가 너무 크다는 안내로 보낸다.
pub const FILE_LIMIT: u64 = 5 * 1024 * 1024;

/// 래스터화한 그림의 긴 변 상한(px).
const LONG_SIDE_LIMIT: f32 = 2048.0;

/// 원래 크기에 곱하는 배수의 상한. 작은 SVG도 이 배수까지만 키워 그린다.
const MAX_SCALE: f32 = 2.0;

/// 시스템 글꼴 목록. 글자가 든 SVG를 처음 그릴 때 한 번 읽는다.
static SYSTEM_FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

/// SVG 원문을 BGRA 픽셀(미리 곱한 알파)과 원래 가로세로로 바꾼다. 읽지 못하거나 그리다 멈추면 None이다.
pub fn rasterize(bytes: &[u8]) -> Option<(Pixels, (u32, u32))> {
    rasterize_logging(bytes, &Arc::new(Mutex::new(Vec::new())))
}

/// `rasterize`와 같되, 렌더러가 읽으려 한 `<image href>` 글자 주소를 `refused`에 쌓는다(모두 거부된 것이다).
pub fn rasterize_logging(
    bytes: &[u8],
    refused: &Arc<Mutex<Vec<String>>>,
) -> Option<(Pixels, (u32, u32))> {
    catch_unwind(AssertUnwindSafe(|| draw(bytes, refused))).ok().flatten()
}

fn draw(
    bytes: &[u8],
    refused: &Arc<Mutex<Vec<String>>>,
) -> Option<(Pixels, (u32, u32))> {
    let log = refused.clone();
    let image_href_resolver = ImageHrefResolver {
        resolve_data: ImageHrefResolver::default_data_resolver(),
        resolve_string: Box::new(move |href, _| {
            log.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(href.to_string());
            None
        }),
    };
    let mut options = Options { image_href_resolver, ..Options::default() };
    if bytes.windows(5).any(|window| window == b"<text") {
        options.fontdb = SYSTEM_FONTS
            .get_or_init(|| {
                let mut database = fontdb::Database::new();
                database.load_system_fonts();
                Arc::new(database)
            })
            .clone();
    }
    Tree::from_data(bytes, &options).ok().and_then(|tree| paint(&tree))
}

/// 읽은 SVG를 원래 크기의 `MAX_SCALE`배(긴 변 `LONG_SIDE_LIMIT`까지)로 그린다. 크기가 0이거나 유한하지 않으면 None이다.
fn paint(tree: &Tree) -> Option<(Pixels, (u32, u32))> {
    let natural = (tree.size().width(), tree.size().height());
    let long_side = natural.0.max(natural.1);
    let valid = long_side > 0.0 && long_side.is_finite();
    let scale = (LONG_SIDE_LIMIT / long_side).min(MAX_SCALE);
    let target = (((natural.0 * scale).round() as u32).max(1), ((natural.1 * scale).round() as u32).max(1));
    valid.then(|| resvg::tiny_skia::Pixmap::new(target.0, target.1)).flatten().map(|mut pixmap| {
        let transform =
            resvg::tiny_skia::Transform::from_scale(target.0 as f32 / natural.0, target.1 as f32 / natural.1);
        resvg::render(tree, transform, &mut pixmap.as_mut());
        let mut bgra = pixmap.take();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        (
            Pixels { width: target.0, height: target.1, bgra },
            ((natural.0.round() as u32).max(1), (natural.1.round() as u32).max(1)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red_square_with(image_href: &str) -> String {
        format!(
            "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='20' height='10'>\
             <rect width='20' height='10' fill='#ff0000'/>\
             <image x='0' y='0' width='5' height='5' href=\"{image_href}\"/></svg>"
        )
    }

    #[test]
    fn test_c_review_1_an_svg_draws_and_reports_its_natural_size() {
        let (pixels, original) = rasterize(red_square_with("").as_bytes()).unwrap();
        assert_eq!(original, (20, 10));
        assert_eq!((pixels.width, pixels.height), (40, 20));
        assert_eq!(&pixels.bgra[..4], &[0, 0, 255, 255], "opaque red in BGRA order");
    }

    #[test]
    fn test_c_review_1_image_hrefs_that_name_files_or_shares_are_refused_never_read() {
        let refused = Arc::new(Mutex::new(Vec::new()));
        let hrefs = [
            r"\\attacker\share\x.png",
            "//attacker/share/x.png",
            "/etc/passwd",
            r"C:\Windows\win.ini",
            "file:///etc/passwd",
            "http://127.0.0.1:1/x.png",
            "../secret.png",
        ];
        for href in hrefs {
            let drawn = rasterize_logging(red_square_with(href).as_bytes(), &refused);
            assert!(drawn.is_some(), "the picture still draws without the image: {href}");
        }
        let seen = refused.lock().unwrap().clone();
        for href in hrefs {
            assert!(seen.iter().any(|seen| seen == href), "{href} went to the refusing resolver: {seen:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_c_review_1_an_href_naming_a_fifo_does_not_hang_the_drawing() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.png");
        let path = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: 경로는 NUL로 끝나는 올바른 C 글자열이다.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let refused = Arc::new(Mutex::new(Vec::new()));
        let drawn = rasterize_logging(red_square_with(&fifo.to_string_lossy()).as_bytes(), &refused);
        assert!(drawn.is_some());
        assert_eq!(refused.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_c_review_1_a_data_url_image_is_not_sent_to_the_string_resolver() {
        // 1×1 투명 PNG.
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let refused = Arc::new(Mutex::new(Vec::new()));
        assert!(rasterize_logging(red_square_with(png).as_bytes(), &refused).is_some());
        assert!(refused.lock().unwrap().is_empty());
    }

    #[test]
    fn test_garbage_is_not_an_svg() {
        assert!(rasterize(b"not an svg").is_none());
    }
}
