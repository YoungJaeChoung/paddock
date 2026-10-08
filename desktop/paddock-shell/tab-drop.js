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

/**
 * 탭 줄 위에 놓을 때 끼워 넣을 자리. 가리킨 탭의 왼쪽 절반이면 그 탭 앞, 오른쪽 절반이면 뒤다.
 * 첫 탭 앞·탭 사이 빈 곳·줄 끝은 오른쪽에서 가장 가까운 탭(없으면 마지막 탭)의 그쪽 가장자리로 본다. 탭이 없으면 null이다.
 *
 * @param tabs 줄에 보이는 차례대로의 `{ id, left, right }`.
 * @param x 놓는 지점의 가로 좌표.
 * @returns `{ mode, refId, edge }` — `edge`는 끼워 넣을 자리의 가로 좌표.
 *
 * | tabs | x | 결과 |
 * |---|---|---|
 * | a 0–100, b 100–200 | 30 | a 앞 (edge 0) |
 * | a 0–100, b 100–200 | 160 | b 뒤 (edge 200) |
 * | a 0–100, b 100–200 | 260 | b 뒤 (줄 끝) |
 * | (없음) | 50 | null |
 */
function tabRowInsert(
    tabs,
    x,
) {
    let insert = null;
    if (tabs.length) {
        const hit = tabs.find(tab => x < tab.right) ?? tabs[tabs.length - 1];
        const isBefore = x < (hit.left + hit.right) / 2;
        insert = { mode: isBefore ? 'tab-before' : 'tab-after', refId: hit.id, edge: isBefore ? hit.left : hit.right };
    }
    return insert;
}

/**
 * 끈 탭을 끼워 넣어도 칸 안 순서가 그대로인지. 끈 탭이 그 칸에 없으면(다른 칸으로 옮김) 늘 false다.
 * 바뀌는 것이 없는 자리에는 미리보기·이동을 하지 않는다.
 *
 * | paneIds | id | mode | refId | 결과 |
 * |---|---|---|---|---|
 * | [a, b, c] | a | tab-before | b | true (a는 이미 b 앞) |
 * | [a, b, c] | a | tab-after | a | true (자기 자신) |
 * | [a, b, c] | c | tab-after | b | true |
 * | [a, b, c] | a | tab-after | b | false |
 * | [b, c] | a | tab-before | b | false (다른 칸) |
 */
function isSameTabPlace(
    paneIds,
    id,
    mode,
    refId,
) {
    const from = paneIds.indexOf(id);
    const at = paneIds.indexOf(refId);
    return from >= 0 && at >= 0 && (refId === id || (mode === 'tab-before' ? at === from + 1 : at === from - 1));
}

/**
 * 같은 칸 안에서 끈 탭이 옮겨 가 앉을 번호. 끈 탭이 그 칸에 없으면(다른 칸으로 옮김) null이다.
 *
 * Lumino의 탭 끼워 넣기(`tab-before`/`tab-after`)는 기준 탭의 원래 번호에 끈 탭을 옮겨 놓는데, 옮기는 동안 끈 탭이 빠지면서
 * 오른쪽 탭들이 한 칸씩 당겨진다. 그래서 오른쪽으로 옮길 때 한 칸 더 뒤에 앉는다. 이 함수는 빠진 뒤의 번호를 돌려준다.
 *
 * | paneIds | id | mode | refId | 결과 |
 * |---|---|---|---|---|
 * | [a, b, c] | a | tab-before | c | 1 (b, a, c) |
 * | [a, b, c] | a | tab-after | b | 1 (b, a, c) |
 * | [a, b, c] | a | tab-after | c | 2 (b, c, a) |
 * | [a, b, c] | c | tab-before | a | 0 (c, a, b) |
 * | [b, c] | a | tab-before | b | null (다른 칸) |
 */
function tabMoveIndex(
    paneIds,
    id,
    mode,
    refId,
) {
    const from = paneIds.indexOf(id);
    const slot = paneIds.indexOf(refId) + (mode === 'tab-after' ? 1 : 0);
    return from >= 0 && paneIds.includes(refId) ? slot - (from < slot ? 1 : 0) : null;
}

module.exports = { tabDropTarget, tabDockRef, tabRowInsert, isSameTabPlace, tabMoveIndex };
