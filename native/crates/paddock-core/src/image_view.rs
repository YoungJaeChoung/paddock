//! 이미지 탭의 규칙: 칸에 맞춘 표시 크기, 맞춤·1:1 전환, 크기 글자, 디코드 크기 상한.
//!
//! 화면에 그리는 일과 파일을 읽는 일은 앱이 하고, 여기서는 가로세로 수만 받아 계산하는 순수 규칙만 둔다.

/// 이 픽셀 수(5천만)를 넘는 이미지는 미리보기를 열지 않고 안내 탭으로 보낸다.
pub const MAX_PIXELS: u64 = 50_000_000;

/// 디코드한 이미지가 메모리에 올라가는 픽셀 수의 상한(1,600만 ≈ 64 MB). 넘는 이미지는 비율을 지켜 줄여 올린다.
pub const DECODE_PIXELS: u64 = 16_000_000;

/// 올린 이미지 한 변의 상한(px). 그래픽 텍스처가 이보다 큰 변을 받지 못한다.
pub const MAX_SIDE: u32 = 16_384;

/// 문서 속 이미지(Markdown 미리보기)를 올리는 픽셀 수의 상한(400만). 본문 폭(760)에 그리는 데는 넘치는 크기다.
pub const INLINE_DECODE_PIXELS: u64 = 4_000_000;

/// 문서 속 이미지 파일이 열 수 있는 픽셀 수의 상한(1,600만). 넘으면 디코드하지 않고 빈 자리로 둔다.
pub const INLINE_SOURCE_PIXELS: u64 = 16_000_000;

/// 세로(가로)가 가로(세로)의 이 배수 이상이면 긴 그림이다: 칸에 통째로 맞추면 글자 수준으로 작아지므로 짧은 쪽에 맞추고 긴 쪽은 스크롤한다.
pub const LONG_ASPECT: f32 = 3.0;

/// 1:1로 볼 때 원본을 나누는 조각의 한 변(px). 보이는 조각만 그래픽 메모리에 올려, 큰 이미지를 통째로 한 번에 올리는 일을 피한다.
pub const TILE_SIDE: u32 = 512;

/// 이미지를 칸에서 보이는 방식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    /// 칸에 맞춘다(키우지는 않는다).
    Fit,
    /// 이미지 픽셀 하나가 화면 픽셀 하나(1:1). 칸보다 크면 스크롤한다.
    Actual,
}

impl Zoom {
    /// 더블클릭이 바꾸는 쪽.
    pub fn toggled(self) -> Self {
        match self {
            Self::Fit => Self::Actual,
            Self::Actual => Self::Fit,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Actual => "actual",
        }
    }
}

/// 이미지를 열 수 있는 크기인지.
pub fn within_limit(
    width: u32,
    height: u32,
) -> bool {
    u64::from(width) * u64::from(height) <= MAX_PIXELS
}

/// 메모리에 올릴 크기: `DECODE_PIXELS` 이하이고 한 변이 `MAX_SIDE` 이하이면 그대로, 넘으면 비율을 지켜 줄인다. 가로세로는 1 이상이다.
///
/// Examples
/// --------
/// | 원본          | 결과          |
/// |---------------|---------------|
/// | 1920 × 1080   | 1920 × 1080   |
/// | 7000 × 7000   | 4000 × 4000   |
pub fn decode_size(
    width: u32,
    height: u32,
) -> (u32, u32) {
    decode_size_within(width, height, DECODE_PIXELS)
}

/// `decode_size`와 같되 상한을 `budget` 픽셀로 준다.
pub fn decode_size_within(
    width: u32,
    height: u32,
    budget: u64,
) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    let longest = width.max(height);
    if pixels <= budget && longest <= MAX_SIDE {
        (width.max(1), height.max(1))
    } else {
        // 픽셀 수 상한과 한 변 상한 중 더 많이 줄이는 쪽을 따른다.
        let by_pixels = (budget as f64 / pixels as f64).sqrt();
        let by_side = f64::from(MAX_SIDE) / f64::from(longest);
        let scale = by_pixels.min(by_side).min(1.0);
        let mut shrunk =
            (((f64::from(width) * scale).round() as u32).max(1), ((f64::from(height) * scale).round() as u32).max(1));
        // 반올림으로 상한을 넘으면 긴 쪽을 한 칸 줄인다.
        while u64::from(shrunk.0) * u64::from(shrunk.1) > budget || shrunk.0.max(shrunk.1) > MAX_SIDE {
            if shrunk.0 >= shrunk.1 { shrunk.0 -= 1 } else { shrunk.1 -= 1 }
        }
        shrunk
    }
}

