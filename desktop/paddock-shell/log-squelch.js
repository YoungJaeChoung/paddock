// 백엔드 로그(stdout·stderr → backend.log)에서 같은 줄의 반복과 세션 전체 크기에 상한을 둔다.
// 화면과 연결이 끊겼다 다시 맺힐 때마다 같은 오류가 수만 줄 쏟아져 14분 만에 342 MB가 된 적이 있다.
// 백엔드의 stdout은 앱이 연 파일을 그대로 물려받으므로(치명 오류의 마지막 출력이 빠지면 안 된다) 파일을 따로 열지 않고,
// 쓰기 함수를 감싸 반복만 걸러 낸다.

/** 반복 억제 규칙. 10초 안에 같은 틀의 출력이 6번째부터는 버리고, 창이 끝날 때 몇 번 버렸는지 한 줄로 남긴다. */
class SQUELCH {
    static WINDOW_MS = 10 * 1000;
    static MAX_REPEATS = 5;
    /** 한 세션(백엔드 프로세스 하나)이 쓸 수 있는 총량. 넘으면 안내 한 줄 뒤 재시작까지 쓰지 않는다. */
    static MAX_SESSION_BYTES = 100 * 1024 * 1024;
    /** 요약 줄에 붙이는 마지막 원문의 최대 글자 수. */
    static PREVIEW_CHARS = 200;
    /** 동시에 추적하는 서로 다른 틀의 최대 수. 넘으면 오래된 것부터 요약하고 버린다(메모리 상한). */
    static MAX_KEYS = 500;
}

/** 새 상태를 만든다. 상태는 `squelch`·`flushExpired`가 채우고 비운다. */
function createSquelchState() {
    return { windows: new Map(), n_bytes: 0, isCapped: false };
}

/**
 * 출력 한 덩이를 같은 틀끼리 묶는 열쇠로 바꾼다. 첫 줄의 숫자(시각·포트·횟수)와 UUID를 지워 반복을 같은 틀로 본다.
 *
 * 여러 줄 덩이(오류 스택)는 첫 줄만 보고 덩이 전체를 한 단위로 다룬다. 스택의 중간 줄만 걸러 내면 읽을 수 없게 되기 때문이다.
 *
 * Examples
 * --------
 * | text                                                     | key                                              |
 * |----------------------------------------------------------|--------------------------------------------------|
 * | `2026-10-14T05:24:01.123Z root ERROR No reply handler 17` | `N-N-NTN:N:N.NZ root ERROR No reply handler N`   |
 * | `[4b0ba821-b495-4f57-b8ea-5411ad21f9b7] Starting`         | `[ID] Starting`                                  |
 */
function squelchKey(
    text,
) {
    const firstLine = text.split('\n', 1)[0];
    return firstLine
        .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, 'ID')
        .replace(/\d+/g, 'N');
}

/** 한 창의 요약 줄. 버린 횟수와 마지막으로 버린 원문 첫 줄을 담아, 요약만 봐도 어떤 오류가 반복됐는지 알 수 있다. */
function summaryLine(
    window,
    now,
) {
    const preview = window.lastText.split('\n', 1)[0].slice(0, SQUELCH.PREVIEW_CHARS);
    return `${new Date(now).toISOString()} paddock:Squelch WARN repeated ${window.n_suppressed} more times in ${SQUELCH.WINDOW_MS / 1000} s: ${preview}\n`;
}

/**
 * 끝난 창(시작한 지 `WINDOW_MS` 지난 것)을 닫고, 버린 줄이 있었던 창의 요약을 모아 돌려준다. 요약할 것이 없으면 빈 문자열이다.
 * 새 출력이 없어도 요약이 남도록 쓰는 쪽이 주기적으로 부른다.
 */
function flushExpired(
    state,
    now,
) {
    let summaries = '';
    for (const [key, window] of state.windows) {
        if (now - window.start >= SQUELCH.WINDOW_MS) {
            if (window.n_suppressed > 0) summaries += summaryLine(window, now);
            state.windows.delete(key);
        }
    }
    return summaries;
}

