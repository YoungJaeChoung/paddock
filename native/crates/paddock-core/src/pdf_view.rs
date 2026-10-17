//! PDF 탭의 규칙: 배율 단계, 칸 폭 맞춤 배율, 보이는 쪽 계산, 그려 둘 쪽과 캐시 축출, 쪽 글자.
//!
//! 쪽을 그리는 일과 스크롤 상자는 앱이 하고, 여기서는 숫자만 받아 계산하는 순수 규칙만 둔다.
//! 메모리가 쪽 수가 아니라 "그려 둔 쪽의 픽셀"에 비례하므로(쪽당 가로×세로×4바이트) 보이는 쪽 우선·개수·픽셀 상한 세 가지로 캐시를 묶는다.

/// 배율 단계(%). `+`는 지금보다 큰 다음 단계로, `−`는 작은 이전 단계로 간다.
pub const ZOOM_STEPS: [u32; 7] = [50, 75, 100, 125, 150, 200, 300];

/// 칸 폭 맞춤 배율의 상한(%). 쪽이 칸보다 한참 좁아도 이보다 키우지 않는다.
pub const FIT_MAX_PERCENT: u32 = 200;

/// 100 % 배율에서 PDF 1pt가 차지하는 화면 픽셀 수(96dpi ÷ 72pt).
pub const PX_PER_POINT: f32 = 96.0 / 72.0;

/// 쪽 사이 간격과 쪽 둘레 여백(논리 px).
pub const PAGE_GAP: f32 = 12.0;

/// 그려 두는 쪽 수의 기본값: 보이는 쪽이 이보다 적으면 보이는 구간 앞뒤 한 쪽을 더해 이 수까지 채운다.
pub const CACHE_PAGES: usize = 2;

/// 보이는 쪽이 많아도(작은 배율에서 높은 창) 한꺼번에 그려 두는 쪽 수의 절대 상한. 보이는 쪽은 `CACHE_PAGES`를 넘어도 모두 그린다.
pub const MAX_VISIBLE_HELD: usize = 8;

/// 그려 둔 쪽들의 픽셀 수 합의 상한(1,600만 ≈ 64 MB). 배율이 커서 쪽 하나가 크면 3쪽보다 적게 둔다.
pub const CACHE_PIXELS: u64 = 16_000_000;

/// 쪽 하나를 그릴 때 가로세로 픽셀 수의 상한(1,200만). 넘으면 배율을 낮춰 그린다.
pub const PAGE_PIXELS: u64 = 12_000_000;

/// 지금 배율에서 칸 폭에 맞는 배율(%). 칸에서 쪽 둘레 여백을 빼고 맞추며, 최소 단계와 `FIT_MAX_PERCENT` 사이로 자른다.
/// 쪽 폭을 모르면(0) 100 %다.
pub fn fit_percent(
    page_width_pt: f32,
    pane_width_px: f32,
) -> u32 {
    if page_width_pt <= 0.0 {
        100
    } else {
        let usable = (pane_width_px - 2.0 * PAGE_GAP).max(0.0);
        let percent = (usable / (page_width_pt * PX_PER_POINT) * 100.0).round() as u32;
        percent.clamp(ZOOM_STEPS[0], FIT_MAX_PERCENT)
    }
}

/// `percent`보다 큰 다음 단계. 마지막 단계면 그대로다.
pub fn zoom_in(percent: u32) -> u32 {
    ZOOM_STEPS.iter().copied().find(|&step| step > percent).unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
}

/// `percent`보다 작은 이전 단계. 첫 단계면 그대로다.
pub fn zoom_out(percent: u32) -> u32 {
    ZOOM_STEPS.iter().rev().copied().find(|&step| step < percent).unwrap_or(ZOOM_STEPS[0])
}

/// 쪽 하나를 `percent`로 그릴 화면 픽셀 크기(가로, 세로). `PAGE_PIXELS`를 넘으면 비율을 지켜 줄인다.
pub fn page_pixels(
    size_pt: (f32, f32),
    percent: u32,
) -> (u32, u32) {
    let scale = PX_PER_POINT * percent as f32 / 100.0;
    let (width, height) = ((size_pt.0 * scale).max(1.0), (size_pt.1 * scale).max(1.0));
    let pixels = f64::from(width) * f64::from(height);
    let shrink = if pixels > PAGE_PIXELS as f64 { (PAGE_PIXELS as f64 / pixels).sqrt() as f32 } else { 1.0 };
    (((width * shrink).round() as u32).max(1), ((height * shrink).round() as u32).max(1))
}

