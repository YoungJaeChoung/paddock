/**
 * 터미널 안 AI 에이전트(claude·codex 등)의 활동 표시와 "작업이 끝났다" 추정에 쓰는 순수 계산.
 *
 * 에이전트는 일하는 동안 화면(스피너·스트리밍 출력)을 계속 고쳐 쓰고, 끝나거나 사용자 답을 기다리면 조용해진다.
 * Enter 뒤 오래 이어진 출력이 멎으면 알린다. 작업 완료나 입력 대기를 확정하지 않는다.
 * 짧게 끝난 응답(몇 초)은 알리지 않는다 — 기다리는 사이 다른 일을 하지 않았을 가능성이 크다.
 *
 * 에이전트가 터미널 제어 시퀀스(OSC 7501, Program Status)로 "일하는 중·사용자를 기다림·끝남"을 직접 알리면
 * 그 신호를 우선한다. 출력이 오래 없는 빌드를 끝난 것으로 잘못 알리지 않고, 답을 기다리는 상태도 알 수 있다.
 * 신호를 보내지 않는 에이전트는 위의 출력 멎음 판정을 그대로 쓴다.
 */

const ACTIVITY = {
    // 최근 출력과 입력 직후 대기를 구분하는 관찰 시간(ms).
    QUIET_MS: 3000,
    // 이만큼 넘게 일한 경우만 알린다(ms).
    MIN_BUSY_MS: 5000,
    // 완료 소리를 낸 뒤 이 시간 안에 오는 다음 소리는 내지 않는다(ms). 여러 에이전트가 거의 동시에 끝날 때 소리가 겹쳐 울리는 것을 막는다.
    SOUND_COOLDOWN_MS: 4000,
};

// 에이전트가 터미널로 직접 알리는 상태(OSC 7501 Program Status)의 규칙.
const PROGRAM_STATUS = {
    OSC_CODE: 7501,
    STATES: ['working', 'blocked', 'done', 'idle', 'error', 'clear'],
};

// 에이전트로 알아보는 프로그램 이름. 실행 파일 이름이나 스크립트 경로의 한 부분과 맞춘다.
const AGENT_NAMES = ['claude', 'codex', 'gemini', 'opencode', 'cursor-agent', 'aider', 'copilot', 'amp', 'droid', 'kimi', 'qwen'];

// 스크립트를 실행하는 런타임·셸. 이 경우 실제 프로그램 이름은 스크립트 쪽에서 찾는다(`#!/bin/bash` 스크립트는 명령줄이 `/bin/bash 스크립트`다).
// 스크립트 없이 셸만 떠 있으면 셸 이름이다.
const RUNTIMES = ['node', 'bun', 'deno', 'python', 'python3', 'bash', 'sh', 'zsh'];

// 대화형 셸 이름. 앞쪽 프로그램이 이 이름이면 터미널은 명령을 기다리는 빈 셸이다.
const SHELL_NAMES = ['bash', 'sh', 'zsh', 'fish', 'dash', 'ksh', 'tcsh', 'csh', 'nu', 'pwsh', 'powershell', 'cmd'];

function baseName(
    file,
) {
    return file.split(/[\\/]/).pop().replace(/\.(exe|cmd|js|mjs|cjs|py)$/i, '');
}

/**
 * 명령줄(argv)에서 사람이 알아볼 프로그램 이름을 뽑는다. 에이전트면 에이전트 이름이다.
 * 빈 명령줄이면 null.
 *
 * Examples:
 *   ['/home/me/.local/bin/claude']                              → 'claude'
 *   ['node', '/usr/lib/node_modules/@openai/codex/bin/codex.js'] → 'codex'
 *   ['node', 'server.js']                                         → 'server'
 *   ['-bash']                                                     → 'bash'
 */
