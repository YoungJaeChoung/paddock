//! PDF 파일을 열어 쪽 수·쪽 크기를 알아내고, 쪽 하나를 비트맵으로 그린다.
//!
//! 순수 Rust 래스터라이저(hayro)를 쓴다. 열기와 그리기는 느릴 수 있어 창 스레드가 아닌 곳에서 부른다. 문서는 한 번 열어 여러 작업에서
//! 함께 쓰고(`PdfDocument`는 스레드 사이를 오갈 수 있다), 파싱·그리기 중 래스터라이저가 비정상 입력으로 멈춰도(패닉) 앱이 죽지 않게
//! 그 쪽만 실패로 돌려준다.
use std::panic::{AssertUnwindSafe, catch_unwind};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::LoadPdfError;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use paddock_core::pdf_view::page_pixels;

/// 열 수 있는 PDF 파일 크기의 상한(64 MiB). 파일 내용을 통째로 메모리에 쥐고 있으므로 이보다 큰 파일은 바깥 프로그램에 맡긴다.
pub const FILE_LIMIT: u64 = 64 * 1024 * 1024;

/// PDF를 열지 못한 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// 암호가 걸려 있어 열 수 없다.
    Encrypted,
    /// 손상됐거나 이 렌더러가 읽지 못하는 파일이다.
    Unreadable,
}

/// 열어 둔 PDF 문서.
pub struct PdfDocument {
    pdf: Pdf,
    /// 쪽마다의 (가로, 세로) 크기(pt, 회전 반영).
    sizes: Vec<(f32, f32)>,
}

impl std::fmt::Debug for PdfDocument {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "PdfDocument({} pages)", self.sizes.len())
    }
}

/// 그려 낸 쪽: BGRA 바이트(`width * height * 4`).
pub struct PageBitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl PdfDocument {
    /// 파일 내용으로 문서를 연다. 쪽이 하나도 없으면 읽지 못한 파일로 본다.
    pub fn open(bytes: Vec<u8>) -> Result<Self, OpenError> {
        let opened = catch_unwind(AssertUnwindSafe(|| match Pdf::new(bytes) {
            Ok(pdf) => {
                let sizes: Vec<(f32, f32)> = pdf.pages().iter().map(|page| page.render_dimensions()).collect();
                if sizes.is_empty() { Err(OpenError::Unreadable) } else { Ok(Self { pdf, sizes }) }
            }
            Err(LoadPdfError::Decryption(_)) => Err(OpenError::Encrypted),
            Err(LoadPdfError::Invalid) => Err(OpenError::Unreadable),
        }));
        opened.unwrap_or(Err(OpenError::Unreadable))
    }

    pub fn n_pages(&self) -> usize {
        self.sizes.len()
    }

    /// 쪽마다의 (가로, 세로) 크기(pt).
    pub fn sizes(&self) -> &[(f32, f32)] {
        &self.sizes
    }

    /// `index`번째(0부터) 쪽을 `percent` 배율로 그린다. 쪽이 없거나 그리다 실패하면 None이다.
    pub fn render(
        &self,
        index: usize,
        percent: u32,
    ) -> Option<PageBitmap> {
        self.sizes.get(index).copied().and_then(|size| {
            let (width, height) = page_pixels(size, percent);
            catch_unwind(AssertUnwindSafe(|| self.draw(index, size, (width, height)))).ok().flatten()
        })
    }

    /// 쪽 하나를 `pixels` 크기로 그려 BGRA 비트맵으로 만든다.
    fn draw(
        &self,
        index: usize,
        size: (f32, f32),
        pixels: (u32, u32),
    ) -> Option<PageBitmap> {
        self.pdf.pages().get(index).map(|page| {
            let settings = PixmapSettings {
                x_scale: pixels.0 as f32 / size.0,
                y_scale: pixels.1 as f32 / size.1,
                bg_color: WHITE,
            };
            let pixmap = render(
                page,
                &RenderCache::new(),
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &settings,
            );
            let mut bgra = pixmap.data_as_u8_slice().to_vec();
            // 바탕이 불투명한 흰색이라 미리 곱한 값과 그대로의 값이 같다. 빨강과 파랑만 맞바꾼다.
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            PageBitmap { width: u32::from(pixmap.width()), height: u32::from(pixmap.height()), bgra }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = include_bytes!("../../../../scripts/assets/sample-3p.pdf");
    const ENCRYPTED: &[u8] = include_bytes!("../../../../scripts/assets/encrypted.pdf");

    #[test]
    fn test_the_sample_pdf_opens_with_three_a4_pages() {
        let document = PdfDocument::open(SAMPLE.to_vec()).unwrap();
        assert_eq!(document.n_pages(), 3);
        assert_eq!(document.sizes()[0], (595.0, 842.0));
    }

    #[test]
    fn test_a_page_renders_at_the_requested_scale_with_dark_glyph_pixels_on_white() {
        let document = PdfDocument::open(SAMPLE.to_vec()).unwrap();
        let started = std::time::Instant::now();
        let page = document.render(0, 100).unwrap();
        let elapsed = started.elapsed();
        assert_eq!((page.width, page.height), (793, 1123));
        assert_eq!(page.bgra.len(), 793 * 1123 * 4);
        let dark = page
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100)
            .count();
        let white = page.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[..3] == [255, 255, 255]).count();
        assert!(dark > 200, "the text is drawn: {dark}");
        assert!(white > page.bgra.len() / 4 / 2, "the paper is white: {white}");
        assert!(elapsed.as_secs_f32() < 1.0, "one A4 text page renders in under a second: {elapsed:?}");
        assert!(document.render(3, 100).is_none());
    }

    #[test]
    fn test_a_password_protected_pdf_is_reported_as_encrypted_and_garbage_as_unreadable() {
        assert_eq!(PdfDocument::open(ENCRYPTED.to_vec()).err(), Some(OpenError::Encrypted));
        assert_eq!(PdfDocument::open(b"not a pdf at all".to_vec()).err(), Some(OpenError::Unreadable));
        assert_eq!(PdfDocument::open(Vec::new()).err(), Some(OpenError::Unreadable));
        // 잘린 파일은 패닉 없이 열리거나 읽지 못한 파일로 돌아온다. 열렸다면 남은 쪽을 그려 보아도 패닉이 없다.
        let cut = &SAMPLE[..SAMPLE.len() / 2];
        match PdfDocument::open(cut.to_vec()) {
            Ok(document) => (0..document.n_pages()).for_each(|index| drop(document.render(index, 50))),
            Err(reason) => assert_eq!(reason, OpenError::Unreadable),
        }
    }
}
