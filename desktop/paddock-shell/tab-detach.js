/**
 * Pointer capture keeps receiving the release when a tab leaves the native window.
 * Delegate from the strip because periodic tab redraws replace its buttons.
 * A release inside the window cancels extraction; Escape and pointer cancellation
 * leave the original tab untouched. Moving widgets is owned by the workspace.
 */
function attachTabDrag(
    strip,
    moveToWindow,
    dock = {},
) {
    const view = strip.ownerDocument.defaultView;
    let drag;
    let suppressClick = false;
    const clear = () => {
        const previous = drag;
        drag = undefined;
        previous?.preview?.remove();
        dock.cancel?.();
        if (previous && strip.hasPointerCapture(previous.pointerId)) strip.releasePointerCapture(previous.pointerId);
        return previous;
    };
    strip.addEventListener('pointerdown', event => {
        const tab = event.target.closest('[role="tab"][data-widget-id]');
        if (event.button === 0 && tab && strip.contains(tab)) {
            clear();
            drag = { id: tab.dataset.widgetId, pointerId: event.pointerId, x: event.clientX, y: event.clientY, label: tab.textContent };
        }
    });
    view.addEventListener('pointermove', event => {
        if (drag && event.pointerId === drag.pointerId) {
            if (!drag.preview && Math.hypot(event.clientX - drag.x, event.clientY - drag.y) >= 8) {
                const preview = strip.ownerDocument.createElement('div');
                preview.className = 'paddock-tab-drag';
                // 이미 다른 창에 있는 탭은 새 창을 약속하지 않고 이 창으로 되돌리는 안내를 보인다.
                const hint = dock.hint?.(drag.id) ?? (dock.drop ? 'Drag to an edge to split, or outside for a new window' : 'Drag outside to open in a new window');
                preview.textContent = `${drag.label} · ${hint}`;
                strip.ownerDocument.body.append(preview);
                drag.preview = preview;
                strip.setPointerCapture(event.pointerId);
            }
            if (drag.preview) {
                drag.preview.style.left = `${event.clientX + 12}px`;
                drag.preview.style.top = `${event.clientY + 12}px`;
                dock.preview?.(drag.id, event.clientX, event.clientY);
            }
        }
    });
    view.addEventListener('pointerup', event => {
        if (drag && event.pointerId === drag.pointerId) {
            const previous = clear();
            suppressClick = Boolean(previous.preview);
            if (previous.preview && (event.clientX < 0 || event.clientY < 0 || event.clientX >= view.innerWidth || event.clientY >= view.innerHeight)) moveToWindow(previous.id);
            else if (previous.preview) dock.drop?.(previous.id, event.clientX, event.clientY);
            // The synthesized click follows pointerup synchronously, including
            // releases retargeted to the captured strip. Later clicks remain usable.
            view.setTimeout(() => { suppressClick = false; }, 0);
        }
    });
    strip.addEventListener('click', event => {
        if (suppressClick) {
            event.preventDefault();
            event.stopPropagation();
        }
    }, true);
    view.addEventListener('pointercancel', clear);
    // Chromium can lose native capture just before delivering an outside
    // release. Keep the intent until pointerup, Escape or pointercancel.
    view.addEventListener('keydown', event => {
        if (drag && event.key === 'Escape') {
            clear();
            event.preventDefault();
            event.stopPropagation();
        }
    }, true);
}

/**
 * Let Lumino own pane docking; only extract a native tab after an outside release.
 * Lumino 탭 줄은 pointerdown에서 기본 동작을 막아 호환 mouse 이벤트가 생기지 않으므로 pointer 이벤트로 듣는다.
 * 누름은 Lumino보다 먼저 칸의 캡처 단계에서, 뗌·취소는 창의 캡처 단계에서 받아 Lumino가 전파를 멈춰도 놓치지 않는다.
 */
