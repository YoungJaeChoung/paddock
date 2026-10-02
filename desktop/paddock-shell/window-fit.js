// 최대화한 창의 내용이 창 크기를 따라왔는지 판정하는 순수 함수.
// Windows에서 최대화한 창이 배율(DPI)이 다른 모니터에 걸리면 창 틀만 커지고 내용은 이전 크기로 남는 경우가 있다.

// 창 내용 크기와 화면 크기의 허용 차이(DIP). 테두리·반올림 오차는 몇 픽셀이라 넉넉히 둔다.
const N_TOLERANCE_DIP = 8;

/**
 * 화면(웹 페이지)이 창 내용 크기를 따라오지 못했는지. 화면 크기(CSS px)에 확대 배율을 곱하면 창 내용 크기(DIP)와 같아야 한다.
 *
 * Examples
 * --------
 * | 창 내용(DIP) | 화면(CSS px) | 확대 | 결과  |
 * | ------------ | ------------ | ---- | ----- |
 * | 1728x3000    | 1200x2083    | 1.44 | false |
 * | 1728x3000    | 949x569      | 1.44 | true  |
 */
function isViewportStale(
    contentSize,
    viewportSize,
    zoomFactor,
) {
    const [contentWidth, contentHeight] = contentSize;
    const [viewportWidth, viewportHeight] = viewportSize;
    return Math.abs(viewportWidth * zoomFactor - contentWidth) > N_TOLERANCE_DIP
        || Math.abs(viewportHeight * zoomFactor - contentHeight) > N_TOLERANCE_DIP;
}

module.exports = { isViewportStale };
