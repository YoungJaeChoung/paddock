//! 셀 격자 위 글자 배치 계산: 전각·이모지 표시(VS16)가 차지하는 칸 수와 칸 안 가운데 맞춤.
//! 화면 그리기와 무관한 순수 계산이라 글꼴이나 그리기 도구 없이 값으로 검증한다.
/// 격자에 놓인 글자 하나. 칸 수(`span`)를 정하는 입력이자 결과다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellGlyph {
    pub row: i32,
    pub col: usize,
    /// 글리프를 그리는 칸인가(빈 칸·두 번째 칸 자리는 false).
    pub drawn: bool,
    /// 격자는 1칸으로 세지만 이모지 표시(VS16)라 2칸 폭으로 그리고 싶은 글자.
    pub vs16: bool,
    /// 그릴 칸 수(1 또는 2).
    pub span: usize,
}

/// 글자가 처음 차지하려는 칸 수. 전각이거나 VS16 글자면 2, 그 밖에는 1이다.
pub fn base_span(
    wide: bool,
    has_vs16: bool,
) -> usize {
    if wide || has_vs16 { 2 } else { 1 }
}

/// VS16 글자의 오른쪽 칸에 다른 글자가 있으면 1칸으로 줄여 겹치지 않게 한다. 오른쪽 칸이 비어 있으면 2칸을 유지한다.
///
/// `cells`는 행 우선, 같은 행에서는 열 오름차순이다(화면을 위에서 아래로 훑은 순서). 그래서 오른쪽 칸의 글자는
/// 바로 다음 원소로 찾고, 화면마다 칸 집합을 새로 만들지 않는다. 그리지 않는 칸은 목록에 있어도 없어도 같다.
///
/// Examples
/// --------
/// | 줄 내용  | 이모지 칸 수 |
/// |----------|--------------|
/// | `⚠️a`    | 1            |
/// | `⚠️ a`   | 2            |
pub fn shrink_blocked_vs16_spans(cells: &mut [CellGlyph]) {
    for index in 0..cells.len() {
        let right = cells.get(index + 1).copied();
        let cell = &mut cells[index];
        let is_blocked = right.is_some_and(|right| right.drawn && right.row == cell.row && right.col == cell.col + 1);
        if cell.vs16 && is_blocked {
            cell.span = 1;
        }
    }
}

/// 글리프를 칸에 맞추려고 글꼴 크기에 곱할 배율. 맞출 필요가 없으면 1.0이다.
///
/// 2칸 글자는 자연 폭이 목표(2칸 폭)와 0.5 이상 다르면 목표에 맞추고, 1칸 글자는 목표를 넘칠 때만 줄인다(키우지 않는다).
///
/// Examples
/// --------
/// | natural | span | cell_w | 결과             |
/// |---------|------|--------|------------------|
/// | 10      | 2    | 8      | 1.6              |
/// | 16.2    | 2    | 8      | 1.0              |
/// | 12      | 1    | 8      | 8 / 12           |
/// | 5       | 1    | 8      | 1.0              |
pub fn fit_scale(
    natural_width: f32,
    span: usize,
    cell_w: f32,
) -> f32 {
    let target = span as f32 * cell_w;
    let need_fit = if span == 2 { (natural_width - target).abs() > 0.5 } else { natural_width > target + 0.5 };
    if need_fit && natural_width > 0.0 { target / natural_width } else { 1.0 }
}

/// 칸 시작에서 글리프를 그릴 x 오프셋. `span`칸 폭 안에서 글리프를 가운데에 놓는다.
pub fn center_offset(
    span: usize,
    cell_w: f32,
    glyph_width: f32,
) -> f32 {
    (span as f32 * cell_w - glyph_width) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(
        col: usize,
        drawn: bool,
        vs16: bool,
    ) -> CellGlyph {
        CellGlyph { row: 0, col, drawn, vs16, span: base_span(false, vs16) }
    }

    #[test]
    fn test_c_d17_vs16_span_two_cells_when_right_empty() {
        // "⚠️ a": 이모지 오른쪽 칸이 비어 있으므로 두 칸을 그대로 쓴다.
        let mut cells = vec![cell(0, true, true), cell(2, true, false)];
        shrink_blocked_vs16_spans(&mut cells);
        assert_eq!(cells[0].span, 2);
        assert_eq!(cells[1].span, 1);
    }

    #[test]
    fn test_c_d17_vs16_span_one_cell_when_right_occupied() {
        // "⚠️a": 오른쪽 칸에 다음 글자가 있으므로 겹치지 않도록 이모지를 한 칸으로 줄인다.
        let mut cells = vec![cell(0, true, true), cell(1, true, false)];
        shrink_blocked_vs16_spans(&mut cells);
        assert_eq!(cells[0].span, 1);
        assert_eq!(cells[1].span, 1);
    }

    #[test]
    fn test_vs16_occupancy_is_per_row() {
        let mut cells = vec![cell(0, true, true), CellGlyph { row: 1, col: 1, drawn: true, vs16: false, span: 1 }];
        shrink_blocked_vs16_spans(&mut cells);
        assert_eq!(cells[0].span, 2);
    }

    #[test]
    fn test_base_span() {
        assert_eq!(base_span(true, false), 2);
        assert_eq!(base_span(false, true), 2);
        assert_eq!(base_span(false, false), 1);
        // 전각 글자는 이미 2칸이므로 VS16으로 줄일 대상이 아니다.
        assert!(!cell(0, true, false).vs16);
    }

    #[test]
    fn test_c_d5a_wide_glyph_centered_in_two_cells() {
        let cell_w = 8.0;
        // 자연 폭이 10인 전각 글리프는 정확히 2칸(16)을 채우도록 늘려 오프셋이 없다.
        let scale = fit_scale(10.0, 2, cell_w);
        assert!((scale - 1.6).abs() < 1e-6);
        assert!((10.0 * scale - 2.0 * cell_w).abs() < 1e-4);
        assert!(center_offset(2, cell_w, 10.0 * scale).abs() < 1e-4);
        // 다시 모양을 잡은 뒤 폭이 목표와 조금 다를 수 있으며, 그때는 2칸 가운데에 놓는다.
        assert!((center_offset(2, cell_w, 15.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_fit_scale_rules() {
        // 목표와 반 픽셀 이내면 그대로 둔다.
        assert_eq!(fit_scale(16.2, 2, 8.0), 1.0);
        // 1칸 글리프는 줄이기만 하고 늘리지 않는다.
        assert_eq!(fit_scale(5.0, 1, 8.0), 1.0);
        assert!((fit_scale(12.0, 1, 8.0) - 8.0 / 12.0).abs() < 1e-6);
        assert_eq!(fit_scale(0.0, 2, 8.0), 1.0);
    }
}
