// 백엔드 로그(stdout·stderr → backend.log)에서 같은 줄의 반복과 세션 전체 크기에 상한을 둔다.
// 화면과 연결이 끊겼다 다시 맺힐 때마다 같은 오류가 수만 줄 쏟아져 14분 만에 342 MB가 된 적이 있다.
// 백엔드의 stdout은 앱이 연 파일을 그대로 물려받으므로(치명 오류의 마지막 출력이 빠지면 안 된다) 파일을 따로 열지 않고,
// 쓰기 함수를 감싸 반복만 걸러 낸다.

/**
 * 반복 억제 규칙. 오류 수준이 아닌 출력은 10초 안에 같은 틀이 6번째부터 버리고, 창이 끝날 때 몇 번 버렸는지 한 줄로 남긴다.
 * 오류 수준 출력(`ERROR`·`FATAL`·처리 안 된 예외·확장 호스트 사망·크래시)은 반복·세션 총량 상한의 대상이 아니다 —
 * 원인을 알려 주는 마지막 출력이므로 버리지 않고, 폭주만 막으려고 별도의 작은 상한을 둔다.
 */
class SQUELCH {
    static WINDOW_MS = 10 * 1000;
    static MAX_REPEATS = 5;
    /** 오류가 아닌 출력이 한 세션(백엔드 프로세스 하나)에 쓸 수 있는 총량. 넘으면 안내 한 줄 뒤 재시작까지 오류 수준만 쓴다. */
    static MAX_SESSION_BYTES = 100 * 1024 * 1024;
    /** 오류 수준 출력이 한 세션에 쓸 수 있는 총량. 넘으면 안내 한 줄 뒤 재시작까지 모두 버린다. */
    static MAX_ERROR_BYTES = 10 * 1024 * 1024;
    /** 요약 줄에 붙이는 원문 하나의 최대 글자 수. */
    static PREVIEW_CHARS = 160;
    /** 동시에 추적하는 서로 다른 틀의 최대 수. 넘으면 오래된 것부터 요약하고 버린다(메모리 상한). */
    static MAX_KEYS = 500;
}

/** 오류 수준으로 보는 줄. 수준 표지(`ERROR`·`FATAL`), 처리 안 된 예외·거부, 크래시, 확장 호스트가 끝났다는 줄이다. */
const ERROR_LEVEL_PATTERN = /(?:^|\s)(?:ERROR|FATAL)\s|uncaught|unhandled|crash|(?:plugin[- ]?host|hosted-plugin)[^\n]*(?:exit|terminat|died|killed)/i;

/** 새 상태를 만든다. 상태는 `squelch`·`flushExpired`가 채우고 비운다. */
function createSquelchState() {
    return { windows: new Map(), n_bytes: 0, n_error_bytes: 0, isCapped: false, isErrorCapped: false };
}

/**
 * 출력 덩이를 판정 단위로 나눈다. 공백으로 시작하는 줄(오류 스택의 `    at …`)은 앞 줄에 붙는다.
 *
 * 한 번의 쓰기에 여러 줄이 올 수 있다(확장 호스트의 출력은 여러 줄이 한 덩이로 온다). 줄마다 따로 판정해야
 * 덩이 첫 줄만 보고 뒤의 다른 오류까지 한꺼번에 버리지 않는다.
 *
 * Examples
 * --------
 * | text                               | 단위                                  |
 * |------------------------------------|---------------------------------------|
 * | `a\nb\n`                           | `['a\n', 'b\n']`                      |
 * | `ERROR x\n    at f (1:2)\nnext\n`  | `['ERROR x\n    at f (1:2)\n', 'next\n']` |
 */
function splitUnits(
    text,
) {
    const units = [];
    for (const line of text.split(/(?<=\n)/)) {
        if (units.length > 0 && /^[ \t]/.test(line)) units[units.length - 1] += line;
        else units.push(line);
    }
    return units;
}

/**
 * 출력 한 단위를 같은 틀끼리 묶는 열쇠로 바꾼다. 첫 줄의 숫자(시각·포트·횟수)와 UUID를 지워 반복을 같은 틀로 본다.
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

/** 한 줄 미리보기. 숫자만 다른 틀이라 터미널 번호·종료 코드 같은 값이 합쳐졌더라도 첫·마지막 원문으로 값이 드러난다. */
function preview(
    text,
) {
    return text.split('\n', 1)[0].slice(0, SQUELCH.PREVIEW_CHARS);
}