function programName(
    argv,
) {
    let name = null;
    if (argv.length && argv[0]) {
        const executable = baseName(argv[0]).replace(/^-/, '');
        // Interactive Bash can name its startup file with --rcfile. That file is not the running program.
        const interactiveBash = executable === 'bash' && argv.slice(1).some(arg => /^-[^-]*i/.test(arg));
        const script = RUNTIMES.includes(executable) && !interactiveBash ? argv.slice(1).find(arg => arg && !arg.startsWith('-')) : null;
        const lookIn = [executable, ...(script ? script.split(/[\\/]/) : [])].map(part => part.toLowerCase());
        const agent = AGENT_NAMES.find(agentName => lookIn.some(part => part === agentName || part.replace(/\.(js|mjs|cjs)$/, '') === agentName || part === `${agentName}-code`));
        name = agent || (script ? baseName(script) : executable);
    }
    return name;
}

function isAgent(
    name,
) {
    return AGENT_NAMES.includes(name);
}

/**
 * 프로그램 이름(`programName`의 결과)이 대화형 셸인지. 셸만 떠 있는 터미널은 셸을 바꿔도 잃을 실행 중인 작업이 없다.
 * 셸이 실행한 스크립트는 스크립트 이름이라 셸이 아니고, 이름을 모르면(null) 셸로 보지 않는다.
 *
 * Examples:
 *   'bash' → true, 'pwsh' → true, 'build' → false, 'claude' → false, null → false
 */
function isShell(
    name,
) {
    return SHELL_NAMES.includes(name);
}

/** 아직 일하지 않는 상태. */
function idle() {
    return { armedAt: null, busySince: null, lastOutput: null, agent: false, signal: null };
}

// 중단 키(Esc·Ctrl+C) 한 바이트. 에이전트는 사용자가 중단하면 끝남 신호를 보내지 않으므로 이 입력으로 대신 판단한다.
const INTERRUPT_KEYS = ['\x1b', '\x03'];

/**
 * 사용자 입력. Enter가 들어오면 그 뒤의 출력을 "일"로 센다.
 * 일하는 중(working)이나 사용자를 기다린다(blocked)고 알린 터미널에 중단 키가 정확히 한 바이트로 오면 신호를 idle로 바꾼다(알림 없음).
 * 기다리는 중의 Esc·Ctrl+C는 권한·질문 대화창을 닫는 것이고, 이때도 에이전트가 끝남·유휴 신호를 보내지 않아 blocked가 남기 때문이다.
 * 화살표 키(`\x1b[A`)·붙여넣기처럼 여러 바이트 입력은 중단이 아니다. 이후 오는 신호가 다시 우선한다.
 */
function noteInput(
    activity,
    data,
    now,
) {
    let next = activity;
    if (data.includes('\r')) {
        // 에이전트가 알려 온 상태(signal)는 입력으로 지우지 않는다. 이 터미널이 신호를 보낸다는 사실이 이어져야 한다.
        next = { ...idle(), armedAt: now, signal: activity.signal };
    } else if (['working', 'blocked'].includes(activity.signal?.state) && INTERRUPT_KEYS.includes(data)) {
        next = { ...activity, signal: { state: 'idle', kind: null, workingSince: activity.signal.workingSince, alert: false } };
    }
    return next;
}

/**
 * 프로그램 출력을 최근 활동으로 기록한다. 완료 알림의 작업 시간은 Enter 뒤의 출력만 센다.
 * 시작 화면·프롬프트 다시 그리기는 완료 알림으로 세지 않지만, 출력이 재개됐다는 표시는 유지한다.
 */
function noteOutput(
    activity,
    now,
) {
    return { ...activity, busySince: activity.armedAt === null ? activity.busySince : activity.busySince ?? now, lastOutput: now };
}

/**
 * 에이전트가 OSC 7501로 보낸 본문을 해석한다. 지원 여부를 묻는 질의(`?`)이거나 상태 알림이다.
 * 서브에이전트의 기록(`id=…`이 붙음)과 알 수 없는 상태는 터미널 전체의 상태가 아니므로 null이다.
 *
 * Examples:
 *   '?'                                             → { isQuery: true }
 *   'state=working:app=claude-code'                 → { state: 'working', kind: null }
 *   'state=blocked:app=claude-code:kind=permission' → { state: 'blocked', kind: 'permission' }
 *   'state=working:app=claude-code:id=sub1'         → null
 *   'state=sleeping'                                → null
 */