function attachNativeTabDrag(
    panel,
    tabId,
    moveToWindow,
) {
    const view = panel.ownerDocument.defaultView;
    let drag;
    panel.addEventListener('pointerdown', event => {
        const id = event.button === 0 && !event.target.closest('.lm-TabBar-tabCloseIcon') ? tabId(event.target) : undefined;
        drag = id ? { id, pointerId: event.pointerId, x: event.clientX, y: event.clientY } : undefined;
    }, true);
    view.addEventListener('pointerup', event => {
        const previous = drag?.pointerId === event.pointerId ? drag : undefined;
        if (previous) drag = undefined;
        if (previous && event.button === 0 && Math.hypot(event.clientX - previous.x, event.clientY - previous.y) >= 8
            && (event.clientX < 0 || event.clientY < 0 || event.clientX >= view.innerWidth || event.clientY >= view.innerHeight)) {
            // Finish Lumino's drag cleanup before changing the widget's document.
            // 창 캡처 단계는 Lumino 끌기의 document 캡처 pointerup보다 먼저 실행되고, 끌기 정리(숨긴 탭 복원)는
            // 그 뒤 마이크로태스크에서 끝난다. 그래서 마이크로태스크가 아니라 다음 작업(setTimeout 0)으로 미룬다.
            view.setTimeout(() => moveToWindow(previous.id), 0);
        }
    }, true);
    view.addEventListener('pointercancel', event => {
        if (drag?.pointerId === event.pointerId) drag = undefined;
    }, true);
    // 공통 탭 줄의 Escape 처리와 달리 전파를 멈추지 않는다. Lumino 끌기도 같은 Escape로 취소돼야 한다.
    view.addEventListener('keydown', event => { if (event.key === 'Escape') drag = undefined; }, true);
}

/**
 * 보조 창 화면 좌표의 점을 메인 창 본문 좌표로 바꾼다. 점이 메인 창 밖이면 null이다.
 *
 * 메인 창의 제목 표시줄처럼 본문 위에 놓인 창 장식의 높이는 `outerHeight - innerHeight`로 본다.
 * 보조 창은 메인 창과 다른 문서라 포인터 좌표를 직접 비교할 수 없어 화면 좌표(screenX·screenY)로 맞춘다.
 *
 * @param point 보조 창 이벤트의 `{ screenX, screenY }`.
 * @param mainWindow 메인 창의 `{ screenX, screenY, innerWidth, innerHeight, outerHeight }`.
 * @returns `{ x, y }`(메인 창 본문 기준) 또는 null.
 *
 * | 메인 창 본문 위치·크기 | 화면 좌표 | 결과 |
 * |---|---|---|
 * | (100, 50) 800×600 | (150, 90) | { x: 50, y: 40 } |
 * | (100, 50) 800×600 | (50, 90) | null (왼쪽 밖) |
 * | (100, 50) 800×600 | (150, 700) | null (아래쪽 밖) |
 */
function mainWindowPoint(
    point,
    mainWindow,
) {
    const chromeHeight = Math.max(0, mainWindow.outerHeight - mainWindow.innerHeight);
    const x = point.screenX - mainWindow.screenX;
    const y = point.screenY - mainWindow.screenY - chromeHeight;
    const isInside = x >= 0 && y >= 0 && x < mainWindow.innerWidth && y < mainWindow.innerHeight;
    return isInside ? { x, y } : null;
}

