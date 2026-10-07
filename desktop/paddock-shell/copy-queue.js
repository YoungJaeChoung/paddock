// 선택 복사의 클립보드 쓰기를 차례대로 묶는다.
// 마우스로 끌어 고르는 동안 선택이 바뀔 때마다 복사가 불리는데, 쓰기가 비동기라 먼저 시작한 짧은 선택의 쓰기가
// 나중에 끝나면 마지막 선택을 덮어 "끈 글자와 다른 글자"가 붙는다. 쓰기를 한 줄로 세우고 그 사이 더 새 선택이 오면 건너뛴다.

/**
 * 마지막으로 요청한 글자만 클립보드에 남기는 쓰기 대기열을 만든다.
 *
 * `write(text)`는 실제 클립보드 쓰기(Promise)다. 돌려주는 함수는 글자를 받아 대기열 끝에 세우고,
 * 자기 차례에 그 글자가 여전히 가장 새 요청이면 쓰고 아니면 건너뛴다. 쓰기 실패는 대기열을 막지 않는다.
 *
 * Examples:
 *   enqueue('ab'); enqueue('abc'); enqueue('abcd')   → write는 'ab' 뒤 'abcd'만 받는다('abc'는 그 차례에 이미 옛 요청)
 *   enqueue('x') (write가 거부)                       → 다음 enqueue('y')는 정상적으로 'y'를 쓴다
 */
function createCopyQueue(
    write,
) {
    let latest;
    let queue = Promise.resolve();
    return text => {
        latest = text;
        queue = queue.then(async () => {
            if (text === latest) {
                try {
                    await write(text);
                } catch {
                    // 한 번의 쓰기 실패는 다음 선택의 복사를 막지 않는다.
                }
            }
        });
        return queue;
    };
}

module.exports = { createCopyQueue };