function parseProgramStatus(
    data,
) {
    let result = null;
    if (data.startsWith('?')) {
        result = { isQuery: true };
    } else {
        const fields = new Map(data.split(':').map((field) => {
            const at = field.indexOf('=');
            return at > 0 ? [field.slice(0, at), field.slice(at + 1)] : [field, ''];
        }));
        const state = fields.get('state');
        if (PROGRAM_STATUS.STATES.includes(state) && !fields.has('id')) result = { state, kind: fields.get('kind') ?? null };
    }
    return result;
}

/**
 * 에이전트가 알린 상태를 활동 기록에 넣는다. 알릴 때(`alert`)가 되면 다음 `settle`이 한 번 알린다.
 *
 * 알리는 경우: 사용자를 기다리게 되었을 때(blocked)·오류가 났을 때(error)는 바로, 끝났을 때(done)는 MIN_BUSY_MS 넘게
 * 일한 뒤에만 알린다. 사용자를 기다리는 에이전트는 시간과 무관하게 사용자가 필요해서다.
 * `clear`는 에이전트가 상태 표시를 거둔다는 뜻이라 신호 사용을 멈추고 출력 멎음 판정으로 돌아간다.
 *
 * Examples:
 *   working(0) → done(6000)    → alert true
 *   working(0) → done(2000)    → alert false
 *   working(0) → blocked(1000) → alert true
 *   done → blocked → blocked   → 첫 blocked에서만 alert
 */
function noteStatus(
    activity,
    status,
    now,
) {
    let next = activity;
    if (status.state === 'clear') {
        next = { ...activity, signal: null };
    } else {
        const previous = activity.signal;
        const isWorking = previous?.state === 'working';
        // 일을 시작한 시각은 끝남·유휴·오류로 일이 마무리되면 비운다. 중단 폴백이 만든 idle만 이 시각을 남겨,
        // 폴백이 틀려 일이 이어졌다가 진짜 done이 오면 그 일한 시간으로 알린다.
        const isFinishing = ['done', 'idle', 'error'].includes(status.state);
        const workingSince = status.state === 'working' ? (isWorking ? previous.workingSince : now) : (isFinishing ? null : previous?.workingSince ?? null);
        let alert = previous?.alert ?? false;
        if (status.state === 'blocked' || status.state === 'error') {
            alert = alert || previous?.state !== status.state;
        } else if (status.state === 'done') {
            alert = alert || (previous?.workingSince != null && now - previous.workingSince >= ACTIVITY.MIN_BUSY_MS);
        }
        next = { ...activity, signal: { state: status.state, kind: status.kind, workingSince, alert } };
    }
    return next;
}

/**
 * 앞쪽 프로그램이 한 에이전트에서 다른 에이전트로 바뀌면 이전 에이전트가 알린 상태를 지운다.
 * 새 에이전트가 신호를 보내지 않으면(예: claude 뒤 codex) 낡은 상태가 그대로 남기 때문이다.
 * 셸로 돌아온 경우와 처음 알아본 경우는 건드리지 않는다. 셸 복귀는 `settle`이 알림을 정리한다.
 *
 * Examples:
 *   ('claude', 'codex')     → 신호를 지운다
 *   ('claude', 'claude')    → 그대로
 *   ('claude', 'bash')      → 그대로
 *   (undefined, 'claude')   → 그대로
 */
function noteProgram(
    activity,
    previous,
    current,
) {
    const isSwitched = isAgent(previous) && isAgent(current) && previous !== current;
    return isSwitched ? { ...activity, signal: null } : activity;
}

