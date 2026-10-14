/**
 * 화면(렌더러)의 긴 작업과 키 입력이 화면에 반영되기까지의 지연을 주기적으로 재서 백엔드에 남기게 한다.
 *
 * 키 입력은 화면 프로세스의 메인 스레드가 처리하므로, 그 스레드가 다른 일(목록 다시 그리기·터미널 출력 해석)로 바쁘면 글자가 늦게 나타난다.
 * 메인 프로세스를 재는 performance-probe.js로는 이 지연이 보이지 않아서 존재한다. 기록은 같은 performance.log에 `source: 'renderer'`로 쌓인다.
 * 이벤트 이름과 걸린 시간만 재고, 누른 키나 화면 내용은 읽지 않는다.
 */

const RENDERER_PROBE = {
    // 한 번 모아 판정하는 주기(ms).
    SAMPLE_MS: 10000,
    // 브라우저가 "긴 작업"으로 알리는 최소 길이는 50ms다. 한 주기 안에 이만큼 이상 멈춘 작업이 있으면 기록한다(ms).
    STALL_MS: 200,
    // 키를 누른 뒤 화면에 반영되기까지 이만큼 이상 걸린 입력을 느린 입력으로 센다(ms).
    SLOW_KEY_MS: 50,
    // 한 주기 안에 이만큼 이상 걸린 입력이 있으면 기록한다(ms).
    KEY_DELAY_MS: 100,
    // 이상이 없어도 이 간격마다 한 줄은 남긴다(ms).
    HEARTBEAT_MS: 300000,
    // 입력 지연으로 보는 이벤트 이름. 한글 조합 입력은 composition 이벤트로도 온다.
    KEY_EVENT: /^(key|composition|beforeinput$|input$)/,
};

/**
 * 한 주기의 측정값을 기록 한 줄(객체)로 만들고 남길지 정한다.
 *
 * `taskMs`는 그 주기의 긴 작업 길이 목록, `keyDelayMs`는 입력 하나(같은 키 누름에서 나온 이벤트 묶음)마다 화면 반영까지 걸린 시간 목록이다.
 * 남기는 조건은 긴 작업 한도 초과(stall), 입력 지연 한도 초과(input), 마지막 기록 뒤 하트비트 간격 경과(heartbeat) 중 하나다.
 *
 * Examples:
 *   긴 작업 250ms                       → { isLogged: true,  reason: 'stall' }
 *   긴 작업 80ms, 입력 지연 120ms         → { isLogged: true,  reason: 'input' }
 *   이상 없음, 마지막 기록 6분 전         → { isLogged: true,  reason: 'heartbeat' }
 *   이상 없음, 마지막 기록 1분 전         → { isLogged: false, reason: null }
 */
function evaluateRendererSample(
    taskMs,
    keyDelayMs,
    now,
    lastLoggedAt,
) {
    const longestTaskMs = Math.round(Math.max(0, ...taskMs));
    const keyDelayMaxMs = Math.round(Math.max(0, ...keyDelayMs));
    let reason = null;
    if (longestTaskMs >= RENDERER_PROBE.STALL_MS) reason = 'stall';
    else if (keyDelayMaxMs >= RENDERER_PROBE.KEY_DELAY_MS) reason = 'input';
    else if (lastLoggedAt === null || now - lastLoggedAt >= RENDERER_PROBE.HEARTBEAT_MS) reason = 'heartbeat';
    const entry = {
        at: new Date(now).toISOString(),
        source: 'renderer',
        reason,
        longestTaskMs,
        n_longTasks: taskMs.length,
        longTaskTotalMs: Math.round(taskMs.reduce((sum, ms) => sum + ms, 0)),
        keyDelayMaxMs,
        n_slowKeys: keyDelayMs.filter(ms => ms >= RENDERER_PROBE.SLOW_KEY_MS).length,
    };
    return { isLogged: reason !== null, reason, entry };
}

/**
 * 측정을 시작하고 멈추는 함수를 반환한다. 기록할 주기마다 `send(entry)`를 부른다.
 * 브라우저가 긴 작업·입력 지연 측정을 지원하지 않으면 그 항목은 빈 목록으로 남는다.
 */
function startRendererProbe(
    send,
) {
    let taskMs = [];
    // 같은 키 누름(interactionId)에서 나온 이벤트 중 가장 오래 걸린 값을 그 입력의 지연으로 본다.
    let keyDelays = new Map();
    let lastLoggedAt = null;
    const observers = [];
    const observe = (type, options, onEntry) => {
        try {
            const observer = new PerformanceObserver(list => list.getEntries().forEach(onEntry));
            observer.observe({ type, buffered: false, ...options });
            observers.push(observer);
        } catch {
            // 지원하지 않는 측정 종류는 건너뛴다.
        }
    };
    observe('longtask', {}, entry => taskMs.push(entry.duration));
    observe('event', { durationThreshold: 16 }, (entry) => {
        if (RENDERER_PROBE.KEY_EVENT.test(entry.name)) {
            const key = entry.interactionId || `${entry.name}:${entry.startTime}`;
            keyDelays.set(key, Math.max(keyDelays.get(key) || 0, entry.duration));
        }
    });
    const timer = setInterval(() => {
        const now = Date.now();
        const result = evaluateRendererSample(taskMs, [...keyDelays.values()], now, lastLoggedAt);
        taskMs = [];
        keyDelays = new Map();
        if (result.isLogged) {
            lastLoggedAt = now;
            send(result.entry);
        }
    }, RENDERER_PROBE.SAMPLE_MS);
    return () => {
        clearInterval(timer);
        observers.forEach(observer => observer.disconnect());
    };
}

module.exports = { RENDERER_PROBE, evaluateRendererSample, startRendererProbe };
