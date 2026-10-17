//! 래스터 이미지를 읽어 화면에 올릴 픽셀로 만든다: 크기 확인, 디코드, 방향 보정, 상한을 넘으면 줄이기.
//!
//! 창 스레드가 아닌 곳에서 부른다. 이미지 파일은 크기에 비례해 디코드 메모리를 쓰므로(픽셀당 4바이트) 헤더의 가로세로를
//! 먼저 보고 열 수 없는 크기는 디코드하지 않으며, 올릴 크기(`decode_size`)를 넘는 이미지는 디코드 직후 줄여 원본 픽셀을 바로 버린다.
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits as Limits_};
use paddock_core::file_kind::extension_of;
use paddock_core::image_view::{
    DECODE_PIXELS, INLINE_DECODE_PIXELS, INLINE_SOURCE_PIXELS, MAX_PIXELS, decode_size_within, fit_decode_size,
};

use super::svg_load::{FILE_LIMIT as SVG_LIMIT, rasterize};

/// 디코드에 허용하는 작업 메모리(바이트). 5천만 픽셀 한도의 RGBA(200 MB)와 줄이는 사본이 들어가는 크기다.
const DECODE_ALLOC_LIMIT: u64 = 768 * 1024 * 1024;

/// 화면에 올릴 픽셀: 가로·세로와 BGRA 바이트(`width * height * 4`).
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "Pixels({}x{})", self.width, self.height)
    }
}

/// 이미지 하나를 읽은 결과.
pub enum RasterLoad {
    /// `pixels`는 올릴 크기, `original`은 파일 속 이미지의 가로세로(방향 보정 뒤)다.
    Ready { pixels: Pixels, original: (u32, u32) },
    /// 열 수 있는 크기(5천만 픽셀)를 넘는다.
    TooLarge { width: u32, height: u32 },
    /// 읽지 못했거나 이미지가 아니다. 값은 사용자에게 보일 이유 문장 조각이다.
    Failed(String),
}

/// 이미지를 읽는 조건.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// 메모리에 올리는 픽셀 수의 상한.
    pub budget: u64,
    /// 파일 속 이미지가 열 수 있는 픽셀 수의 상한. 넘으면 디코드하지 않는다.
    pub max_source_pixels: u64,
    /// 칸에 맞춰 보는 크기(화면 픽셀). 있으면 올릴 크기를 이 안으로 줄인다.
    pub fit_bound: Option<(u32, u32)>,
}

impl Limits {
    /// 이미지 탭이 쓰는 조건: 올릴 픽셀 1,600만, 열 수 있는 크기 5천만, 칸 크기는 호출자가 정한다.
    pub fn viewer(fit_bound: Option<(u32, u32)>) -> Self {
        Self { budget: DECODE_PIXELS, max_source_pixels: MAX_PIXELS, fit_bound }
    }

    /// 문서 속 이미지가 쓰는 조건: 올릴 픽셀 400만, 열 수 있는 크기 1,600만.
    pub fn inline() -> Self {
        Self { budget: INLINE_DECODE_PIXELS, max_source_pixels: INLINE_SOURCE_PIXELS, fit_bound: None }
    }
}

/// 경로의 래스터 이미지(png·jpg·gif·webp·bmp·ico·tiff)를 읽는다. 움직이는 gif·webp는 첫 장면만 쓴다.
/// 디코더가 비정상 파일에서 멈춰도(패닉) 앱이 죽지 않고 읽지 못한 이미지로 돌려준다.
pub fn load_raster_with(
    path: &str,
    limits: Limits,
) -> RasterLoad {
    match catch_unwind(AssertUnwindSafe(|| decode(path, limits))) {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(reason)) => RasterLoad::Failed(reason),
        Err(_) => RasterLoad::Failed("the decoder stopped on this file".to_string()),
    }
}

/// 이미지 탭이 원본 크기(올릴 수 있는 상한까지)로 읽는다.
pub fn load_raster(path: &str) -> RasterLoad {
    load_raster_with(path, Limits::viewer(None))
}