/// 스크롤한 위치에서 보이는 쪽 번호(0부터, 오름차순). `heights`는 쪽마다의 높이(논리 px), 쪽 사이와 맨 위·아래에 `PAGE_GAP`이 있다.
/// 보이는 쪽이 없으면(목록 끝을 지났거나 쪽이 없음) 빈 목록이다.
pub fn visible_pages(
    scroll_top: f32,
    viewport_height: f32,
    heights: &[f32],
) -> Vec<usize> {
    let (view_top, view_bottom) = (scroll_top, scroll_top + viewport_height);
    let mut top = PAGE_GAP;
    let mut visible = Vec::new();
    for (index, height) in heights.iter().enumerate() {
        let bottom = top + height;
        if bottom > view_top && top < view_bottom {
            visible.push(index);
        }
        top = bottom + PAGE_GAP;
    }
    visible
}

/// 그려 둘 쪽을 중요한 순서로: 보이는 쪽(최대 `MAX_VISIBLE_HELD`), 그다음 보이는 구간 바로 앞 한 쪽과 뒤 한 쪽. 앞뒤 쪽은 전체가 `limit`개가 될 때까지만 더한다.
pub fn pages_to_hold(
    visible: &[usize],
    n_pages: usize,
    limit: usize,
) -> Vec<usize> {
    let mut hold: Vec<usize> = visible.iter().copied().filter(|&page| page < n_pages).take(MAX_VISIBLE_HELD).collect();
    if let (Some(&first), Some(&last)) = (visible.first(), visible.last()) {
        let neighbours = [first.checked_sub(1), Some(last + 1).filter(|&next| next < n_pages)];
        for neighbour in neighbours.into_iter().flatten() {
            if hold.len() < limit && !hold.contains(&neighbour) {
                hold.push(neighbour);
            }
        }
    }
    hold
}

/// 이미 그려 둔 쪽 중 버릴 쪽. `hold`에 없는 쪽을 버리고, 남은 것이 픽셀 상한을 넘으면 `hold` 뒤쪽(덜 중요한 쪽)부터 더 버린다.
/// `hold`의 첫 쪽(가장 중요한 쪽)은 상한을 넘어도 남는다.
///
/// `cached`는 (쪽 번호, 픽셀 수) 목록이다. 버릴 쪽은 `cached` 순서로 돌려준다.
pub fn pages_to_drop(
    cached: &[(usize, u64)],
    hold: &[usize],
) -> Vec<usize> {
    let mut kept: Vec<(usize, u64)> = cached.iter().copied().filter(|(page, _)| hold.contains(page)).collect();
    // 덜 중요한 순서(hold의 뒤쪽 먼저)로 정렬해 픽셀 합이 상한 안에 들 때까지 앞에서부터 뺀다.
    kept.sort_by_key(|(page, _)| std::cmp::Reverse(hold.iter().position(|held| held == page)));
    let mut total: u64 = kept.iter().map(|(_, pixels)| pixels).sum();
    let mut dropped_for_budget = Vec::new();
    while total > CACHE_PIXELS && kept.len() > 1 {
        let (page, pixels) = kept.remove(0);
        total -= pixels;
        dropped_for_budget.push(page);
    }
    cached
        .iter()
        .map(|(page, _)| *page)
        .filter(|page| !hold.contains(page) || dropped_for_budget.contains(page))
        .collect()
}

/// 쪽 하나의 그림 상태: 그려졌거나(`Drawn`), 그리지 못했다(`Failed`). `percent`는 그릴 때 쓴 배율(화면 픽셀 기준 %)이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSlot {
    Drawn { percent: u32 },
    Failed { percent: u32 },
}

