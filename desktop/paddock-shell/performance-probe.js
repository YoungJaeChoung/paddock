/**
 * 메인 프로세스의 멈춤과 앱 전체(렌더러·GPU·백엔드 포함) 메모리를 주기적으로 재서 파일에 남긴다.
 *
 * 성능을 고치기 전에 "어디서 얼마나 멈추고 얼마나 쓰는가"를 같은 방식으로 재야 고친 효과를 비교할 수 있어서 존재한다.
 * 평소에는 기록하지 않고, 이벤트 루프가 한도 이상 멈췄거나 메모리가 한도 이상일 때와 일정 간격마다만 한 줄씩 남긴다.
 * 파일이 한도를 넘으면 오래된 절반을 버린다. 프로세스 종류별 메모리와 지연 시간만 재고 사용자 입력·화면 내용은 읽지 않는다.
 */

const fs = require('node:fs');
const path = require('node:path');
const { monitorEventLoopDelay } = require('node:perf_hooks');

const PROBE = {
    // 한 번 재는 주기(ms).
    SAMPLE_MS: 10000,
    // 한 주기 안에서 이벤트 루프가 이만큼 이상 멈췄으면 기록한다(ms).
    STALL_MS: 200,
    // 앱 전체 작업 메모리가 이만큼 이상이면 기록한다(MB).
    MEMORY_MB: 3072,
    // 이상이 없어도 이 간격마다 한 줄은 남긴다(ms). 추세를 보려는 용도다.
    HEARTBEAT_MS: 300000,
    // 기록 파일 크기 한도(바이트). 넘으면 뒤쪽 절반만 남긴다.
    MAX_FILE_BYTES: 1048576,
    FILE_NAME: 'performance.log',
};

/**
 * 한 주기의 측정값을 기록 한 줄(객체)로 만들고 남길지 정한다.
 *
 * `delay`는 이벤트 루프 지연 통계(ns), `metrics`는 프로세스별 `{ type, workingSetKb }` 목록이다.
 * 남기는 조건은 멈춤 한도 초과, 메모리 한도 초과, 마지막 기록 뒤 하트비트 간격 경과 중 하나다.
 *
 * Examples:
 *   max 350ms                         → { isLogged: true,  reason: 'stall' }
 *   작업 메모리 합 4GB                  → { isLogged: true,  reason: 'memory' }
 *   이상 없음, 마지막 기록 6분 전        → { isLogged: true,  reason: 'heartbeat' }
 *   이상 없음, 마지막 기록 1분 전        → { isLogged: false, reason: null }
 */
function evaluateSample(
    delay,
    metrics,
    now,
    lastLoggedAt,
) {
    const stallMs = Math.round(delay.max / 1e6);
    const p99Ms = Math.round(delay.p99 / 1e6);
    const memoryMb = Math.round(metrics.reduce((sum, processMetric) => sum + processMetric.workingSetKb, 0) / 1024);
    const memoryByType = {};
    for (const processMetric of metrics) {
        memoryByType[processMetric.type] = (memoryByType[processMetric.type] || 0) + Math.round(processMetric.workingSetKb / 1024);
    }
    let reason = null;
    if (stallMs >= PROBE.STALL_MS) reason = 'stall';
    else if (memoryMb >= PROBE.MEMORY_MB) reason = 'memory';
    else if (lastLoggedAt === null || now - lastLoggedAt >= PROBE.HEARTBEAT_MS) reason = 'heartbeat';
    const entry = { at: new Date(now).toISOString(), reason, stallMs, p99Ms, memoryMb, memoryByType };
    return { isLogged: reason !== null, reason, entry };
}

/** 기록 파일이 한도를 넘었으면 뒤쪽 절반(온전한 줄부터)만 남긴다. */
function trimLog(
    file,
) {
    const size = fs.statSync(file).size;
    if (size > PROBE.MAX_FILE_BYTES) {
        const text = fs.readFileSync(file, 'utf8');
        const tail = text.slice(Math.floor(text.length / 2));
        fs.writeFileSync(file, tail.slice(tail.indexOf('\n') + 1));
    }
}

/**
 * 측정을 시작하고 멈추는 함수를 반환한다. `getMetrics`는 Electron의 `app.getAppMetrics()`를 `{ type, workingSetKb }`로 바꿔 돌려준다.
 * 기록을 쓰지 못해도(폴더 없음·권한) 앱 동작에는 영향이 없도록 오류를 삼킨다.
 */
function startPerformanceProbe(
    directory,
    getMetrics,
) {
    const histogram = monitorEventLoopDelay({ resolution: 20 });
    histogram.enable();
    const file = path.join(directory, PROBE.FILE_NAME);
    let lastLoggedAt = null;
    const timer = setInterval(() => {
        try {
            const now = Date.now();
            const result = evaluateSample(
                { max: histogram.max, p99: histogram.percentile(99) },
                getMetrics(),
                now,
                lastLoggedAt,
            );
            histogram.reset();
            if (result.isLogged) {
                fs.mkdirSync(directory, { recursive: true });
                fs.appendFileSync(file, `${JSON.stringify(result.entry)}\n`);
                trimLog(file);
                lastLoggedAt = now;
            }
        } catch (error) {
            console.warn('Performance probe could not record a sample:', error);
        }
    }, PROBE.SAMPLE_MS);
    // 측정 때문에 앱 종료가 막히지 않게 한다.
    timer.unref();
    return () => {
        clearInterval(timer);
        histogram.disable();
    };
}

module.exports = {
    PROBE,
    evaluateSample,
    trimLog,
    startPerformanceProbe,
};
