// 같은 문구의 알림 요청을 하나로 합치는 순수 로직. Theia에 의존하지 않아 단위 테스트로 확인한다.

/**
 * 같은 문구가 열려 있는 동안 다시 요청되면 새로 열지 않고 열려 있는 쪽의 결과를 돌려준다.
 *
 * 5초마다 같은 git 오류가 반복돼도 알림이 쌓이지 않게 하려는 것이다. 닫히면(열기 결과가 확정되면)
 * 목록에서 빠지므로, 닫은 뒤 같은 문구가 다시 오면 새로 열린다.
 * `pending`은 문구 → 열려 있는 결과의 Map이며 이 함수가 채우고 비운다.
 *
 * Examples
 * --------
 * | 호출 순서                                  | open 호출 수 |
 * |--------------------------------------------|--------------|
 * | 'a' 열기, 닫히기 전에 'a' 열기             | 1            |
 * | 'a' 열기, 'b' 열기                         | 2            |
 * | 'a' 열기, 닫힘, 다시 'a' 열기              | 2            |
 */
function coalesceMessage(
    pending,
    text,
    open,
) {
    let opened = pending.get(text);
    if (opened === undefined) {
        opened = Promise.resolve(open()).finally(() => pending.delete(text));
        pending.set(text, opened);
    }
    return opened;
}

module.exports = { coalesceMessage };
