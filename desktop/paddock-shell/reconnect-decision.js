// 재접속 실패 때 새로 고칠지 알릴지 정하는 순수 로직. Theia에 의존하지 않아 단위 테스트로 확인한다.

/**
 * 여러 위젯 목록(메인 영역·편집기 관리자·보조 창)을 합쳐 저장하지 않은 위젯만 돌려준다. 같은 위젯은 한 번만 센다.
 *
 * Theia의 저장 확인이 보는 위젯은 영역에 따라 목록이 갈리므로(보조 창으로 뗀 탭, 편집기 관리자만 아는 편집기),
 * 한 목록만 보면 미저장 편집기를 놓친다.
 *
 * Examples
 * --------
 * | lists                      | isDirty         | 결과     |
 * |----------------------------|-----------------|----------|
 * | `[[a, b], [b, c]]`         | `b`, `c`만 true | `[b, c]` |
 * | `[[], []]`                 | —               | `[]`     |
 */
function dirtyWidgets(
    lists,
    isDirty,
) {
    return [...new Set(lists.flat())].filter(widget => isDirty(widget));
}

/** 미저장 위젯이 없으면 `'reload'`(바로 새로 고침), 있으면 `'notify'`(알리고 사용자가 정하게 함). */
function reconnectAction(
    n_dirty,
) {
    return n_dirty > 0 ? 'notify' : 'reload';
}

/** 연결을 잃고 저장하지 않은 편집기가 있을 때 알림·상태 줄에 쓰는 문장. */
function lostConnectionMessage(
    n_dirty,
) {
    return `Lost the connection to Paddock's background service. ${n_dirty === 1 ? '1 editor has' : `${n_dirty} editors have`} unsaved changes that could not be saved. Copy what you need, then reload.`;
}

module.exports = { dirtyWidgets, reconnectAction, lostConnectionMessage };
