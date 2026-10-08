// 본문 칸의 화면 위치로 위쪽 탭 줄의 열(column)을 정한다.
// 칸을 나누면 탭은 그 화면이 든 칸 바로 위에 놓여야 하므로, 왼쪽 가장자리가 같은 칸들을 한 열로 묶고 열을 왼쪽부터 늘어놓는다.
// DOM 측정과 그리기는 호출자가 맡고, 이 모듈은 숫자와 키만 다룬다.

/**
 * 칸들을 왼쪽 가장자리가 같은 열로 묶는다.
 *
 * 열 하나는 위쪽 탭 줄의 묶음 하나가 된다. 옆으로 나눈 칸은 열이 다르고, 위아래로 나눈 칸은 같은 열에 위에서 아래 순으로 든다.
 * `tolerance`(px) 안의 왼쪽 차이는 같은 열로 본다(경계선 두께·소수점 오차).
 *
 * Examples:
 *   panes                                        → columns
 *   [{key:a,left:0,top:0}]                       → [{ left: 0, keys: [a] }]
 *   [{a,0,0},{b,500,0}]                          → [{ left: 0, keys: [a] }, { left: 500, keys: [b] }]
 *   [{a,0,0},{b,500,0},{c,500,300}]              → [{ left: 0, keys: [a] }, { left: 500, keys: [b, c] }]
 *   []                                           → []
 */
function paneColumns(
    panes,
    tolerance = 1,
) {
    const columns = [];
    const ordered = [...panes].sort((first, second) => first.left - second.left || first.top - second.top);
    for (const pane of ordered) {
        const column = columns.find(item => Math.abs(item.left - pane.left) <= tolerance);
        if (column) column.keys.push(pane.key);
        else columns.push({ left: pane.left, keys: [pane.key] });
    }
    return columns;
}

/**
 * 탭을 열 순서로 나눈다. 열 안에서는 칸 순서(위→아래), 같은 칸 안에서는 `ids`의 원래 순서를 지킨다.
 *
 * `paneOf(id)`는 그 탭의 화면이 든 칸의 키, 칸이 없으면(다른 창에 분리된 탭 등) null이다.
 * 칸이 없는 탭은 마지막 열 뒤에 붙인다. 열이 없으면 모든 탭이 한 묶음이다.
 *
 * Examples:
 *   ids        columns                 paneOf          → groups
 *   [t1,t2,f]  [[A],[B]]               t1→A,t2→B,f→B   → [[t1],[t2,f]]
 *   [t1,t2]    [[A],[B,C]]             t1→C,t2→B       → [[],[t2,t1]]
 *   [t1,w]     [[A]]                   t1→A,w→null     → [[t1,w]]
 *   [t1]       []                      t1→null         → [[t1]]
 */
function groupTabsByColumn(
    ids,
    paneOf,
    columns,
) {
    let groups;
    if (!columns.length) {
        groups = [[...ids]];
    } else {
        groups = columns.map(column => column.keys.flatMap(key => ids.filter(id => paneOf(id) === key)));
        const placed = new Set(groups.flat());
        groups[groups.length - 1].push(...ids.filter(id => !placed.has(id)));
    }
    return groups;
}

/**
 * 열마다 위쪽 탭 줄 안에서 차지할 가로 구간을 정한다. 열은 다음 열의 왼쪽까지, 마지막 열은 줄 끝까지 뻗는다.
 *
 * `stripLeft`·`stripWidth`는 탭 줄의 화면 위치와 폭이다. 줄 왼쪽보다 앞선 열(사이드바 버튼에 가린 첫 열)은 0에서 시작하고,
 * 줄 오른쪽 밖에서 시작하는 열(오른쪽 버튼들에 가린 열)은 줄 끝에서 폭 0으로 남는다.
 *
 * Examples:
 *   columns lefts   stripLeft  stripWidth → [{ left, width }]
 *   [0, 500]        30         900        → [{ 0, 470 }, { 470, 430 }]
 *   [0]             30         900        → [{ 0, 900 }]
 */
function columnSpans(
    columns,
    stripLeft,
    stripWidth,
) {
    return columns.map((column, index) => {
        const left = Math.min(stripWidth, Math.max(0, column.left - stripLeft));
        const right = index + 1 < columns.length ? Math.min(stripWidth, Math.max(left, columns[index + 1].left - stripLeft)) : stripWidth;
        return { left, width: right - left };
    });
}

/**
 * 탭을 본문 칸에 실제로 놓인 차례로 늘어놓는다. 앞 칸의 탭이 먼저, 같은 칸 안에서는 칸의 탭 순서를 따른다.
 *
 * 위쪽 탭 줄은 끌어서 바꾼 칸 안 순서를 그대로 보여야 한다. `place(id)`는 `{ pane, index }`(칸 차례·칸 안 번호)이고,
 * 칸이 없는 탭(다른 창에 분리된 탭)은 null이다. 칸이 없는 탭은 맨 뒤에 원래 순서대로 남는다.
 *
 * Examples:
 *   ids          place                                   → result
 *   [a, b, c]    a→{0,1}, b→{0,0}, c→{0,2}               → [b, a, c]
 *   [a, b, c]    a→{1,0}, b→{0,1}, c→{0,0}               → [c, b, a]
 *   [w, a, b]    w→null, a→{0,1}, b→{0,0}                → [b, a, w]
 */
function orderTabsByPane(
    ids,
    place,
) {
    const rank = id => {
        const at = place(id);
        return at ? [at.pane, at.index] : [Infinity, 0];
    };
    return ids
        .map((id, order) => ({ id, order, rank: rank(id) }))
        .sort((first, second) => first.rank[0] - second.rank[0] || first.rank[1] - second.rank[1] || first.order - second.order)
        .map(({ id }) => id);
}

module.exports = { paneColumns, groupTabsByColumn, columnSpans, orderTabsByPane };