/// 문서 속 이미지 하나를 읽는다: SVG는 안전하게 그리고(`svg_load`), 래스터는 작게 올린다. 디코드는 한 번에 하나만 돈다 —
/// 이미지가 많은 문서가 디코드 메모리를 겹쳐 쓰지 않게 한다.
pub fn load_document_image(path: &str) -> RasterLoad {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if extension_of(path) == "svg" {
        match std::fs::File::open(path).and_then(|file| {
            let mut bytes = Vec::new();
            file.take(SVG_LIMIT + 1).read_to_end(&mut bytes).map(|_| bytes)
        }) {
            Ok(bytes) if bytes.len() as u64 > SVG_LIMIT => RasterLoad::TooLarge { width: 0, height: 0 },
            Ok(bytes) => match rasterize(&bytes) {
                Some((pixels, original)) => RasterLoad::Ready { pixels, original },
                None => RasterLoad::Failed("not a readable SVG".to_string()),
            },
            Err(error) => RasterLoad::Failed(error.to_string()),
        }
    } else {
        load_raster_with(path, Limits::inline())
    }
}

/// 래스터 읽기의 본체. 실패 이유를 짧은 영어 조각으로 돌려준다.
fn decode(
    path: &str,
    limits: Limits,
) -> Result<RasterLoad, String> {
    ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| error.to_string())
        .and_then(|reader| reader.into_decoder().map_err(|error| error.to_string()))
        .and_then(|decoder| {
            let (width, height) = decoder.dimensions();
            if u64::from(width) * u64::from(height) <= limits.max_source_pixels {
                read_pixels(decoder, limits)
            } else {
                Ok(RasterLoad::TooLarge { width, height })
            }
        })
}