/** 한 창의 요약 줄. 버린 횟수와 처음·마지막으로 버린 원문 첫 줄을 담아 어떤 오류가 반복됐는지, 값이 달랐는지 알 수 있다. */
function summaryLine(
    window,
    now,
) {
    const first = preview(window.firstSuppressed);
    const last = preview(window.lastSuppressed);
    const samples = first === last ? `: ${first}` : `; first: ${first} | last: ${last}`;
    return `${new Date(now).toISOString()} paddock:Squelch WARN repeated ${window.n_suppressed} more times in ${SQUELCH.WINDOW_MS / 1000} s${samples}\n`;
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

/** 한 단위를 쓸지 정한다. 쓸 수 없으면 `{ isPassed: false }`, 새로 안내할 줄이 생기면 `notice`에 담는다. */
function judgeUnit(
    state,
    unit,
    now,
) {
    let isPassed = false;
    let notice = '';
    const size = Buffer.byteLength(unit);
    const stamp = new Date(now).toISOString();
    if (ERROR_LEVEL_PATTERN.test(unit)) {
        if (state.isErrorCapped) {
            isPassed = false;
        } else if (state.n_error_bytes + size > SQUELCH.MAX_ERROR_BYTES) {
            state.isErrorCapped = true;
            notice = `${stamp} paddock:Squelch ERROR log limit reached: error output exceeded ${SQUELCH.MAX_ERROR_BYTES / 1024 / 1024} MB; suppressing further output until restart\n`;
        } else {
            state.n_error_bytes += size;
            isPassed = true;
        }
    } else if (state.isCapped) {
        isPassed = false;
    } else if (state.n_bytes + size > SQUELCH.MAX_SESSION_BYTES) {
        state.isCapped = true;
        notice = `${stamp} paddock:Squelch ERROR log limit reached: backend.log exceeded ${SQUELCH.MAX_SESSION_BYTES / 1024 / 1024} MB; suppressing further non-error output until restart\n`;
    } else {
        const key = squelchKey(unit);
        let window = state.windows.get(key);
        if (window === undefined) {
            if (state.windows.size >= SQUELCH.MAX_KEYS) {
                const [oldestKey, oldest] = state.windows.entries().next().value;
                if (oldest.n_suppressed > 0) notice += summaryLine(oldest, now);
                state.windows.delete(oldestKey);
            }
            window = { start: now, n_seen: 0, n_suppressed: 0, firstSuppressed: unit, lastSuppressed: unit };
            state.windows.set(key, window);
        }
        window.n_seen += 1;
        if (window.n_seen <= SQUELCH.MAX_REPEATS) {
            isPassed = true;
            state.n_bytes += size;
        } else {
            if (window.n_suppressed === 0) window.firstSuppressed = unit;
            window.n_suppressed += 1;
            window.lastSuppressed = unit;
        }
    }
    return { isPassed, notice };
}

/**
 * 출력 한 덩이를 줄(단위)마다 판정해, 먼저 써야 할 요약·안내(`prefix`)와 실제로 쓸 글(`passed`)을 돌려준다.
 * 모두 통과하면 `passed`는 입력과 같은 문자열이고, 모두 버리면 빈 문자열이다.
 *
 * 오류가 아닌 단위는 같은 틀(숫자를 지운 첫 줄)이 창 안에서 `MAX_REPEATS`번까지 통과하고 그 뒤는 버린다.
 * 세션 누적이 `MAX_SESSION_BYTES`를 넘으면 안내를 한 번 쓰고 오류가 아닌 출력은 모두 버린다.
 * 오류 수준 단위는 이 규칙을 건너뛰고 `MAX_ERROR_BYTES`까지만 통과한다.
 *
 * Examples
 * --------
 * | 입력                                           | passed             | prefix                        |
 * |------------------------------------------------|--------------------|-------------------------------|
 * | 같은 틀 1~5번째                                | 입력 그대로        | 빈 문자열                     |
 * | 같은 틀 6번째 이후(10초 안)                    | 빈 문자열          | 빈 문자열                     |
 * | 같은 틀, 10초 뒤 첫 줄                         | 입력 그대로        | "repeated N more times" 요약  |
 * | 100 MB 넘은 뒤의 `ERROR …`                    | 입력 그대로        | 빈 문자열                     |
 * | 100 MB 넘은 뒤의 일반 줄                       | 빈 문자열          | 안내 한 줄(한 번만)           |
 */
function squelch(
    state,
    text,
    now,
) {
    let prefix = flushExpired(state, now);
    let passed = '';
    for (const unit of splitUnits(text)) {
        const verdict = judgeUnit(state, unit, now);
        prefix += verdict.notice;
        if (verdict.isPassed) passed += unit;
    }
    return { prefix, passed };
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
            if (decision.passed === text) {
                // 전부 통과하면 받은 그대로(바이트 포함) 쓴다.
                result = writers[index](chunk, encoding, callback);
            } else if (decision.passed) {
                result = writers[index](decision.passed, typeof encoding === 'string' ? encoding : undefined, done);
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

module.exports = { SQUELCH, createSquelchState, splitUnits, squelchKey, flushExpired, squelch, installLogSquelch };
