/** Resolves a pane's edge or center and the corresponding docking preview. */
function tabDropTarget(rect, x, y) {
    const { left, top, width, height } = rect;
    if (width <= 0 || height <= 0 || x < left || y < top || x >= left + width || y >= top + height) return null;
    const edges = [
        [(x - left) / width, 'split-left', { left, top, width: width / 2, height }],
        [(left + width - x) / width, 'split-right', { left: left + width / 2, top, width: width / 2, height }],
        [(y - top) / height, 'split-top', { left, top, width, height: height / 2 }],
        [(top + height - y) / height, 'split-bottom', { left, top: top + height / 2, width, height: height / 2 }],
    ].sort((a, b) => a[0] - b[0]);
    return edges[0][0] < 0.25 ? { mode: edges[0][1], rect: edges[0][2] } : { mode: 'tab-after', rect: { left, top, width, height } };
}

/**
 * 끈 탭을 칸에 놓을 때 기준이 될 탭 id. 놓아도 바뀌는 것이 없으면 null이다.
 *
 * 끈 탭이 이미 그 칸에 있으면 가운데(`tab-after`)에 놓아도 갈 곳이 없다. 이때 기준 탭을 고르면
 * 끈 탭이 그 탭 뒤로 옮겨져 사용자가 의도하지 않은 순서 바꾸기가 되므로 미리보기·이동 모두 하지 않는다.
 * 가장자리 분할에서 끈 탭이 그 칸의 현재 탭이면 자기를 뺀 첫 탭을 기준으로 삼는다. 칸에 탭이 하나뿐이면 나눌 수 없어 null이다.
 *
 * | mode | titleIds | currentId | id | 결과 |
 * |---|---|---|---|---|
 * | split-right | [a, b] | a | a | b |
 * | split-right | [a] | a | a | null |
 * | tab-after | [a, b] | b | a | null |
 * | tab-after | [c] | c | a | c |
 */
function tabDockRef(
    mode,
    titleIds,
    currentId,
    id,
) {
    const isOwnPane = titleIds.includes(id);
    let ref = currentId;
    if (isOwnPane && mode === 'tab-after') ref = null;
    else if (currentId === id) ref = titleIds.find(titleId => titleId !== id) ?? null;
    return ref;
}

module.exports = { tabDropTarget, tabDockRef };
