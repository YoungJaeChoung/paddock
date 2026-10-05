// 내부 터미널·파일 줄의 스크롤 위치와 넘침 표시를 맞춘다.
// 탭 생성·선택·이벤트 등록은 호출자가 맡고, 이 모듈은 전달받은 탭 줄의 DOM과 키보드 초점만 다룬다.

/**
 * 탭 내용을 갱신해도 같은 버튼의 키보드 초점과 사용자가 옮긴 스크롤을 유지한다.
 * 선택이 바뀌었는지를 돌려주므로 호출자는 새로 선택한 탭만 화면 안으로 옮길 수 있다.
 */
function replaceTabs(
    strip,
    contents,
) {
    const focused = strip.contains(document.activeElement) ? document.activeElement : null;
    const focusedId = focused?.closest('.tab')?.querySelector('[role="tab"]')?.dataset.widgetId;
    const focusedControl = focused?.classList.contains('tab-close') ? '.tab-close' : '[role="tab"]';
    const selectedId = strip.querySelector('[aria-selected="true"]')?.dataset.widgetId;
    const scrollLeft = strip.scrollLeft;
    strip.replaceChildren(contents);
    const tabs = [...strip.querySelectorAll('[role="tab"]')];
    const selected = tabs.find(tab => tab.getAttribute('aria-selected') === 'true');
    const restored = tabs.find(tab => tab.dataset.widgetId === focusedId);
    const tabStop = restored || selected || tabs[0];
    setTabStop(strip, tabStop);
    strip.scrollLeft = scrollLeft;
    if (restored) restored.closest('.tab').querySelector(focusedControl)?.focus({ preventScroll: true });
    return selectedId !== selected?.dataset.widgetId;
}

/** 탭 줄에서는 선택한 탭 하나로 진입하고, 그 탭의 닫기 버튼으로 이어서 이동한다. */
function setTabStop(
    strip,
    selected,
) {
    for (const tab of strip.querySelectorAll('[role="tab"]')) {
        tab.tabIndex = tab === selected ? 0 : -1;
        const close = tab.closest('.tab').querySelector('.tab-close');
        if (close) close.tabIndex = tab.tabIndex;
    }
}

/** 방향키와 Home·End로 탭을 고른다. Enter·Space의 기본 버튼 동작이 실제 화면을 선택한다. */
function moveTabFocus(
    strip,
    event,
) {
    const current = event.target.closest('[role="tab"]');
    const keys = ['ArrowLeft', 'ArrowRight', 'Home', 'End'];
    if (current && strip.contains(current) && keys.includes(event.key) && !event.altKey && !event.ctrlKey && !event.metaKey) {
        const tabs = [...strip.querySelectorAll('[role="tab"]')];
        const n_tabs = tabs.length;
        const index = tabs.indexOf(current);
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? n_tabs - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + n_tabs) % n_tabs;
        const target = tabs[next];
        event.preventDefault();
        event.stopPropagation();
        setTabStop(strip, target);
        // 잘린 탭은 실수로 닫지 못하게 숨겨져 있다. 키보드로 고를 때는 먼저 보이는 범위로 옮긴다.
        // 이름 버튼만 옮기면 탭의 왼쪽 여백이 잘려 다시 숨겨지므로 탭 전체를 기준으로 삼는다.
        const targetTab = target.closest('.tab');
        targetTab.classList.remove('is-clipped');
        targetTab.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        // 탭 하나보다 줄이 좁을 때도 이름이 있는 왼쪽부터 보이게 해 키보드 초점이 숨겨지지 않게 한다.
        if (targetTab.offsetWidth > strip.clientWidth) strip.scrollLeft = targetTab.offsetLeft;
        updateTabOverflow(strip);
        target.focus({ preventScroll: true });
    }
}

/**
 * 스크롤이 탭 중간에서 멈춰 맨 왼쪽 탭이 반쯤 잘리면(닫기 ×만 보이면) 탭 경계로 옮긴다.
 * 잘린 탭을 온전히 보여도 활성 탭이 보이면 그 탭 시작으로, 아니면 다음 탭 시작으로 간다.
 * CSS 스크롤 맞춤(scroll-snap)은 활성 탭 따라가기와 충돌해 활성 탭을 밖으로 밀어내서 쓰지 않는다.
 */
function alignTabsToEdge(
    strip,
) {
    const tabs = [...strip.querySelectorAll('.tab')];
    const cut = tabs.find(tab => tab.offsetLeft < strip.scrollLeft && tab.offsetLeft + tab.offsetWidth > strip.scrollLeft);
    if (cut) {
        const active = strip.querySelector('.tab.is-active');
        const activeRight = active ? active.offsetLeft + active.offsetWidth : 0;
        const keepsActive = !active || activeRight <= cut.offsetLeft + strip.clientWidth;
        strip.scrollLeft = keepsActive ? cut.offsetLeft : cut.offsetLeft + cut.offsetWidth;
    }
}

/**
 * 넘침 표시를 현재 스크롤 위치에 맞춘다: 가장자리 흐림, 넘친 쪽 넘김 버튼(‹ ›)과 그 버튼의 점, 왼쪽에서 잘린 탭 숨김.
 * 사용자가 직접 스크롤할 때도 부른다(이때는 경계로 옮기지 않는다 — 스크롤 중에 위치를 바꾸면 튄다).
 */
function updateTabOverflow(
    strip,
) {
    const isOverflowing = strip.scrollWidth > strip.clientWidth + 1;
    const hasLater = isOverflowing && strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1;
    const hasEarlier = isOverflowing && strip.scrollLeft > 0;
    strip.classList.toggle('is-overflowing', hasLater);
    strip.classList.toggle('is-scrolled', hasEarlier);
    // 목록 끝이라 더 스크롤할 수 없으면 왼쪽에 잘린 탭이 남는다. 이름 없이 닫기 ×만 보이면 잘못 눌러 터미널을 닫을 수 있어 숨긴다.
    for (const tab of strip.querySelectorAll('.tab')) {
        tab.classList.toggle('is-clipped', tab.offsetLeft < strip.scrollLeft - 1 && tab.offsetLeft + tab.offsetWidth > strip.scrollLeft);
    }
    // 탭 줄은 넘친 쪽에 넘김 버튼(‹ ›)을 보인다. 가장자리 흐림만으로는 잘린 탭이 렌더링 오류처럼 보인다.
    // 가려진 쪽에 에이전트가 끝난 탭이 있으면 그쪽 넘김 버튼에 점을 찍는다. 다른 칸에 보이는 상태만으로는 완료 점을 쓰지 않는다.
    const hiddenMarks = { '-1': false, '1': false };
    for (const tab of strip.querySelectorAll('.tab.is-done')) {
        if (tab.offsetLeft + tab.offsetWidth <= strip.scrollLeft) hiddenMarks['-1'] = true;
        if (tab.offsetLeft >= strip.scrollLeft + strip.clientWidth) hiddenMarks['1'] = true;
    }
    for (const scroller of strip.parentElement.querySelectorAll(':scope > .tabs-scroll')) {
        scroller.hidden = scroller.dataset.direction === '1' ? !hasLater : !hasEarlier;
        scroller.classList.toggle('has-mark', hiddenMarks[scroller.dataset.direction]);
    }
}

module.exports = { alignTabsToEdge, updateTabOverflow, replaceTabs, moveTabFocus };