/// 쪽을 `percent` 배율로 그리라고 요청해야 하는지. 이미 그렸거나 같은 배율에서 그리지 못한 쪽, 이미 그리는 중인 쪽은 요청하지 않는다.
/// 배율이 달라지면 실패한 쪽도 한 번 다시 시도한다.
///
/// Examples
/// --------
/// | 상태               | 같은 배율 | 그리는 중 | 결과 |
/// |--------------------|-----------|-----------|------|
/// | 없음               | -         | 아니오    | 참   |
/// | 그렸음             | 예        | 아니오    | 거짓 |
/// | 그렸음             | 아니오    | 아니오    | 참   |
/// | 실패               | 예        | 아니오    | 거짓 |
/// | 실패               | 아니오    | 아니오    | 참   |
/// | 없음               | -         | 예        | 거짓 |
pub fn should_request(
    slot: Option<PageSlot>,
    percent: u32,
    pending: bool,
) -> bool {
    let needed = match slot {
        None => true,
        Some(PageSlot::Drawn { percent: drawn }) | Some(PageSlot::Failed { percent: drawn }) => drawn != percent,
    };
    needed && !pending
}

/// 그리기 직전에 이 쪽이 아직 필요한지: 지금 그려 두기로 한 쪽 목록에 있고 배율도 그대로여야 한다. 스크롤이 빨라 이미 지나간 쪽은 건너뛴다.
pub fn is_still_wanted(
    page: usize,
    percent: u32,
    hold: &[usize],
    current_percent: u32,
) -> bool {
    percent == current_percent && hold.contains(&page)
}

/// 상태 줄·툴바에 보이는 쪽 글자. 보이는 첫 쪽 번호(1부터)와 전체 쪽 수다.
pub fn page_text(
    first_visible: usize,
    n_pages: usize,
) -> String {
    if n_pages == 0 { "0 / 0".to_string() } else { format!("{} / {n_pages}", first_visible + 1) }
}