/**
 * 완료 소리를 낼지. 마지막 소리 뒤 SOUND_COOLDOWN_MS가 지났거나 아직 낸 적이 없을 때만 낸다.
 * 소리만 거르고 완료 표시(미확인 점)는 그대로 붙는다 — 어느 터미널이 끝났는지는 표시로 알 수 있다.
 *
 * Examples:
 *   lastSoundAt null, now 1000     → true
 *   lastSoundAt 1000, now 3000     → false
 *   lastSoundAt 1000, now 5000     → true
 */
function shouldSound(
    lastSoundAt,
    now,
) {
    return lastSoundAt === null || now - lastSoundAt >= ACTIVITY.SOUND_COOLDOWN_MS;
}

/**
 * 에이전트가 직접 알린 상태(working·blocked·done·idle·error). 알린 적이 없거나 에이전트가 아니면 null이다.
 * 화면은 이 값이 있으면 "에이전트가 알린 상태"로, 없으면 "관찰로 추정한 상태"로 구분해 표시한다.
 * `activityState`의 working은 두 경우 모두 나오므로 출처를 가르려면 이 함수를 쓴다.
 *
 * Examples:
 *   signal working, agentNow true  → 'working'
 *   signal blocked, agentNow false → null
 *   signal 없음                    → null
 */
function signalOf(
    activity,
    agentNow,
) {
    return agentNow && activity.signal ? activity.signal.state : null;
}

/**
 * 터미널 상태. 프로그램을 모르면 unknown, 에이전트가 아니면 null이다.
 *
 * 에이전트가 알린 상태(`signalOf`)가 있으면 그 값(working·blocked·done·idle·error)을 그대로 돌려준다.
 * 없을 때만 터미널 입력·출력으로 추정한다: Enter 직후 첫 출력을 기다리는 3초는 waiting, 최근 출력은 working, 그 밖에는 quiet이다.
 * 추정 쪽 이름은 내부 호환용이며 화면에서는 Sent / Output / Quiet으로 표시한다.
 * 초기 화면이나 프롬프트를 다시 그리는 출력도 최근 활동에 포함된다.
 * 출력이 멎어도 내부 계산이나 사용자 답을 기다리는지는 알 수 없으므로 완료 여부는 확정하지 않는다.
 * 완료 알림의 최소 작업 시간과 달리 짧은 응답도 같은 표시 규칙을 쓴다.
 */
function activityState(
    activity,
    now,
    agentNow,
) {
    let state = null;
    if (agentNow === null || agentNow === undefined) {
        state = 'unknown';
    } else if (agentNow) {
        state = 'quiet';
        if (activity.signal) {
            // 에이전트가 직접 알린 상태가 추정보다 정확하다. 출력이 없어도 조용한 것이 아니고, 출력이 있어도 알린 상태를 따른다.
            state = activity.signal.state;
        } else if (activity.armedAt !== null && activity.busySince === null && now - activity.armedAt < ACTIVITY.QUIET_MS) {
            state = 'waiting';
        } else if (activity.lastOutput !== null && now - activity.lastOutput < ACTIVITY.QUIET_MS) {
            state = 'working';
        }
    }
    return state;
}

/**
 * 화면에 보일 상태 글자와 설명. 에이전트가 알린 상태(`reported`)와 관찰로 추정한 상태는 같은 working이어도 글자가 다르다.
 * 알린 상태는 Working / Needs input / Idle / Error, 추정은 Output / Sent / Quiet이다. 끝남(done)은 다음 프롬프트를 기다리는 Idle이다.
 * 모르는 상태는 null이다.
 *
 * Examples:
 *   ('working', true)  → ['Working', …]
 *   ('working', false) → ['Output', …]
 *   ('blocked', true)  → ['Needs input', …]
 *   ('done', true)     → ['Idle', …]
 *   ('unknown', false) → null
 */
