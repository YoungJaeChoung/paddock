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

module.exports = { withQueuedCopy };
