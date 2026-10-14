// 화면이 들어갈 영역을 정한다. Paddock 쉘은 사이드바와 본문만 두므로, Theia가 왼쪽·오른쪽·아래 패널로 보낸 화면은 붙을 곳이 없어 보이지 않는다.
// 그래서 그런 화면을 본문 탭으로 돌리고, 사이드바가 이미 품는 보기만 사이드바로 보낸다. 판정 규칙만 두어 Theia 없이 시험한다.

// Paddock 쉘에 그 패널이 없는 영역.
const PANEL_AREAS = ['left', 'right', 'bottom'];

/**
 * 화면을 넣을 영역을 돌려준다.
 *
 * 터미널은 어느 영역을 요청해도 본문이다(보조 창 제외). 그 밖의 화면은 패널이 없는 영역(left·right·bottom)을 요청하면 본문으로 돌린다.
 * 사이드바가 품는 보기(Source control·Extensions)는 요청한 영역을 그대로 두어 Theia가 넣은 뒤 사이드바가 가져가게 한다.
 * `redirectsPanels`가 거짓이면(시작 때 배치를 만드는 동안) 터미널 밖의 화면은 요청한 영역을 그대로 둔다.
 * 본문·보조 창 같은 나머지 영역과 영역 없음(undefined)은 그대로 돌려준다.
 *
 * Examples:
 *   (redirectsPanels true)
 *   area      isTerminal  isSidebarView   → 영역
 *   'bottom'  false       false           → 'main'
 *   'left'    false       false           → 'main'
 *   'left'    false       true            → 'left'
 *   'bottom'  true        false           → 'main'
 *   'secondaryWindow' true false          → 'secondaryWindow'
 *   'main'    false       false           → 'main'
 *   undefined false       false           → undefined
 *   (redirectsPanels false) 'bottom' false false → 'bottom'
 */
function widgetArea({
    area,
    isTerminal,
    isSidebarView,
    redirectsPanels,
}) {
    let result = area;
    if (isTerminal && area !== 'secondaryWindow') result = 'main';
    else if (redirectsPanels && !isSidebarView && PANEL_AREAS.includes(area)) result = 'main';
    return result;
}

module.exports = { widgetArea };