/// 칸에 맞춰 보는 동안 올릴 크기: `decode_size_within`의 크기를 `Fit`으로 칸 크기 `bound`(화면 픽셀)에 맞춰 한 번 더 줄인다(긴 그림은 폭에 맞춤). 키우지는 않는다.
/// 칸보다 큰 이미지를 원본 크기로 올리는 일은 1:1로 바꿀 때만 한다.
///
/// Examples
/// --------
/// | 원본          | 칸          | 결과          |
/// |---------------|-------------|---------------|
/// | 3840 × 2160   | 1500 × 900  | 1500 × 844    |
/// | 800 × 600     | 1500 × 900  | 800 × 600     |
pub fn fit_decode_size(
    width: u32,
    height: u32,
    bound: (u32, u32),
    budget: u64,
) -> (u32, u32) {
    let (reduced_width, reduced_height) = decode_size_within(width, height, budget);
    let (shown_width, shown_height) =
        display_size((reduced_width, reduced_height), (bound.0 as f32, bound.1 as f32), Zoom::Fit);
    ((shown_width.round() as u32).max(1), (shown_height.round() as u32).max(1))
}

/// 칸 `pane`(가로, 세로)에서 이미지 `image`(가로, 세로 픽셀)가 차지하는 크기.
///
/// `Actual`은 픽셀 크기 그대로다. `Fit`은 칸 안에 비율을 지켜 줄여 넣되 키우지는 않는다. 긴 그림(`LONG_ASPECT` 이상)은 짧은 쪽을
/// 칸에 맞추고 긴 쪽은 넘쳐 스크롤하게 둔다.
///
/// Examples
/// --------
/// | 이미지        | 칸           | 방식   | 결과          |
/// |---------------|--------------|--------|---------------|
/// | 1920 × 1080   | 960 × 800    | Fit    | 960 × 540     |
/// | 100 × 50      | 800 × 600    | Fit    | 100 × 50      |
/// | 800 × 8000    | 600 × 400    | Fit    | 600 × 6000    |
/// | 1920 × 1080   | 400 × 300    | Actual | 1920 × 1080   |
pub fn display_size(
    image: (u32, u32),
    pane: (f32, f32),
    zoom: Zoom,
) -> (f32, f32) {
    let (width, height) = (image.0.max(1) as f32, image.1.max(1) as f32);
    let scale = match zoom {
        Zoom::Actual => 1.0,
        Zoom::Fit if height / width >= LONG_ASPECT => (pane.0 / width).min(1.0),
        Zoom::Fit if width / height >= LONG_ASPECT => (pane.1 / height).min(1.0),
        Zoom::Fit => (pane.0 / width).min(pane.1 / height).min(1.0),
    };
    (width * scale, height * scale)
}

/// 1:1 조각의 가로·세로 칸 수. 가로세로는 1 이상으로 본다.
///
/// Examples
/// --------
/// | 이미지        | 결과   |
/// |---------------|--------|
/// | 1920 × 1080   | (4, 3) |
/// | 512 × 512     | (1, 1) |
pub fn tile_counts(image: (u32, u32)) -> (u32, u32) {
    (image.0.max(1).div_ceil(TILE_SIDE), image.1.max(1).div_ceil(TILE_SIDE))
}

/// 조각 (`col`, `row`)이 이미지에서 차지하는 영역 (x, y, 가로, 세로). 오른쪽·아래 끝 조각은 `TILE_SIDE`보다 짧다.
///
/// Examples
/// --------
/// | 이미지        | 칸     | 결과                |
/// |---------------|--------|---------------------|
/// | 1920 × 1080   | (0, 0) | (0, 0, 512, 512)    |
/// | 1920 × 1080   | (3, 2) | (1536, 1024, 384, 56) |
pub fn tile_rect(
    image: (u32, u32),
    col: u32,
    row: u32,
) -> (u32, u32, u32, u32) {
    let (x, y) = (col * TILE_SIDE, row * TILE_SIDE);
    (x, y, TILE_SIDE.min(image.0.max(1).saturating_sub(x)), TILE_SIDE.min(image.1.max(1).saturating_sub(y)))
}

