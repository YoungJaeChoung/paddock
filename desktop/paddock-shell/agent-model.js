/**
 * 터미널 안 AI 에이전트(claude·codex 등)의 활동 표시와 "작업이 끝났다" 추정에 쓰는 순수 계산.
 *
 * 에이전트는 일하는 동안 화면(스피너·스트리밍 출력)을 계속 고쳐 쓰고, 끝나거나 사용자 답을 기다리면 조용해진다.
 * Enter 뒤 오래 이어진 출력이 멎으면 알린다. 작업 완료나 입력 대기를 확정하지 않는다.
 * 짧게 끝난 응답(몇 초)은 알리지 않는다 — 기다리는 사이 다른 일을 하지 않았을 가능성이 크다.
 */

const ACTIVITY = {
    // 최근 출력과 입력 직후 대기를 구분하는 관찰 시간(ms).
    QUIET_MS: 3000,
    // 이만큼 넘게 일한 경우만 알린다(ms).
    MIN_BUSY_MS: 5000,
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
    return { armedAt: null, busySince: null, lastOutput: null, agent: false };
}

/** 사용자 입력. Enter가 들어오면 그 뒤의 출력을 "일"로 센다. */
function noteInput(
    activity,
    data,
    now,
) {
    return data.includes('\r') ? { ...idle(), armedAt: now } : activity;
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
 * 터미널 입력·출력으로 관찰한 에이전트 상태. 프로그램을 모르면 unknown, 에이전트가 아니면 null이다.
 *
 * Enter 직후 첫 출력을 기다리는 3초는 waiting, 최근 출력은 working, 그 밖에는 quiet이다.
 * 이름은 내부 호환용이며 화면에서는 Sent / Output / Quiet으로 표시한다.
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
        if (activity.armedAt !== null && activity.busySince === null && now - activity.armedAt < ACTIVITY.QUIET_MS) {
            state = 'waiting';
        } else if (activity.lastOutput !== null && now - activity.lastOutput < ACTIVITY.QUIET_MS) {
            state = 'working';
        }
    }
    return state;
}

/**
 * 주기적으로 불러 긴 출력이 멎었는지 본다. `agentNow`는 지금 이 터미널의 프로그램이 에이전트인지다.
 * 일하는 동안 한 번이라도 에이전트였으면 끝났을 때 알린다(`claude -p`처럼 끝나며 셸로 돌아가는 경우 포함).
 * 반환: `{ activity, finished }` — `finished`가 true면 알릴 때다.
 */
function settle(
    activity,
    now,
    agentNow,
) {
    let next = activity.busySince !== null && agentNow ? { ...activity, agent: true } : activity;
    let finished = false;
    if (next.busySince !== null && now - next.lastOutput >= ACTIVITY.QUIET_MS) {
        finished = next.agent && next.lastOutput - next.busySince >= ACTIVITY.MIN_BUSY_MS;
        next = idle();
    }
    return { activity: next, finished };
}

module.exports = {
    ACTIVITY,
    AGENT_NAMES,
    programName,
    isAgent,
    isShell,
    idle,
    noteInput,
    noteOutput,
    activityState,
    settle,
};