/**
 * 출력 한 덩이를 쓸지 정한다. 쓸 덩이 앞에 먼저 써야 할 요약·안내(`prefix`)와 덩이를 그대로 쓸지(`isPassed`)를 돌려준다.
 *
 * 같은 틀(숫자를 지운 첫 줄)이 창 안에서 `MAX_REPEATS`번까지는 통과하고 그 뒤는 버린다. 오류든 정보든 같은 규칙이며,
 * 창이 끝날 때 요약 줄이 마지막 원문과 함께 남는다. 세션 누적 통과량이 `MAX_SESSION_BYTES`를 넘으면 안내를 한 번 쓰고 이후는 모두 버린다.
 *
 * Examples
 * --------
 * | 입력                                           | isPassed        | prefix                        |
 * |------------------------------------------------|-----------------|-------------------------------|
 * | 같은 틀 1~5번째                                | true            | 빈 문자열                     |
 * | 같은 틀 6번째 이후(10초 안)                    | false           | 빈 문자열                     |
 * | 같은 틀, 10초 뒤 첫 줄                         | true            | "repeated N more times" 요약  |
 * | 누적 100 MB 초과 직후                          | false           | 안내 한 줄(한 번만)           |
 */
function squelch(
    state,
    text,
    now,
) {
    let prefix = flushExpired(state, now);
    let isPassed = false;
    if (state.isCapped) {
        isPassed = false;
    } else if (state.n_bytes + Buffer.byteLength(text) > SQUELCH.MAX_SESSION_BYTES) {
        state.isCapped = true;
        prefix += `${new Date(now).toISOString()} paddock:Squelch ERROR backend.log exceeded ${SQUELCH.MAX_SESSION_BYTES / 1024 / 1024} MB; suppressing further output until restart\n`;
    } else {
        const key = squelchKey(text);
        let window = state.windows.get(key);
        if (window === undefined) {
            if (state.windows.size >= SQUELCH.MAX_KEYS) {
                const [oldestKey, oldest] = state.windows.entries().next().value;
                if (oldest.n_suppressed > 0) prefix += summaryLine(oldest, now);
                state.windows.delete(oldestKey);
            }
            window = { start: now, n_seen: 0, n_suppressed: 0, lastText: text };
            state.windows.set(key, window);
        }
        window.n_seen += 1;
        if (window.n_seen <= SQUELCH.MAX_REPEATS) {
            isPassed = true;
            state.n_bytes += Buffer.byteLength(text);
        } else {
            window.n_suppressed += 1;
            window.lastText = text;
        }
        state.n_bytes += Buffer.byteLength(prefix);
    }
    return { prefix, isPassed };
}

/** 요약이 새 출력 없이도 나오도록 끝난 창을 확인하는 주기(ms). */
const FLUSH_INTERVAL_MS = 5 * 1000;

/**
 * `process.stdout`·`process.stderr`의 쓰기를 감싸 반복 출력을 걸러 낸다. 두 스트림은 같은 파일로 가므로 한 상태를 함께 쓴다.
 *
 * 걸러 낸 덩이도 쓰기 호출은 성공으로 응답한다(콜백 호출). 호출한 쪽(Theia 로거)이 막히거나 오류를 받지 않게 하기 위해서다.
 */
function installLogSquelch(
    streams,
) {
    const state = createSquelchState();
    const writers = streams.map(stream => stream.write.bind(stream));
    streams.forEach((stream, index) => {
        stream.write = (chunk, encoding, callback) => {
            const done = typeof encoding === 'function' ? encoding : callback;
            const text = typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8');
            const decision = squelch(state, text, Date.now());
            if (decision.prefix) writers[index](decision.prefix);
            let result = true;
            if (decision.isPassed) {
                result = writers[index](chunk, encoding, callback);
            } else if (typeof done === 'function') {
                done();
            }
            return result;
        };
    });
    const timer = setInterval(() => {
        const summaries = flushExpired(state, Date.now());
        if (summaries) writers[0](summaries);
    }, FLUSH_INTERVAL_MS);
    timer.unref();
    return state;
}

module.exports = { SQUELCH, createSquelchState, squelchKey, flushExpired, squelch, installLogSquelch };