/// 이미지 좌표의 창 `window`(x, y, 가로, 세로: 이미지 픽셀)와 겹치는 조각의 (가로 칸 범위, 세로 칸 범위). 닿기만 하는 조각은 뺀다.
/// 창이 이미지를 벗어난 부분은 잘라 보고, 이미지와 전혀 겹치지 않으면 빈 범위다.
///
/// Examples
/// --------
/// | 이미지        | 창                    | 결과          |
/// |---------------|-----------------------|---------------|
/// | 3840 × 2160   | (0, 0, 1500, 900)     | (0..3, 0..2)  |
/// | 3840 × 2160   | (512, 512, 512, 512)  | (1..2, 1..2)  |
/// | 3840 × 2160   | (9000, 0, 500, 500)   | 빈 범위       |
pub fn visible_tiles(
    image: (u32, u32),
    window: (f32, f32, f32, f32),
) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    let (n_cols, n_rows) = tile_counts(image);
    let side = TILE_SIDE as f32;
    let span = |start: f32, length: f32, n_tiles: u32| {
        let limit = n_tiles as f32 * side;
        let (from, to) = (start.max(0.0), (start + length).min(limit));
        if to <= from { 0..0 } else { (from / side).floor() as u32..((to / side).ceil() as u32).min(n_tiles) }
    };
    (span(window.0, window.2, n_cols), span(window.1, window.3, n_rows))
}

/// 표시 크기가 칸을 넘어 스크롤이 필요한지.
pub fn scrolls(
    shown: (f32, f32),
    pane: (f32, f32),
) -> bool {
    shown.0 > pane.0 || shown.1 > pane.1
}