/// 열 수 있는 크기로 확인된 이미지를 디코드해 올릴 크기로 줄인 BGRA 픽셀로 만든다.
fn read_pixels(
    mut decoder: impl ImageDecoder,
    limits: Limits,
) -> Result<RasterLoad, String> {
    let mut alloc = Limits_::default();
    alloc.max_alloc = Some(DECODE_ALLOC_LIMIT);
    decoder
        .set_limits(alloc)
        .and_then(|()| decoder.orientation())
        .and_then(|orientation| {
            DynamicImage::from_decoder(decoder).map(|mut image| {
                image.apply_orientation(orientation);
                image
            })
        })
        .map(|mut image| {
            let original = (image.width(), image.height());
            let (target_width, target_height) = match limits.fit_bound {
                Some(bound) => fit_decode_size(original.0, original.1, bound, limits.budget),
                None => decode_size_within(original.0, original.1, limits.budget),
            };
            if (target_width, target_height) != original {
                image = image.resize_exact(target_width, target_height, FilterType::Triangle);
            }
            let mut bgra = image.into_rgba8().into_raw();
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            RasterLoad::Ready { pixels: Pixels { width: target_width, height: target_height, bgra }, original }
        })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use image::{ImageBuffer, Rgb, Rgba};

    use super::*;

    fn write_png(
        dir: &Path,
        name: &str,
        width: u32,
        height: u32,
    ) -> String {
        let path = dir.join(name);
        ImageBuffer::from_fn(width, height, |x, _| Rgb([(x % 251) as u8, 20, 200])).save(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// 가로세로만 맞는 PNG 머리(서명 + IHDR + 빈 IDAT). 픽셀 데이터가 없어 디코드는 못 하지만 크기는 읽힌다.
    fn png_header_only(
        dir: &Path,
        name: &str,
        width: u32,
        height: u32,
    ) -> String {
        let mut ihdr = Vec::new();
        ihdr.extend(width.to_be_bytes());
        ihdr.extend(height.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        let mut crc: u32 = 0xffff_ffff;
        for byte in b"IHDR".iter().chain(ihdr.iter()) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(&ihdr);
        bytes.extend((!crc).to_be_bytes());
        // 디코더는 첫 이미지 데이터 조각(IDAT)을 만나야 머리 읽기를 마친다: 길이 0인 조각 하나를 붙인다.
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(b"IDAT");
        bytes.extend(0x35af_061eu32.to_be_bytes());
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn test_a_small_png_loads_at_its_own_size_in_bgra_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("one.png");
        ImageBuffer::from_pixel(2, 1, Rgba([10u8, 20, 30, 255])).save(&path).unwrap();
        match load_raster(&path.to_string_lossy()) {
            RasterLoad::Ready { pixels, original } => {
                assert_eq!((pixels.width, pixels.height, original), (2, 1, (2, 1)));
                assert_eq!(&pixels.bgra[..4], &[30, 20, 10, 255], "red and blue are swapped for the GPU");
            }
            _ => panic!("expected a loaded image"),
        }
    }

    #[test]
    fn test_an_image_over_fifty_megapixels_is_refused_from_its_header_without_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = png_header_only(dir.path(), "huge.png", 8000, 8000);
        assert!(matches!(load_raster(&path), RasterLoad::TooLarge { width: 8000, height: 8000 }));
    }

    #[test]
    fn test_an_image_over_the_decode_budget_is_reduced_but_reports_its_original_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), "big.png", 4600, 3800);
        match load_raster(&path) {
            RasterLoad::Ready { pixels, original } => {
                assert_eq!(original, (4600, 3800));
                assert!(u64::from(pixels.width) * u64::from(pixels.height) <= paddock_core::image_view::DECODE_PIXELS);
                assert_eq!(pixels.bgra.len(), pixels.width as usize * pixels.height as usize * 4);
                assert!(pixels.width < 4600);
            }
            _ => panic!("expected a reduced image"),
        }
    }

    #[test]
    fn test_a_missing_or_non_image_file_fails_with_a_reason_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("none.png").to_string_lossy().into_owned();
        assert!(matches!(load_raster(&missing), RasterLoad::Failed(_)));
        let text = dir.path().join("fake.png");
        fs::write(&text, "not an image").unwrap();
        assert!(matches!(load_raster(&text.to_string_lossy()), RasterLoad::Failed(_)));
        let cut = dir.path().join("cut.png");
        let good = write_png(dir.path(), "good.png", 50, 50);
        let bytes = fs::read(&good).unwrap();
        fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        assert!(matches!(load_raster(&cut.to_string_lossy()), RasterLoad::Failed(_)));
    }

    #[test]
    fn test_c_review_9_a_document_image_over_sixteen_megapixels_is_refused_and_a_smaller_one_is_reduced() {
        let dir = tempfile::tempdir().unwrap();
        let huge = png_header_only(dir.path(), "huge.png", 5000, 4000);
        assert!(matches!(load_document_image(&huge), RasterLoad::TooLarge { width: 5000, height: 4000 }));
        let mid = write_png(dir.path(), "mid.png", 3000, 2000);
        match load_document_image(&mid) {
            RasterLoad::Ready { pixels, original } => {
                assert_eq!(original, (3000, 2000));
                assert!(
                    u64::from(pixels.width) * u64::from(pixels.height)
                        <= paddock_core::image_view::INLINE_DECODE_PIXELS
                );
            }
            _ => panic!("expected a reduced image"),
        }
    }

    #[test]
    fn test_c_review_8_a_document_svg_is_drawn_safely_and_not_decoded_as_a_raster() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.svg");
        fs::write(&path, "<svg xmlns='http://www.w3.org/2000/svg' width='30' height='10'><rect width='30' height='10' fill='#00ff00'/></svg>").unwrap();
        match load_document_image(&path.to_string_lossy()) {
            RasterLoad::Ready { pixels, original } => {
                assert_eq!(original, (30, 10));
                assert_eq!(&pixels.bgra[..4], &[0, 255, 0, 255]);
            }
            _ => panic!("expected the svg to draw"),
        }
    }

    #[test]
    fn test_c_review_m3_a_fit_view_decodes_only_to_the_pane_size_and_reports_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), "wide.png", 400, 300);
        match load_raster_with(&path, Limits::viewer(Some((200, 200)))) {
            RasterLoad::Ready { pixels, original } => {
                assert_eq!(((pixels.width, pixels.height), original), ((200, 150), (400, 300)))
            }
            _ => panic!("expected an image"),
        }
        match load_raster(&path) {
            RasterLoad::Ready { pixels, .. } => assert_eq!((pixels.width, pixels.height), (400, 300)),
            _ => panic!("expected an image"),
        }
    }
}
