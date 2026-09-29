/**
 * 상태 줄의 AI 사용량 게이지가 쓰는 순수 계산.
 *
 * 로그인 없이, 각 CLI가 스스로 남긴 값만 읽는다.
 * - Codex: 세션 기록(`~/.codex/sessions/…/rollout-*.jsonl`)의 `rate_limits`.
 * - Claude Code: 상태 줄(statusline) 명령이 받는 입력의 `rate_limits`(Pro·Max 구독).
 * 창(window) 하나는 `{ label: '5h' | 'week' | …, used: 0~100 정수, resetsAt: 초 단위 Unix 시각 또는 null }`이다.
 * 7일 창은 '7d' 대신 'week'로 적는다 — 상태 줄의 작은 글자에서 '5h'와 한 글자만 달라 헷갈리지 않게 모양부터 다르게 하고,
 * 줄임말('wk')은 뜻이 바로 안 읽혀 온전한 단어로 쓴다.
 */

function windowLabel(
    minutes,
) {
    let label = `${minutes}m`;
    if (minutes === 10080) label = 'week';
    else if (minutes % 1440 === 0) label = `${minutes / 1440}d`;
    else if (minutes % 60 === 0) label = `${minutes / 60}h`;
    return label;
}

function parseLine(
    line,
) {
    let value = null;
    try {
        value = JSON.parse(line);
    } catch {
        value = null;
    }
    return value;
}

/**
 * Codex 세션 기록 전체 문자열에서 마지막 사용량 한도를 창별로 돌려준다(짧은 창이 먼저).
 * 한도 기록이 없거나 줄이 깨졌으면 그 줄은 건너뛰고, 하나도 없으면 빈 목록이다.
 */
function codexWindows(
    text,
) {
    const lines = text.split('\n');
    let limits = null;
    for (let index = lines.length - 1; index >= 0 && !limits; index -= 1) {
        if (lines[index].includes('"rate_limits"')) {
            limits = parseLine(lines[index])?.payload?.rate_limits ?? null;
        }
    }
    const windows = [];
    for (const entry of [limits?.primary, limits?.secondary]) {
        if (entry && typeof entry.used_percent === 'number' && entry.window_minutes) {
            windows.push({ label: windowLabel(entry.window_minutes), used: Math.round(entry.used_percent), resetsAt: entry.resets_at });
        }
    }
    return windows.sort((left, right) => left.resetsAt - right.resetsAt);
}

/** Claude Code 상태 줄 입력에서 5시간·주간 창을 돌려준다. 구독이 아니면 빈 목록이다. */
function claudeWindows(
    input,
) {
    const windows = [];
    for (const [key, label] of [['five_hour', '5h'], ['seven_day', 'week']]) {
        const entry = input?.rate_limits?.[key];
        if (entry && typeof entry.used_percentage === 'number') {
            windows.push({ label, used: Math.round(entry.used_percentage), resetsAt: entry.resets_at });
        }
    }
    return windows;
}

/**
 * 지금 기준의 창 목록. 초기화 시각이 지난 창은 빼지 않고 사용량 0%·초기화 시각 없음으로 바꾼다.
 *
 * 기록은 CLI를 마지막으로 쓸 때 남으므로, 5시간 창이 초기화된 뒤 아직 안 썼으면 옛 값만 남아 있다.
 * 이 창을 빼면 한도가 있다는 사실 자체가 사라져 보여, 실제 상태(초기화되어 0%)로 보인다.
 * 초기화된 창의 다음 초기화 시각은 다음 요청이 새 창을 열 때 정해지므로 알 수 없다(null).
 *
 * Examples:
 *   now 200, [{ 5h, 90%, resetsAt 100 }, { week, 10%, resetsAt 300 }] → [{ 5h, 0%, null }, { week, 10%, 300 }]
 */
function currentWindows(
    windows,
    nowSeconds,
) {
    return windows.map(window => (window.resetsAt && window.resetsAt <= nowSeconds ? { ...window, used: 0, resetsAt: null } : window));
}

/**
 * 말풍선에 쓰는 창 이름. 짧은 표지를 풀어 쓴다.
 *
 * Examples:
 *   '5h' → '5-hour' · 'week' → 'weekly' · '45m' → '45m'
 */
function windowName(
    label,
) {
    const names = { '5h': '5-hour', week: 'weekly' };
    return names[label] ?? label;
}

function duration(
    seconds,
) {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    return days ? `${days}d ${hours}h` : `${hours}h ${minutes}m`;
}

/**
 * 게이지 말풍선 문구.
 *
 * Examples:
 *   { used: 23, resetsAt: now + 2h10m } → "23% used · 77% left · resets in 2h 10m"
 *   { used: 0, resetsAt: null }         → "0% used · 100% left · reset — a new window starts with your next request"
 */
function describe(
    window,
    nowSeconds,
) {
    // 초기화 시각이 null이면 이미 초기화된 창이고(currentWindows), 없으면(undefined) 기록에 시각이 없던 창이다.
    let reset = '';
    if (window.resetsAt) reset = ` · resets in ${duration(Math.max(0, window.resetsAt - nowSeconds))}`;
    else if (window.resetsAt === null) reset = ' · reset — a new window starts with your next request';
    return `${window.used}% used · ${Math.max(0, 100 - window.used)}% left${reset}`;
}

/**
 * Claude Code 설정에 Paddock 상태 줄 명령을 넣는다.
 * 원래 쓰던 상태 줄은 `previous`로 돌려주어 Paddock 명령이 이어 부르게 한다. 이미 Paddock 명령이면 `previous`는 null.
 */
function installStatusLine(
    settings,
    command,
) {
    const current = settings.statusLine;
    const isOurs = current?.command === command;
    return {
        settings: { ...settings, statusLine: { type: 'command', command } },
        previous: !isOurs && current ? current : null,
    };
}

/** Paddock 상태 줄을 걷어 내고 원래 상태 줄(없으면 빈 상태)로 되돌린다. */
function restoreStatusLine(
    settings,
    previous,
) {
    const restored = { ...settings };
    if (previous) restored.statusLine = previous;
    else delete restored.statusLine;
    return restored;
}

module.exports = {
    windowLabel,
    windowName,
    codexWindows,
    claudeWindows,
    currentWindows,
    describe,
    duration,
    installStatusLine,
    restoreStatusLine,
};
