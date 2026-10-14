// 터미널 셸 프로세스가 지금 있는 폴더와 앞쪽에서 실행 중인 프로그램을 운영체제에서 읽는다.
// 백엔드가 터미널 화면을 파일에 남길 때 "어느 폴더에서 어느 에이전트 대화를 열고 있었나"를 함께 적는 데 쓴다.
const fs = require('node:fs');
const os = require('node:os');
const { spawnSync } = require('node:child_process');
const { parseEnviron } = require('./agent-account');
const agentModel = require('./agent-model');
const { AgentResume, resumableAgent, elapsedSeconds } = require('./agent-resume');

/**
 * 셸 프로세스의 지금 앞쪽 프로그램(터미널에서 실행 중인 명령)의 `{ pid, argv, env }`. 셸만 떠 있으면 셸 자신이다.
 * env는 그 프로세스의 CODEX_HOME·CLAUDE_CONFIG_DIR·HOME만 담고, 읽을 수 없으면 null(모름)이다.
 * Linux는 /proc, macOS는 ps로 읽는다(macOS는 env를 읽지 않는다). 명령줄을 읽을 수 없으면 null.
 * Windows는 WindowsProcessList와 windows-processes.js가 맡는다.
 */
function foregroundProcess(
    shellPid,
) {
    let result = null;
    try {
        if (process.platform === 'linux') {
            // stat의 8번째 칸이 터미널 앞쪽 프로세스 그룹. 프로그램 이름에 공백·괄호가 있을 수 있어 마지막 ')' 뒤부터 센다.
            const stat = fs.readFileSync(`/proc/${shellPid}/stat`, 'utf8');
            const foreground = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[5]);
            const pid = foreground > 0 ? foreground : shellPid;
            const argv = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean);
            let env = null;
            try {
                env = parseEnviron(fs.readFileSync(`/proc/${pid}/environ`, 'utf8'));
            } catch {
                // 다른 사용자로 실행된 프로그램(sudo 등)은 환경을 읽을 수 없다. 계정은 모름으로 둔다.
                env = null;
            }
            result = { pid, argv, env };
        } else if (process.platform === 'darwin') {
            const group = spawnSync('ps', ['-o', 'tpgid=', '-p', String(shellPid)], { encoding: 'utf8' }).stdout.trim();
            const pid = Number(group) > 0 ? group : String(shellPid);
            const args = spawnSync('ps', ['-o', 'args=', '-p', pid], { encoding: 'utf8' }).stdout.trim();
            result = args ? { pid: Number(pid), argv: args.split(/\s+/), env: null } : null;
        }
    } catch {
        result = null;
    }
    return result;
}

/** 프로세스가 시작한 시각(ms). `ps`의 경과 시간으로 구하며, 읽을 수 없으면 null이다. 초 단위로만 알 수 있다. */
function processStartedAt(
    pid,
    now,
) {
    let result = null;
    try {
        const seconds = elapsedSeconds(spawnSync('ps', ['-o', 'etime=', '-p', String(pid)], { encoding: 'utf8' }).stdout);
        if (seconds !== null) result = now - seconds * 1000;
    } catch { /* ps를 실행할 수 없으면 시작 시각을 모른다. */ }
    return result;
}

/** 프로세스의 현재 폴더. Linux는 /proc, macOS는 lsof로 읽고 그 밖의 운영체제나 읽기 실패는 빈 문자열이다. */
function currentFolder(
    pid,
) {
    let result = '';
    try {
        if (process.platform === 'linux') {
            result = fs.readlinkSync(`/proc/${pid}/cwd`);
        } else if (process.platform === 'darwin') {
            const lines = spawnSync('lsof', ['-a', '-d', 'cwd', '-p', String(pid), '-Fn'], { encoding: 'utf8' }).stdout.split('\n');
            result = lines.find(line => line.startsWith('n'))?.slice(1) || '';
        }
    } catch { /* 읽을 수 없으면 모름으로 둔다. */ }
    return result;
}

/** 첫 줄만 읽는다. rollout 파일은 수 MB가 될 수 있어 앞부분만 연다. */
function readFirstLine(
    file,
    maxBytes,
) {
    const handle = fs.openSync(file, 'r');
    let text = '';
    try {
        const buffer = Buffer.alloc(maxBytes);
        const n_read = fs.readSync(handle, buffer, 0, maxBytes, 0);
        text = buffer.subarray(0, n_read).toString('utf8').split('\n')[0];
    } finally {
        fs.closeSync(handle);
    }
    return text;
}

/**
 * 터미널 프로세스의 현재 폴더와 이어 열 수 있는 에이전트 대화를 읽는 함수(`inspect`)와, Windows의 WSL 터미널 정보를 받는 함수(`noteWsl`)를 만든다.
 *
 * `inspect(terminalProcess)`는 `{ cwd, agent, foreground }`를 돌려준다. agent는 `{ provider, sessionId, cwd }`(이어 열 수 있는 대화)이거나 null이고,
 * foreground는 앞쪽에서 실행 중인 에이전트 이름(claude·codex 등)이거나 에이전트가 없으면 null이다. 에이전트가 있는데 agent가 null이면 대화를 확신하지 못한 것이다.
 * - WSL 터미널(실행 환경에 PADDOCK_TERMINAL 표지가 있고 `noteWsl`로 받은 정보가 있는 것)은 그 정보를 쓴다.
 * - Linux·macOS 셸은 운영체제에서 직접 읽는다.
 * - 그 밖(Windows 네이티브 셸)은 null이다 — 현재 폴더는 셸이 알린 값(OSC 7)이나 처음 폴더로 대신한다.
 */
function createTerminalInspector(
    {
        homeDirectory = os.homedir(),
        now = Date.now,
    } = {},
) {
    // 터미널 표지 id → { cwd, program, sessionId }. 최근 WSL 스캔 결과다.
    let wslTerminals = {};
    const deps = { homeDirectory, readFile: file => fs.readFileSync(file, 'utf8'), fileSystem: { readdir: directory => fs.readdirSync(directory), readFirstLine } };

    const inspect = (terminalProcess) => {
        let result = null;
        const marker = terminalProcess.options?.options?.env?.PADDOCK_TERMINAL;
        if (marker && wslTerminals[marker]) {
            const { cwd, program, sessionId } = wslTerminals[marker];
            const agent = program === 'claude' && AgentResume.UUID.test(sessionId || '') ? { provider: 'claude', sessionId, cwd } : null;
            result = { cwd, agent, foreground: agentModel.isAgent(program) ? program : null };
        } else if (process.platform === 'linux' || process.platform === 'darwin') {
            let shellPid = null;
            try {
                shellPid = terminalProcess.pid;
            } catch { /* 셸이 시작되지 못한 터미널은 pid가 없다. */ }
            if (Number.isSafeInteger(shellPid)) {
                const cwd = currentFolder(shellPid);
                const foreground = foregroundProcess(shellPid);
                const program = foreground ? agentModel.programName(foreground.argv) : null;
                let startedAt = null;
                if (program === 'codex') startedAt = processStartedAt(foreground.pid, now());
                const agent = foreground ? resumableAgent({ program, pid: foreground.pid, env: foreground.env, cwd, startedAt }, deps) : null;
                result = { cwd, agent, foreground: agentModel.isAgent(program) ? program : null };
            }
        }
        return result;
    };

    const noteWsl = (terminals) => {
        wslTerminals = terminals && typeof terminals === 'object' ? terminals : {};
    };

    return { inspect, noteWsl };
}

module.exports = { foregroundProcess, createTerminalInspector, currentFolder };
