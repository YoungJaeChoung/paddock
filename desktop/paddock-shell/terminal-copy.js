const { createCopyQueue } = require('./copy-queue');

/**
 * 터미널 선택 복사 처리기의 클립보드 쓰기를 차례대로 묶는다.
 *
 * Theia의 처리기는 선택이 바뀔 때마다 권한 확인 뒤 클립보드 쓰기를 비동기로 시작하고 순서를 보장하지 않는다.
 * 마우스로 끌어 고르면 선택이 수십 번 바뀌어, 먼저 시작한 짧은 선택의 쓰기가 나중에 끝나면 끈 글자와 다른 글자가 클립보드에 남는다.
 * 처리기의 `copy`를 쓰기 대기열(copy-queue)로 감싸 마지막 선택만 클립보드에 남긴다.
 *
 * 처리기는 모든 터미널이 공유하는 하나라, 만들어지는 순간(DI 활성화) 한 번 감싼다. 같은 처리기를 다시 넘겨도 두 번 감싸지 않는다.
 * 감싼 처리기를 그대로 돌려준다.
 */
function withQueuedCopy(
    handler,
) {
    if (handler && !handler.paddockCopyQueue) {
        const copy = handler.copy.bind(handler);
        handler.paddockCopyQueue = createCopyQueue(copy);
        handler.copy = text => handler.paddockCopyQueue(text);
    }
    return handler;
}

/**
 * 터미널 프로그램이 보낸 클립보드 쓰기 요청(OSC 52: `ESC ] 52 ; 대상 ; base64 글자 BEL`)의 본문에서 복사할 글자를 꺼낸다.
 *
 * Claude Code는 화면 안에서 끌어 고른 글자를 이 요청으로 보낸다. 마우스를 직접 받는 프로그램이라 터미널 자체의 선택 복사는 일어나지 않고,
 * 함께 시도하는 운영체제 복사 명령(WSL의 powershell.exe)이 실패하면 클립보드에 예전 글자가 남는다.
 * `data`는 `52;` 뒤의 본문이다(`대상;base64`). 대상(c·p·s 등)은 하나의 클립보드로 합친다.
 * 클립보드 읽기 요청(`?`)·빈 본문·형식이 틀린 본문·UTF-8이 아닌 내용이면 null이다. 읽기 요청에는 응답하지 않는다 — 프로그램이 클립보드를 엿보지 못하게 한다.
 *
 * Examples:
 *   data                     → 결과
 *   'c;aGVsbG8='             → 'hello'
 *   ';7ZWc6riA'              → '한글'
 *   'c;?'                    → null
 *   'c'                      → null
 *   'c;'                     → null
 *   'c;!!!'                  → null
 */
function osc52ClipboardText(
    data,
) {
    let text = null;
    const separator = data.indexOf(';');
    const payload = separator >= 0 ? data.slice(separator + 1) : '';
    if (/^[A-Za-z0-9+/]+={0,2}$/.test(payload)) {
        try {
            // 화면 쪽 번들에는 Node의 Buffer가 없어 브라우저 함수(atob)로 바이트를 되살린다.
            text = new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(atob(payload), char => char.charCodeAt(0)));
        } catch {
            text = null;
        }
    }
    return text;
}

module.exports = { withQueuedCopy, osc52ClipboardText };
