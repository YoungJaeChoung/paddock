// 터미널에서 실행 중인 claude·codex가 어느 대화를 열고 있는지 읽어, 터미널을 다시 열 때 그 대화를 이어 열 수 있게 하는 순수 함수.
// 대화 식별자는 Claude가 프로세스마다 남기는 sessions 기록과 Codex가 대화마다 남기는 rollout 파일의 첫 줄에서 읽는다. 대화 내용은 읽지 않는다.
// 읽는 쪽이 파일 시스템·시각을 넘겨 받아, 실제 홈 폴더 없이 시험한다.
const nodePath = require('node:path');

class AgentResume {
    /** 대화 식별자(UUID)의 모양. Claude는 UUID v4, Codex는 v7이라 버전 칸은 가리지 않는다. */
    static UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    /** sessions 기록 파일 하나의 최대 크기. 이보다 크면 읽지 않는다. */
    static N_MAX_RECORD_BYTES = 16384;
    /** rollout 파일에서 첫 줄을 읽을 최대 바이트. 첫 줄에 지시문 전문이 들어가 길 수 있다. */
    static N_FIRST_LINE_BYTES = 65536;
    /** 프로세스 시작 시각을 초 단위로만 알 수 있어 두는 여유(ms). 대화 파일이 이 시각보다 이만큼 일러도 같은 프로세스의 것으로 본다. */
    static START_MARGIN_MS = 3000;
    /** Claude가 프로세스를 시작한 뒤 sessions 기록을 남기기까지 걸릴 수 있는 최대 시간(ms). */
    static RECORD_LAG_MS = 120000;
}

/**
 * Claude가 이 프로세스에 대해 남긴 sessions 기록에서 대화 식별자를 읽는다. 기록이 없거나 이 프로세스의 것이 아니면 null이다.
 * 기록의 `pid`가 같고 `kind`가 interactive(터미널에서 쓰는 대화형)일 때만 인정한다 — 하위 에이전트·비대화형 실행은 터미널의 대화가 아니다.
 *
 * Examples
 * --------
 * | sessions/42.json                                             | pid | 결과           |
 * |--------------------------------------------------------------|-----|----------------|
 * | `{"pid":42,"sessionId":"<uuid>","kind":"interactive"}`       | 42  | `{ sessionId }` |
 * | 같은 기록                                                    | 43  | null           |
 * | `"kind":"sdk"`                                               | 42  | null           |
 * | 파일 없음 · 깨진 JSON                                        | 42  | null           |
 * | 기록의 `startedAt`이 프로세스 시작(`processStartedAt`)보다 오래전 | 42  | null           |
 *
 * `processStartedAt`(ms)을 모르면 시작 시각 비교는 건너뛴다. WSL 스캔도 같은 판정을 쓰도록 기록 본문만 넘길 수 있다(`readFile`이 본문을 돌려준다).
 */
function claudeSessionOf(
    {
        configDirectory,
        pid,
        readFile,
        processStartedAt = null,
    },
) {
    let result = null;
    try {
        const text = readFile(nodePath.join(configDirectory, 'sessions', `${pid}.json`));
        if (typeof text === 'string' && text.length <= AgentResume.N_MAX_RECORD_BYTES) {
            const record = JSON.parse(text);
            // pid가 재사용되면 옛 프로세스의 기록이 남아 있을 수 있다. 기록의 시작 시각이 이 프로세스의 시작과 맞지 않으면 다른 프로세스의 기록이다.
            const isSameProcess = !Number.isFinite(processStartedAt) || !Number.isFinite(record.startedAt)
                || (record.startedAt >= processStartedAt - AgentResume.START_MARGIN_MS && record.startedAt <= processStartedAt + AgentResume.RECORD_LAG_MS);
            if (record && record.pid === pid && record.kind === 'interactive' && isSameProcess && AgentResume.UUID.test(String(record.sessionId || ''))) {
                result = { sessionId: record.sessionId, cwd: typeof record.cwd === 'string' ? record.cwd : '' };
            }
        }
    } catch { /* 기록이 없거나 읽을 수 없으면 대화를 모르는 것으로 둔다. */ }
    return result;
}