/**
 * 보조 창의 탭을 끌어 메인 창에 놓으면 그 자리로 되돌린다.
 * 보조 창은 칸 끌기가 꺼져 있어 탭을 놓을 곳이 없다. 메인 창의 탭 줄 끌기와 같은 칸·탭 줄 대상을 쓰도록
 * 메인 창 좌표로 바꿔 `preview`·`drop`에 넘긴다. 보조 창 안에서 놓거나 두 창 밖에서 놓으면 아무 일도 하지 않는다.
 * 누름은 창 캡처 단계에서 받아 Lumino가 전파를 멈춰도 놓치지 않는다. Escape·pointercancel은 끌기를 취소한다.
 *
 * @param secondaryWindow 보조 창. 포인터를 누른 채 창 밖으로 나가도 놓음 이벤트는 이 창이 받는다.
 * @param mainWindow 메인 창.
 * @param tabId 눌린 요소에서 탭의 위젯 id를 찾는 함수. 탭이 아니면 undefined를 돌려준다.
 * @param dock `preview(id, x, y)` 놓을 자리 미리보기, `cancel()` 미리보기 제거, `drop(id, x, y)` 되돌리기. 좌표는 메인 창 본문 기준.
 */
function attachDetachedTabDrag(
    secondaryWindow,
    mainWindow,
    tabId,
    dock = {},
) {
    let drag;
    const clear = () => {
        const previous = drag;
        drag = undefined;
        dock.cancel?.();
        return previous;
    };
    const isOutside = event => event.clientX < 0 || event.clientY < 0
        || event.clientX >= secondaryWindow.innerWidth || event.clientY >= secondaryWindow.innerHeight;
    secondaryWindow.addEventListener('pointerdown', event => {
        const id = event.button === 0 && !event.target.closest('.lm-TabBar-tabCloseIcon') ? tabId(event.target) : undefined;
        drag = id ? { id, pointerId: event.pointerId, x: event.clientX, y: event.clientY } : undefined;
    }, true);
    secondaryWindow.addEventListener('pointermove', event => {
        if (drag && event.pointerId === drag.pointerId && Math.hypot(event.clientX - drag.x, event.clientY - drag.y) >= 8) {
            const point = isOutside(event) ? mainWindowPoint(event, mainWindow) : null;
            if (point) dock.preview?.(drag.id, point.x, point.y);
            else dock.cancel?.();
        }
    }, true);
    secondaryWindow.addEventListener('pointerup', event => {
        const previous = drag?.pointerId === event.pointerId ? clear() : undefined;
        if (previous && event.button === 0 && Math.hypot(event.clientX - previous.x, event.clientY - previous.y) >= 8 && isOutside(event)) {
            const point = mainWindowPoint(event, mainWindow);
            if (point) dock.drop?.(previous.id, point.x, point.y);
        }
    }, true);
    secondaryWindow.addEventListener('pointercancel', event => {
        if (drag?.pointerId === event.pointerId) clear();
    }, true);
    secondaryWindow.addEventListener('keydown', event => {
        if (drag && event.key === 'Escape') clear();
    }, true);
}

/**
 * 보조 창의 위젯 목록에서 이미 그 창을 떠난 위젯을 지운다.
 * Theia 보조 창의 위젯 목록은 위젯이 닫힐 때만 줄어든다. 위젯을 메인 창으로 되돌려도 목록에 남으면,
 * 보조 창의 beforeunload 검사가 메인으로 옮긴 저장 안 한 에디터를 보고 창 닫기를 영구히 막는다.
 * 지운 뒤에도 창 안에 남은 위젯은 그대로 두어 그 위젯의 닫기 확인은 유지한다.
 *
 * @param rootWidget 보조 창의 최상위 위젯. `widgets` 배열을 직접 줄인다. 없으면 아무것도 하지 않는다.
 * @returns 지운 위젯 수.
 */
function forgetRestoredWidgets(
    rootWidget,
) {
    const widgets = rootWidget?.widgets ?? [];
    const isInside = widget => {
        let parent = widget.parent;
        while (parent && parent !== rootWidget) parent = parent.parent;
        return Boolean(parent);
    };
    const kept = widgets.filter(widget => !widget.isDisposed && isInside(widget));
    const n_removed = widgets.length - kept.length;
    widgets.splice(0, widgets.length, ...kept);
    return n_removed;
}

module.exports = { attachTabDrag, attachNativeTabDrag, attachDetachedTabDrag, mainWindowPoint, forgetRestoredWidgets };
