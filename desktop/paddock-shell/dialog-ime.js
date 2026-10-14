// 한글 등 조합 입력 중 대화상자 Enter 처리. 화면 모듈(frontend-module.js)과 분리해 Theia 없이 시험한다.

// 조합이 끝난 뒤 진짜 Enter 키 입력이 이어서 오는지 기다리는 시간(ms). 오면 그 입력이 확인을 맡고, 안 오면 대신 확인을 보낸다.
const ENTER_AFTER_COMPOSITION_MS = 30;

/**
 * 글자 조합 중에 누른 Enter가 대화상자를 먼저 닫지 않게 붙잡았다가, 조합이 끝난 뒤 확인을 보낸다.
 *
 * Theia 대화상자는 Enter를 키 자리(`code`)로만 판정해, 한글 마지막 글자가 아직 조합 중일 때 누른 Enter에도 바로 확인하고 닫는다.
 * 그러면 조합 중이던 글자는 대화상자가 닫힌 뒤 확정되어, 초점을 넘겨받은 터미널에 입력된다(탭 이름을 바꾸면 터미널에 마지막 글자가 들어가던 원인).
 * 대화상자 안(`.dialogOverlay`)에서 조합 중인 Enter(`isComposing` 또는 keyCode 229)만 창 단계(capture)에서 막고,
 * 조합이 끝나면 입력란에 Enter를 다시 보내 마지막 글자까지 담긴 값으로 확인되게 한다.
 * 조합이 끝난 직후 운영체제가 진짜 Enter를 따로 보내면(일부 입력기) 그 입력이 확인을 맡고 다시 보내지 않는다 — 두 번 확인되지 않게.
 * 대화상자 밖의 입력(터미널·편집기)은 건드리지 않는다. 해제 함수를 돌려준다.
 */
function holdEnterWhileComposing(
    target,
    schedule = setTimeout,
) {
    let pending = null;
    const isEnter = event => event.code === 'Enter' || event.code === 'NumpadEnter' || event.key === 'Enter';
    const inDialog = event => Boolean(event.target?.closest?.('.dialogOverlay'));
    const onKeyDown = event => {
        if (inDialog(event) && isEnter(event)) {
            if (event.isComposing || event.keyCode === 229) {
                event.stopImmediatePropagation();
                event.preventDefault();
                pending = { element: event.target, isWaiting: true };
            } else if (pending) {
                // 조합이 끝난 뒤 진짜 Enter가 왔다. 이 입력이 확인을 맡는다.
                pending = null;
            }
        }
    };
    const onCompositionEnd = event => {
        const held = pending;
        if (held && held.element === event.target) {
            schedule(() => {
                if (pending === held) {
                    pending = null;
                    if (held.element.isConnected) {
                        held.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true }));
                    }
                }
            }, ENTER_AFTER_COMPOSITION_MS);
        }
    };
    target.addEventListener('keydown', onKeyDown, true);
    target.addEventListener('compositionend', onCompositionEnd, true);
    return () => {
        target.removeEventListener('keydown', onKeyDown, true);
        target.removeEventListener('compositionend', onCompositionEnd, true);
    };
}

module.exports = { holdEnterWhileComposing, ENTER_AFTER_COMPOSITION_MS };
