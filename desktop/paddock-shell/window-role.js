// 앱 창의 역할. 첫 창(primary)만 배치·작업 목록을 저장하고 되살린다.
// 다른 창이 떠 있을 때 여는 새 창(secondary)은 빈 화면으로 시작하고, 저장 내용을 읽지도 덮어쓰지도 않는다.
// 창을 여는 앱 본체가 창 주소의 쿼리(`paddockWindow=secondary`)로 역할을 알린다. 주소는 새로고침해도 남는다.

/** 창 역할 쿼리의 이름과 값. */
const WINDOW_ROLE = Object.freeze({
    QUERY: 'paddockWindow',
    PRIMARY: 'primary',
    SECONDARY: 'secondary',
});

/**
 * 창 주소의 쿼리 문자열에서 창 역할을 읽는다.
 *
 * Examples
 * --------
 * | search                              | 결과          |
 * | ----------------------------------- | ------------- |
 * | `?port=3000`                        | `'primary'`   |
 * | `?port=3000&paddockWindow=secondary` | `'secondary'` |
 * | `''` · 다른 값                      | `'primary'`   |
 */
function windowRole(
    search,
) {
    const value = new URLSearchParams(search || '').get(WINDOW_ROLE.QUERY);
    return value === WINDOW_ROLE.SECONDARY ? WINDOW_ROLE.SECONDARY : WINDOW_ROLE.PRIMARY;
}

module.exports = { WINDOW_ROLE, windowRole };