function stateText(
    state,
    reported,
) {
    const texts = reported ? {
        working: ['Working', 'The agent is working on a request.'],
        blocked: ['Needs input', 'The agent is waiting for your permission or an answer.'],
        done: ['Idle', 'The agent finished its last request and is waiting for your next prompt.'],
        idle: ['Idle', 'The agent is waiting for your next prompt.'],
        error: ['Error', 'The agent stopped with an error.'],
    } : {
        working: ['Output', 'Terminal output was detected in the last 3 seconds; this can include prompt redraws. This does not confirm the agent is working.'],
        waiting: ['Sent', 'Enter was sent less than 3 seconds ago; no subsequent terminal output has been detected. This does not confirm the agent received a request.'],
        quiet: ['Quiet', 'No terminal output in the last 3 seconds. The agent may be waiting for input or still thinking.'],
    };
    return texts[state] ?? null;
}

/**
 * 주기적으로 불러 긴 출력이 멎었는지 본다. `agentNow`는 지금 이 터미널의 프로그램이 에이전트인지다.
 * 일하는 동안 한 번이라도 에이전트였으면 끝났을 때 알린다(`claude -p`처럼 끝나며 셸로 돌아가는 경우 포함).
 * 에이전트가 상태를 직접 알려 온 터미널(`signal`)은 출력 멎음을 보지 않고 그 신호가 알릴 때를 정한다.
 * 반환: `{ activity, finished }` — `finished`가 true면 알릴 때다.
 */
function settle(
    activity,
    now,
    agentNow,
) {
    let next = activity.busySince !== null && agentNow ? { ...activity, agent: true } : activity;
    let finished = false;
    if (next.signal) {
        // 에이전트가 직접 알린 상태가 있으면 출력이 멎은 것은 보지 않는다. 출력 없이 도는 빌드는 끝난 것이 아니다.
        if (agentNow === false) {
            // 에이전트가 끝나 셸로 돌아왔다. 알릴 것이 남았거나 일하다 끊겼다면 알리고 기록을 비운다.
            const wasWorking = next.signal.state === 'working' && now - next.signal.workingSince >= ACTIVITY.MIN_BUSY_MS;
            finished = next.signal.alert || wasWorking;
            next = idle();
        } else if (next.signal.alert) {
            finished = true;
            next = { ...next, signal: { ...next.signal, alert: false } };
        }
    } else if (next.busySince !== null && now - next.lastOutput >= ACTIVITY.QUIET_MS) {
        finished = next.agent && next.lastOutput - next.busySince >= ACTIVITY.MIN_BUSY_MS;
        next = idle();
    }
    return { activity: next, finished };
}

/**
 * 터미널 제목에서 에이전트가 진행 표시로 붙인 맨 앞 기호를 뗀 것.
 *
 * Claude Code 같은 에이전트는 일하는 동안 제목 맨 앞 기호를 1초쯤마다 바꾼다(`◐ 작업` ↔ `◑ 작업`). 화면 목록을 다시 그릴지 정할 때
 * 이 값을 비교하면, 기호만 바뀐 제목 변경으로 사이드바·탭 줄을 다시 그려 키 입력 처리를 늦추지 않는다.
 * 맨 앞이 기호 문자(유니코드 So) 하나이고 그 뒤가 공백이거나 끝일 때만 뗀다. 글자·숫자·경로로 시작하는 제목은 그대로다.
 *
 * Examples
 * --------
 * | 제목                    | 결과                |
 * | ----------------------- | ------------------- |
 * | `◐ Rivers paragraph`    | `Rivers paragraph`  |
 * | `✳ Claude Code`         | `Claude Code`       |
 * | `⠂ build`               | `build`             |
 * | `◐`                     | ``                  |
 * | `~/work`                | `~/work`            |
 * | `bash`                  | `bash`              |
 */
function titleWithoutProgress(
    title,
) {
    return String(title ?? '').replace(/^\p{So}(\s+|$)/u, '');
}

module.exports = {
    ACTIVITY,
    titleWithoutProgress,
    PROGRAM_STATUS,
    parseProgramStatus,
    noteStatus,
    noteProgram,
    AGENT_NAMES,
    programName,
    isAgent,
    isShell,
    idle,
    noteInput,
    noteOutput,
    signalOf,
    stateText,
    activityState,
    settle,
    shouldSound,
};