/**
 * rollout 파일의 첫 줄(JSON)에서 이어 열 수 있는 대화인지 판정하고 `{ id, cwd, startedAt }`을 읽는다. 아니면 null이다.
 * 터미널에서 직접 연 Codex 대화(originator가 `codex-tui`, source가 문자열)만 인정한다 — 하위 에이전트(source가 객체)·비대화형 실행(`codex_exec`)은 제외한다.
 */
function codexSessionHeader(
    firstLine,
) {
    let result = null;
    try {
        const payload = JSON.parse(firstLine).payload;
        const startedAt = Date.parse(payload.timestamp);
        if (payload.originator === 'codex-tui' && typeof payload.source === 'string' && AgentResume.UUID.test(String(payload.id || ''))
            && typeof payload.cwd === 'string' && payload.cwd && Number.isFinite(startedAt)) {
            result = { id: payload.id, cwd: payload.cwd, startedAt };
        }
    } catch { /* 첫 줄이 깨진 파일은 후보가 아니다. */ }
    return result;
}

/** 폴더 경로를 비교용으로 다듬는다: 끝의 구분자를 떼고 대소문자는 그대로 둔다. */
function sameFolder(
    left,
    right,
) {
    const trim = value => String(value).replace(/[\\/]+$/, '');
    return trim(left) === trim(right);
}

/**
 * Codex 프로세스가 연 대화를 후보 중에서 고른다. 후보가 정확히 하나일 때만 식별자를 돌려주고, 아니면 null이다.
 * 후보 조건은 작업 폴더가 같고 시작 시각이 프로세스 시작 이후(여유 3초)인 것이다.
 * 같은 폴더에서 연 대화가 둘 이상이면 어느 것이 이 터미널의 것인지 정할 수 없어 고르지 않는다 — 틀린 대화를 이어 여는 것보다 이어 열지 않는 편이 낫다.
 *
 * Examples
 * --------
 * | 후보(폴더, 시작)                          | 프로세스(폴더, 시작) | 결과      |
 * |-------------------------------------------|----------------------|-----------|
 * | (a, 100s)                                 | (a, 90s)             | 그 대화   |
 * | (a, 100s) · (a, 120s)                     | (a, 90s)             | null      |
 * | (b, 100s)                                 | (a, 90s)             | null      |
 * | (a, 50s)                                  | (a, 90s)             | null      |
 */
function chooseCodexSession(
    candidates,
    {
        cwd,
        processStartedAt,
    },
) {
    const matches = candidates.filter(item => sameFolder(item.cwd, cwd)
        && item.startedAt >= processStartedAt - AgentResume.START_MARGIN_MS);
    return matches.length === 1 ? { sessionId: matches[0].id, cwd: matches[0].cwd } : null;
}

/** 날짜를 sessions 아래 폴더 경로 조각 `YYYY/MM/DD`(로컬 시각)로 바꾼다. Codex가 파일 이름에 로컬 시각을 쓴다. */
function dateParts(
    millis,
) {
    const date = new Date(millis);
    const pad = value => String(value).padStart(2, '0');
    return [String(date.getFullYear()), pad(date.getMonth() + 1), pad(date.getDate())];
}

/**
 * Codex가 프로세스 시작 이후 만든 rollout 파일들의 머리 정보를 읽는다. 시작한 날과 다음 날 폴더(자정을 넘긴 경우)만 본다.
 * `fileSystem`은 `{ readdir(경로) → 이름 배열, readFirstLine(경로, 최대 바이트) → 문자열 }`이다. 폴더가 없으면 빈 목록이다.
 */
