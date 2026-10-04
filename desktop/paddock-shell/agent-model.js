/**
 * 터미널 안 AI 에이전트(claude·codex 등)의 "작업이 끝났다" 판단에 쓰는 순수 계산.
 *
 * 에이전트는 일하는 동안 화면(스피너·스트리밍 출력)을 계속 고쳐 쓰고, 끝나거나 사용자 답을 기다리면 조용해진다.
 * 그래서 사용자가 Enter를 누른 뒤 출력이 한동안 이어지다가 멈추면 "끝남"으로 본다.
 * 짧게 끝난 응답(몇 초)은 알리지 않는다 — 기다리는 사이 다른 일을 하지 않았을 가능성이 크다.
 */

const ACTIVITY = {
    // 출력이 이만큼 멈추면 일을 마친 것으로 본다(ms). 스피너는 이보다 자주 화면을 고친다.
    QUIET_MS: 3000,
    // 이만큼 넘게 일한 경우만 알린다(ms).
    MIN_BUSY_MS: 5000,
};

// 에이전트로 알아보는 프로그램 이름. 실행 파일 이름이나 스크립트 경로의 한 부분과 맞춘다.
const AGENT_NAMES = ['claude', 'codex', 'gemini', 'opencode', 'cursor-agent', 'aider', 'copilot', 'amp', 'droid', 'kimi', 'qwen'];

// 스크립트를 실행하는 런타임·셸. 이 경우 실제 프로그램 이름은 스크립트 쪽에서 찾는다(`#!/bin/bash` 스크립트는 명령줄이 `/bin/bash 스크립트`다).
// 스크립트 없이 셸만 떠 있으면 셸 이름이다.
const RUNTIMES = ['node', 'bun', 'deno', 'python', 'python3', 'bash', 'sh', 'zsh'];

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
        const script = RUNTIMES.includes(executable) ? argv.slice(1).find(arg => arg && !arg.startsWith('-')) : null;
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

/** 프로그램 출력. Enter 뒤의 출력만 센다(시작 화면·프롬프트 다시 그리기는 세지 않는다). */
function noteOutput(
    activity,
    now,
) {
    return activity.armedAt === null ? activity : { ...activity, busySince: activity.busySince ?? now, lastOutput: now };
}

/**
 * 터미널 입력·출력으로 관찰한 에이전트 상태. 실행 중인 에이전트가 아니면 null이다.
 *
 * Enter 뒤 첫 출력을 기다리면 waiting, 최근 출력이 있으면 working, 그 밖에는 idle이다.
 * 출력이 멎어도 내부 계산이나 사용자 답을 기다리는지는 알 수 없으므로 완료 여부는 확정하지 않는다.
 * 완료 알림의 최소 작업 시간과 달리 짧은 응답도 같은 표시 규칙을 쓴다.
 */
function activityState(
    activity,
    now,
    agentNow,
) {
    let state = null;
    if (agentNow) {
        state = 'idle';
        if (activity.armedAt !== null && activity.busySince === null) {
            state = 'waiting';
        } else if (activity.lastOutput !== null && now - activity.lastOutput < ACTIVITY.QUIET_MS) {
            state = 'working';
        }
    }
    return state;
}

/**
 * 주기적으로 불러 끝났는지 본다. `agentNow`는 지금 이 터미널의 프로그램이 에이전트인지다.
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
    idle,
    noteInput,
    noteOutput,
    activityState,
    settle,
};
