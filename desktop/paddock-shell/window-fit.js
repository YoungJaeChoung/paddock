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

/**
 * 창이 실제로 화면 작업 영역(작업 표시줄을 뺀 영역)을 채우는지. 최대화한 창은 테두리만큼 작업 영역 밖으로 조금 넘친다.
 * Windows는 최대화 표시가 남은 채 창 크기만 바뀌는 경우가 있다(최대화한 창에 위치·크기를 직접 지정할 때).
 * 이때 최대화를 풀었다 다시 걸면 창이 최대화 전 자리(다른 모니터일 수 있다)의 최대화 크기로 튄다. 그래서 다시 걸기 전에 실제 크기로 확인한다.
 *
 * Examples
 * --------
 * | 창(x,y,w,h)           | 작업 영역(x,y,w,h) | 결과  |
 * | --------------------- | ------------------ | ----- |
 * | -8,-8,2576,1568       | 0,0,2560,1552      | true  |
 * | 200,200,1800,1100     | 0,0,2560,1552      | false |
 */
function fillsWorkArea(
    bounds,
    workArea,
) {
    return bounds.x <= workArea.x + N_TOLERANCE_DIP
        && bounds.y <= workArea.y + N_TOLERANCE_DIP
        && bounds.x + bounds.width >= workArea.x + workArea.width - N_TOLERANCE_DIP
        && bounds.y + bounds.height >= workArea.y + workArea.height - N_TOLERANCE_DIP;
}

/**
 * 창 위치를 작업 영역 안으로 옮긴 값. 작업 영역보다 크면 작업 영역 크기로 줄인다.
 * 최대화를 다시 걸 때 최대화 전 자리가 다른 모니터에 있으면 지금 모니터 안으로 옮겨, 최대화가 지금 모니터에 걸리게 하려고 존재한다.
 *
 * Examples
 * --------
 * | 창(x,y,w,h)            | 작업 영역(x,y,w,h)     | 결과                   |
 * | ---------------------- | ---------------------- | ---------------------- |
 * | -1000,-1900,1600,1000  | 0,0,2560,1552          | 0,0,1600,1000          |
 * | 100,100,800,600        | 0,0,2560,1552          | 100,100,800,600        |
 * | 0,0,3000,2000          | 0,0,2560,1552          | 0,0,2560,1552          |
 */
function boundsWithin(
    bounds,
    workArea,
) {
    const width = Math.min(bounds.width, workArea.width);
    const height = Math.min(bounds.height, workArea.height);
    const clamp = (value, low, high) => Math.min(Math.max(value, low), high);
    return {
        x: clamp(bounds.x, workArea.x, workArea.x + workArea.width - width),
        y: clamp(bounds.y, workArea.y, workArea.y + workArea.height - height),
        width,
        height,
    };
}

module.exports = { isViewportStale, fillsWorkArea, boundsWithin };