function listCodexSessions(
    {
        codexHome,
        processStartedAt,
        fileSystem,
    },
) {
    const headers = [];
    const day = 24 * 60 * 60 * 1000;
    for (const millis of [processStartedAt, processStartedAt + day]) {
        const directory = nodePath.join(codexHome, 'sessions', ...dateParts(millis));
        let names = [];
        try {
            names = fileSystem.readdir(directory);
        } catch { /* 그 날 폴더가 없으면 대화도 없다. */ }
        for (const name of names) {
            if (/^rollout-.*\.jsonl$/.test(name)) {
                try {
                    const header = codexSessionHeader(fileSystem.readFirstLine(nodePath.join(directory, name), AgentResume.N_FIRST_LINE_BYTES));
                    if (header && !headers.some(item => item.id === header.id)) headers.push(header);
                } catch { /* 읽을 수 없는 파일은 후보가 아니다. */ }
            }
        }
    }
    return headers;
}

/**
 * `ps -o etime=` 값(`[[일-]시:]분:초`)을 초로 바꾼다. 형식이 맞지 않으면 null이다.
 *
 * Examples
 * --------
 * | 입력          | 결과  |
 * |---------------|-------|
 * | `05:07`       | 307   |
 * | `01:02:03`    | 3723  |
 * | `2-00:00:01`  | 172801 |
 * | `abc`         | null  |
 */
function elapsedSeconds(
    text,
) {
    const match = /^\s*(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+)\s*$/.exec(String(text));
    return match ? ((Number(match[1] || 0) * 24 + Number(match[2] || 0)) * 60 + Number(match[3])) * 60 + Number(match[4]) : null;
}

/**
 * 셸 터미널 안에서 앞쪽 프로그램이 이어 열 수 있는 에이전트일 때 그 대화를 `{ provider, sessionId, cwd }`로 돌려준다. 아니면 null이다.
 * 에이전트가 아니거나 대화를 확신할 수 없으면 null이므로, 호출한 쪽은 이어 열지 않고 새 셸만 연다.
 *
 * `foreground`는 `{ program, pid, env, cwd, startedAt }`(앞쪽 프로그램 이름·pid·계정 환경·셸의 현재 폴더·프로세스 시작 시각(ms), 모르면 null),
 * `deps`는 `{ homeDirectory, readFile, fileSystem }`이다. 시작 시각은 Codex 대화를 고를 때 꼭 필요하고(모르면 Codex는 null), Claude 기록이 이 프로세스의 것인지 확인할 때는 알 때만 쓴다.
 */
function resumableAgent(
    foreground,
    deps,
) {
    let result = null;
    const env = foreground.env || {};
    if (foreground.program === 'claude' && Number.isSafeInteger(foreground.pid)) {
        const found = claudeSessionOf({
            configDirectory: env.CLAUDE_CONFIG_DIR || nodePath.join(env.HOME || deps.homeDirectory, '.claude'),
            pid: foreground.pid,
            readFile: deps.readFile,
            processStartedAt: foreground.startedAt,
        });
        // 계정 폴더를 환경 변수로 지정해 실행한 경우만 남긴다 — 복원 셸이 같은 계정 폴더로 --resume해야 대화를 찾는다.
        if (found) result = { provider: 'claude', sessionId: found.sessionId, cwd: found.cwd || foreground.cwd || '', ...(env.CLAUDE_CONFIG_DIR ? { configDir: env.CLAUDE_CONFIG_DIR } : {}) };
    } else if (foreground.program === 'codex' && foreground.cwd && Number.isFinite(foreground.startedAt)) {
        const candidates = listCodexSessions({
            codexHome: env.CODEX_HOME || nodePath.join(env.HOME || deps.homeDirectory, '.codex'),
            processStartedAt: foreground.startedAt,
            fileSystem: deps.fileSystem,
        });
        const found = chooseCodexSession(candidates, { cwd: foreground.cwd, processStartedAt: foreground.startedAt });
        if (found) result = { provider: 'codex', sessionId: found.sessionId, cwd: found.cwd };
    }
    return result;
}

module.exports = { AgentResume, claudeSessionOf, codexSessionHeader, chooseCodexSession, listCodexSessions, elapsedSeconds, resumableAgent };
