// 본문 칸 배치(Lumino 도크 배치 설정)를 묶음별로 나누고 합치는 순수 함수. 위젯은 `id`와 `isDisposed`만 읽는다.
// 배치는 { type: 'tab-area', widgets, currentIndex } 또는 { type: 'split-area', orientation, children, sizes }다.

/** 배치에 들어 있는 위젯을 왼쪽·위부터 순서대로 돌려준다. 빈 배치(null)는 빈 목록이다. */
function widgetsOf(
    area,
) {
    let widgets = [];
    if (area && area.type === 'tab-area') widgets = [...area.widgets];
    else if (area && area.type === 'split-area') widgets = area.children.flatMap(child => widgetsOf(child));
    return widgets;
}

/**
 * `keep`이 참인 위젯만 남긴 배치. 남은 위젯이 없는 칸은 사라지고, 자식이 하나만 남은 분할은 그 자식이 된다.
 * 칸에서 선택된 탭은 남은 위젯 중 원래 선택한 것을 유지하고, 사라졌으면 가장 가까운 탭으로 옮긴다.
 * 남은 위젯이 없으면 null이다.
 */
function prune(
    area,
    keep,
) {
    let result = null;
    if (area && area.type === 'tab-area') {
        const widgets = area.widgets.filter(widget => keep(widget));
        if (widgets.length) {
            const selected = area.widgets[area.currentIndex];
            const kept = widgets.indexOf(selected);
            result = { ...area, widgets, currentIndex: kept >= 0 ? kept : Math.min(area.currentIndex, widgets.length - 1) };
        }
    } else if (area && area.type === 'split-area') {
        const children = [];
        const sizes = [];
        area.children.forEach((child, index) => {
            const pruned = prune(child, keep);
            if (pruned) {
                children.push(pruned);
                sizes.push(area.sizes[index] ?? 1);
            }
        });
        if (children.length === 1) result = children[0];
        else if (children.length > 1) result = { ...area, children, sizes };
    }
    return result;
}

/** 배치에서 이미 닫힌 위젯을 뺀다. */
function withoutDisposed(
    area,
) {
    return prune(area, widget => !widget.isDisposed);
}

/** 배치의 첫 번째 칸 끝에 위젯을 탭으로 더하고 그 탭을 선택한다. 배치가 없으면 위젯 하나짜리 칸이다. */
function withWidget(
    area,
    widget,
) {
    let result;
    if (!area) {
        result = { type: 'tab-area', widgets: [widget], currentIndex: 0 };
    } else if (area.type === 'tab-area') {
        result = { ...area, widgets: [...area.widgets, widget], currentIndex: area.widgets.length };
    } else {
        result = { ...area, children: [withWidget(area.children[0], widget), ...area.children.slice(1)] };
    }
    return result;
}

/**
 * 여러 배치를 가로로 나란히 이은 한 배치. 각 배치의 칸 나눔·칸별 선택 탭은 그대로 남는다.
 * 나중에 묶음별로 `prune`하면 각 묶음이 원래 배치로 돌아온다. null은 건너뛰고, 하나만 남으면 그 배치, 없으면 null이다.
 */
function besides(
    areas,
) {
    const children = areas.filter(Boolean);
    let result = null;
    if (children.length === 1) result = children[0];
    else if (children.length > 1) result = { type: 'split-area', orientation: 'horizontal', children, sizes: children.map(() => 1 / children.length) };
    return result;
}

/** 배치에 위젯이 들어 있는지. */
function includes(
    area,
    widget,
) {
    return widgetsOf(area).includes(widget);
}

/**
 * 위젯이 속한 칸에서 그 위젯을 선택된 탭으로 만든다. 배치에 없는 위젯이면 그대로 돌려준다.
 */
function select(
    area,
    widget,
) {
    let result = area;
    if (area && area.type === 'tab-area') {
        const index = area.widgets.indexOf(widget);
        if (index >= 0) result = { ...area, currentIndex: index };
    } else if (area && area.type === 'split-area') {
        result = { ...area, children: area.children.map(child => select(child, widget)) };
    }
    return result;
}

module.exports = { widgetsOf, prune, withoutDisposed, withWidget, besides, includes, select };