/// 상태 줄 크기 글자.
pub fn size_text(
    width: u32,
    height: u32,
) -> String {
    format!("{width} × {height}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_img_1_fit_shrinks_to_the_pane_keeping_the_ratio_and_never_enlarges() {
        assert_eq!(display_size((1920, 1080), (960.0, 800.0), Zoom::Fit), (960.0, 540.0));
        assert_eq!(display_size((1000, 2000), (800.0, 500.0), Zoom::Fit), (250.0, 500.0));
        assert_eq!(display_size((100, 50), (800.0, 600.0), Zoom::Fit), (100.0, 50.0));
    }

    #[test]
    fn test_c_img_2_actual_is_the_pixel_size_and_double_click_switches_between_the_two() {
        assert_eq!(display_size((1920, 1080), (400.0, 300.0), Zoom::Actual), (1920.0, 1080.0));
        assert_eq!(Zoom::Fit.toggled(), Zoom::Actual);
        assert_eq!(Zoom::Actual.toggled(), Zoom::Fit);
        assert!(scrolls((1920.0, 1080.0), (400.0, 300.0)));
        assert!(!scrolls((400.0, 300.0), (400.0, 300.0)));
        assert!(!scrolls((100.0, 50.0), (400.0, 300.0)));
    }

    #[test]
    fn test_c_img_3_a_very_tall_picture_fits_the_width_and_scrolls_down() {
        let shown = display_size((800, 8000), (600.0, 400.0), Zoom::Fit);
        assert_eq!(shown, (600.0, 6000.0));
        assert!(scrolls(shown, (600.0, 400.0)));
        // 가로로 긴 그림도 같은 규칙이다.
        assert_eq!(display_size((8000, 800), (400.0, 600.0), Zoom::Fit), (6000.0, 600.0));
        // 긴 그림이어도 칸보다 작으면 키우지 않는다.
        assert_eq!(display_size((100, 400), (600.0, 800.0), Zoom::Fit), (100.0, 400.0));
    }

    #[test]
    fn test_c_img_4_the_size_text_uses_a_times_sign_with_spaces() {
        assert_eq!(size_text(1920, 1080), "1920 × 1080");
        assert_eq!(size_text(1, 1), "1 × 1");
    }

    #[test]
    fn test_c_img_5_images_over_fifty_megapixels_are_refused() {
        assert!(within_limit(7071, 7071));
        assert!(within_limit(50_000, 1000));
        assert!(!within_limit(8000, 8000));
        assert!(!within_limit(u32::MAX, u32::MAX));
    }

    #[test]
    fn test_c_img_6_a_big_picture_is_decoded_at_a_reduced_size_with_the_same_ratio() {
        assert_eq!(decode_size(1920, 1080), (1920, 1080));
        assert_eq!(decode_size(3840, 2160), (3840, 2160));
        assert_eq!(decode_size(4000, 4000), (4000, 4000));
        let (width, height) = decode_size(7000, 7000);
        assert_eq!((width, height), (4000, 4000));
        let (width, height) = decode_size(10_000, 2000);
        assert!(u64::from(width) * u64::from(height) <= DECODE_PIXELS);
        assert!((f64::from(width) / f64::from(height) - 5.0).abs() < 0.01);
        assert_eq!(decode_size(40_000, 1), (MAX_SIDE, 1));
        assert_eq!(decode_size(16_384, 100), (16_384, 100));
        let (width, height) = decode_size(20_000, 800);
        assert!(width <= MAX_SIDE && height <= MAX_SIDE);
        assert!((f64::from(width) / f64::from(height) - 25.0).abs() < 0.5);
        assert!(decode_size(1, 1).0 >= 1);
    }

    #[test]
    fn test_a_fit_view_uploads_the_picture_reduced_to_the_pane_not_the_original() {
        assert_eq!(fit_decode_size(3840, 2160, (1500, 900), DECODE_PIXELS), (1500, 844));
        assert_eq!(fit_decode_size(800, 600, (1500, 900), DECODE_PIXELS), (800, 600));
        assert_eq!(fit_decode_size(1000, 8000, (1500, 900), DECODE_PIXELS), (1000, 8000));
        let (width, height) = fit_decode_size(20_000, 20_000, (30_000, 30_000), DECODE_PIXELS);
        assert!(u64::from(width) * u64::from(height) <= DECODE_PIXELS);
    }

    #[test]
    fn test_tiles_cover_the_image_with_edge_tiles_cut_short() {
        // 1:1로 볼 때 원본을 TILE_SIDE 조각으로 나눈 칸 수와 각 조각의 위치·크기.
        assert_eq!(tile_counts((1920, 1080)), (4, 3));
        assert_eq!(tile_counts((512, 512)), (1, 1));
        assert_eq!(tile_counts((513, 1)), (2, 1));
        assert_eq!(tile_counts((0, 0)), (1, 1));
        assert_eq!(tile_rect((1920, 1080), 0, 0), (0, 0, 512, 512));
        assert_eq!(tile_rect((1920, 1080), 3, 2), (1536, 1024, 384, 56));
        // 모든 조각의 넓이를 합치면 원본 픽셀 수와 같다.
        let (cols, rows) = tile_counts((3840, 2160));
        let total: u64 = (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (col, row)))
            .map(|(col, row)| {
                let (_, _, width, height) = tile_rect((3840, 2160), col, row);
                u64::from(width) * u64::from(height)
            })
            .sum();
        assert_eq!(total, 3840 * 2160);
    }

    #[test]
    fn test_only_the_tiles_touching_the_visible_window_are_wanted() {
        // 창(x, y, 가로, 세로: 원본 픽셀 좌표)과 겹치는 조각의 칸 범위.
        assert_eq!(visible_tiles((3840, 2160), (0.0, 0.0, 1500.0, 900.0)), (0..3, 0..2));
        // 조각 경계에 정확히 닿는 쪽은 겹치지 않는다.
        assert_eq!(visible_tiles((3840, 2160), (0.0, 0.0, 512.0, 512.0)), (0..1, 0..1));
        assert_eq!(visible_tiles((3840, 2160), (512.0, 512.0, 512.0, 512.0)), (1..2, 1..2));
        // 가운데로 스크롤하면 양쪽 경계에 걸친 조각이 모두 든다.
        assert_eq!(visible_tiles((3840, 2160), (1000.0, 600.0, 1500.0, 900.0)), (1..5, 1..3));
        // 원본 밖으로 나간 창은 원본 안으로 잘라 본다.
        assert_eq!(visible_tiles((3840, 2160), (3000.0, 1500.0, 5000.0, 5000.0)), (5..8, 2..5));
        assert_eq!(visible_tiles((3840, 2160), (-300.0, -300.0, 500.0, 500.0)), (0..1, 0..1));
        // 창이 원본을 통째로 벗어나면 비어 있다.
        assert!(visible_tiles((3840, 2160), (9000.0, 0.0, 500.0, 500.0)).0.is_empty());
        // 작은 이미지는 조각이 하나다.
        assert_eq!(visible_tiles((100, 50), (0.0, 0.0, 800.0, 600.0)), (0..1, 0..1));
    }
}