/// 자동 점검 상태 글에 보이는 쪽 번호 목록(1부터, 쉼표).
pub fn visible_text(visible: &[usize]) -> String {
    visible.iter().map(|page| (page + 1).to_string()).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_pdf_1_the_visible_pages_are_the_ones_the_viewport_overlaps() {
        // 쪽 높이 100, 간격 12: 쪽 0은 12..112, 쪽 1은 124..224, 쪽 2는 236..336.
        let heights = [100.0, 100.0, 100.0];
        assert_eq!(visible_pages(0.0, 50.0, &heights), vec![0]);
        assert_eq!(visible_pages(0.0, 130.0, &heights), vec![0, 1]);
        assert_eq!(visible_pages(100.0, 100.0, &heights), vec![0, 1]);
        assert_eq!(visible_pages(115.0, 20.0, &heights), vec![1]);
        assert_eq!(visible_pages(200.0, 300.0, &heights), vec![1, 2]);
        assert_eq!(visible_pages(0.0, 1000.0, &heights), vec![0, 1, 2]);
        assert_eq!(visible_pages(0.0, 100.0, &[]), Vec::<usize>::new());
        // 끝을 지나 스크롤해도 비어 있지 않게 마지막 쪽이 남지는 않는다(보이는 것이 없으면 빈 목록).
        assert_eq!(visible_pages(5000.0, 100.0, &heights), Vec::<usize>::new());
    }

    #[test]
    fn test_c_pdf_2_zoom_steps_move_one_notch_and_stop_at_both_ends() {
        assert_eq!(zoom_in(100), 125);
        assert_eq!(zoom_in(110), 125);
        assert_eq!(zoom_in(300), 300);
        assert_eq!(zoom_out(100), 75);
        assert_eq!(zoom_out(60), 50);
        assert_eq!(zoom_out(50), 50);
        // 칸 폭 맞춤에서 시작해도 한 걸음은 단계 하나다.
        assert_eq!(zoom_in(fit_percent(595.0, 700.0)), 100);
    }

    #[test]
    fn test_c_pdf_2_fit_percent_fills_the_pane_width_within_the_step_range() {
        // A4(595pt)는 100 %에서 595 × 4/3 ≈ 793px. 칸 폭 700 - 여백 24 = 676 → 676 / 793 ≈ 85 %.
        assert_eq!(fit_percent(595.0, 700.0), 85);
        assert_eq!(fit_percent(595.0, 100.0), 50);
        assert_eq!(fit_percent(100.0, 2000.0), 200);
        assert_eq!(fit_percent(0.0, 700.0), 100);
    }

    #[test]
    fn test_c_pdf_2_a_huge_zoom_on_a_huge_page_is_drawn_smaller_instead_of_failing() {
        assert_eq!(page_pixels((595.0, 842.0), 100), (793, 1123));
        let (width, height) = page_pixels((14400.0, 14400.0), 300);
        assert!(u64::from(width) * u64::from(height) <= PAGE_PIXELS);
        assert!(width >= 1 && height >= 1);
        let (width, height) = page_pixels((595.0, 842.0), 300);
        assert_eq!((width, height), (2380, 3368));
    }

    #[test]
    fn test_c_pdf_3_the_pages_to_hold_put_the_visible_ones_first_then_the_neighbours() {
        assert_eq!(pages_to_hold(&[4], 10, 3), vec![4, 3, 5]);
        assert_eq!(pages_to_hold(&[0], 10, 3), vec![0, 1]);
        assert_eq!(pages_to_hold(&[9], 10, 3), vec![9, 8]);
        assert_eq!(pages_to_hold(&[2, 3], 10, 3), vec![2, 3, 1]);
        assert_eq!(pages_to_hold(&[1, 2, 3, 4], 10, 3), vec![1, 2, 3, 4]);
        let many: Vec<usize> = (0..12).collect();
        assert_eq!(pages_to_hold(&many, 20, 3), (0..8).collect::<Vec<_>>());
        assert_eq!(pages_to_hold(&[], 10, 3), Vec::<usize>::new());
        assert_eq!(pages_to_hold(&[0], 1, 3), vec![0]);
    }

    #[test]
    fn test_c_pdf_3_pages_outside_the_hold_list_are_dropped_and_the_pixel_budget_drops_the_least_important_more() {
        let small = 1_000_000;
        assert_eq!(pages_to_drop(&[(0, small), (1, small), (2, small), (7, small)], &[4, 3, 5]), vec![0, 1, 2, 7]);
        assert_eq!(pages_to_drop(&[(4, small), (3, small)], &[4, 3, 5]), Vec::<usize>::new());
        // 쪽 하나가 커서 셋을 다 두면 픽셀 상한을 넘는다: 덜 중요한 쪽(hold 뒤쪽)부터 버린다. 가장 중요한 쪽은 항상 남는다.
        let big = 7_000_000;
        assert_eq!(pages_to_drop(&[(4, big), (3, big), (5, big)], &[4, 3, 5]), vec![5]);
        let huge = 20_000_000;
        assert_eq!(pages_to_drop(&[(4, huge), (3, huge)], &[4, 3, 5]), vec![3]);
    }

    #[test]
    fn test_c_pdf_4_page_text_counts_from_one_and_visible_text_joins_with_commas() {
        assert_eq!(page_text(2, 12), "3 / 12");
        assert_eq!(page_text(0, 3), "1 / 3");
        assert_eq!(page_text(0, 0), "0 / 0");
        assert_eq!(visible_text(&[0, 1]), "1,2");
        assert_eq!(visible_text(&[]), "");
    }

    #[test]
    fn test_c_review_2_a_page_that_failed_is_not_requested_again_until_the_zoom_changes() {
        let drawn = Some(PageSlot::Drawn { percent: 100 });
        let failed = Some(PageSlot::Failed { percent: 100 });
        assert!(should_request(None, 100, false));
        assert!(!should_request(drawn, 100, false));
        assert!(should_request(drawn, 150, false));
        assert!(!should_request(failed, 100, false), "no endless retries");
        assert!(should_request(failed, 150, false), "one more try after a zoom change");
        assert!(!should_request(None, 100, true));
        assert!(!should_request(failed, 150, true));
    }

    #[test]
    fn test_c_review_11_a_queued_render_is_skipped_once_the_page_scrolled_away_or_the_zoom_changed() {
        assert!(is_still_wanted(4, 100, &[4, 3, 5], 100));
        assert!(!is_still_wanted(9, 100, &[4, 3, 5], 100));
        assert!(!is_still_wanted(4, 100, &[4, 3, 5], 125));
    }
}
