// 앱이 터미널 프로세스째 사라진 뒤 그 터미널 탭을 어떻게 다시 여는지 정하는 순수 함수.
// 저장해 둔 기록(화면·셸·폴더·에이전트 대화)에서 새 셸의 실행 옵션, 새 셸에 입력할 이어하기 명령, 화면에 보일 안내 문구를 만든다.
const { refreshAccountResume, quotePosix } = require('./account-launch');
const { isWslShell } = require('./wsl-terminals');

class TerminalRestore {
    /** 에이전트별 이어하기 명령. 대화 식별자는 UUID로 검증한 값만 들어온다. */
    static COMMANDS = Object.freeze({
        claude: id => `claude --resume ${id}`,
        codex: id => `codex resume ${id}`,
    });
    /** 셸 시작 인자에서 시작 폴더를 지정하는 wsl.exe 옵션. 복원할 폴더는 터미널의 시작 폴더 옵션으로 넘기므로 이 쌍은 걷어 낸다. */
    static WSL_CD = '--cd';
    /** 환경 변수 앞붙임 문법(`NAME=값 명령`)을 쓸 수 없는 셸 실행 파일 이름. */
    static NON_POSIX_SHELLS = Object.freeze(['powershell', 'powershell.exe', 'pwsh', 'pwsh.exe', 'cmd', 'cmd.exe']);
    /** 새 셸이 프롬프트를 낸 것으로 보는 출력 멎음 시간(ms). */
    static PROMPT_QUIET_MS = 500;
    /** 프롬프트를 기다리는 최대 시간(ms). */
    static PROMPT_TIMEOUT_MS = 8000;
    /** 대화를 못 찾았을 때 사용자에게 알려 줄 직접 이어하기 명령. */
    static MANUAL = Object.freeze({ claude: 'claude --resume', codex: 'codex resume' });
}

/**
 * wsl.exe 시작 인자에서 `--cd <값>`(또는 `--cd=<값>`)을 걷어 낸다. Windows 홈에서 연 WSL 터미널은 `--cd ~`로 시작하는데,
 * 이것이 남으면 wsl.exe가 시작 폴더 옵션 대신 Linux 홈에서 셸을 시작해 복원할 폴더가 무시된다.
 *
 * Examples
 * --------
 * | 입력                              | 결과                 |
 * |-----------------------------------|----------------------|
 * | `['--cd', '~', '-d', 'Ubuntu']`   | `['-d', 'Ubuntu']`   |
 * | `['-d', 'Ubuntu', '--cd=/tmp']`   | `['-d', 'Ubuntu']`   |
 * | `['-d', 'Ubuntu']`                | 그대로               |
 */
function withoutWslCd(
    args,
) {
    const kept = [];
    for (let index = 0; index < args.length; index += 1) {
        if (args[index] === TerminalRestore.WSL_CD) {
            index += 1;
        } else if (typeof args[index] !== 'string' || !args[index].startsWith(`${TerminalRestore.WSL_CD}=`)) {
            kept.push(args[index]);
        }
    }
    return kept;
}

/** 이어하기 명령을 입력할 셸이 `NAME=값 명령` 문법을 쓰는 POSIX 계열(bash·zsh·fish·sh·WSL)인지. */
function isPosixShell(
    shellPath,
) {
    return !TerminalRestore.NON_POSIX_SHELLS.includes(String(shellPath || '').split(/[\\/]/).pop().toLowerCase());
}

/** 폴더 경로를 셸 시작 폴더(file URI 문자열)로 바꾸는 함수를 받아, 기록의 현재 폴더가 있으면 그 URI를 돌려준다. 없으면 undefined다. */
function startFolder(
    record,
    toUri,
) {
    return typeof record.cwdNow === 'string' && record.cwdNow ? toUri(record.cwdNow) : undefined;
}

/**
 * 저장 기록으로 새 터미널의 실행 옵션과 이어하기를 정한다.
 *
 * - 계정 터미널(`options.paddockAccount`가 있는 것, 계정 메뉴로 연 터미널)이고 Claude 대화가 있으면 계정 시작 옵션에 `--resume <id>`를 넣는다.
 *   시작 폴더는 처음 폴더를 유지한다(WSL 계정 터미널은 시작 폴더가 시작 인자에 들어 있다).
 * - 그 밖의 터미널은 셸 옵션이 없으면 기록의 셸·인자·환경으로 채우고, 시작 폴더를 기록의 현재 폴더로 바꾼다.
 *   대화가 있으면 새 셸이 뜬 뒤 입력할 명령(`command`)을 돌려준다 — 사용자 셸의 PATH·별칭을 그대로 쓰도록 직접 입력한다.
 * - 에이전트가 떠 있었는데 대화를 확신할 수 없으면 이어하지 않고 안내만 한다.
 *
 * `notice`는 복원된 화면 아래에 보일 한 줄 안내다. `toUri(경로)`는 폴더 경로를 file URI 문자열로 바꾸는 함수다.
 *
 * Examples
 * --------
 * | 기록의 agent                    | 터미널 종류 | 결과                                               |
 * |---------------------------------|-------------|----------------------------------------------------|
 * | claude, 대화 있음               | 일반        | `command: 'claude --resume <id>'`                  |
 * | claude, 대화 있음               | 계정        | 계정 옵션 인자에 `--resume <id>`, `command: null`  |
 * | codex, 대화 있음                | 일반        | `command: 'codex resume <id>'`                     |
 * | codex, 대화 있음                | 계정        | 이어하지 않음, 안내에 `codex resume`               |
 * | null, 앞쪽 프로그램이 codex     | 일반        | 이어하지 않음, 안내에 `codex resume`               |
 * | null, 셸만                      | 일반        | 이어하지 않음, 안내는 화면 복원만                  |
 */
function planRestore(
    {
        options,
        record,
        toUri,
    },
) {
    const agent = record.agent && TerminalRestore.COMMANDS[record.agent.provider] ? record.agent : null;
    const isAccount = Boolean(options.paddockAccount);
    let planned = { ...options };
    let command = null;
    let resumed = false;
    if (isAccount) {
        if (agent && agent.provider === 'claude') {
            const profile = options.paddockAccount;
            planned = refreshAccountResume(options, {
                provider: 'claude', sessionId: agent.sessionId, runtime: profile.runtime, wslDistribution: profile.wslDistribution, useSessionId: true,
            });
            resumed = true;
        }
    } else {
        if (typeof options.shellPath !== 'string' || !options.shellPath) {
            const launcher = record.launcher || {};
            if (typeof launcher.command === 'string' && launcher.command) {
                planned.shellPath = launcher.command;
                planned.shellArgs = Array.isArray(launcher.args) ? launcher.args : [];
                planned.env = { ...launcher.env, ...options.env };
            }
        }
        const folder = startFolder(record, toUri);
        if (folder) {
            planned.cwd = folder;
            if (isWslShell(planned.shellPath) && Array.isArray(planned.shellArgs)) planned.shellArgs = withoutWslCd(planned.shellArgs);
        }
        if (agent) {
            command = TerminalRestore.COMMANDS[agent.provider](agent.sessionId);
            // 대화를 찾은 계정 폴더로 이어 열어야 같은 대화를 찾는다. 환경 변수 앞붙임을 쓸 수 있는 셸에서만 붙인다.
            if (agent.provider === 'claude' && agent.configDir && isPosixShell(planned.shellPath)) command = `CLAUDE_CONFIG_DIR=${quotePosix(agent.configDir)} ${command}`;
            resumed = true;
        }
    }
    let notice = 'Previous shell ended. Screen restored; commands that were running were stopped.';
    if (resumed) {
        notice += ` Continuing the ${agent.provider === 'claude' ? 'Claude' : 'Codex'} conversation.`;
    } else if (record.foreground && TerminalRestore.MANUAL[record.foreground]) {
        notice += ` ${record.foreground === 'claude' ? 'Claude' : 'Codex'} was running but its conversation could not be identified; run \`${TerminalRestore.MANUAL[record.foreground]}\` to choose one.`;
    }
    return { options: planned, command, notice };
}

/** 복원된 화면 아래에 쓰는 흐린 한 줄. 화면 출력이 줄 중간에서 끝났을 수 있어 줄을 바꿔 시작한다. */
function noticeLine(
    notice,
) {
    return `\x1b[0m\r\n\x1b[2m— ${notice} —\x1b[0m\r\n`;
}

/**
 * 새 셸이 프롬프트를 내고 입력을 받을 준비가 됐는지 기다린다: 출력이 한 번이라도 나온 뒤 `PROMPT_QUIET_MS` 동안 멎고 셸 아래에서 도는 프로그램이 없을 때 `'ready'`.
 * 시작 스크립트가 tmux·fzf 같은 프로그램을 띄워 두었으면(`hasChildren()`이 true) `'busy'`, 시간 안에 조용해지지 않으면 `'timeout'`이다.
 * 둘 다 이어하기 명령을 입력하지 않는다 — 입력이 엉뚱한 프로그램으로 들어가는 것보다 직접 입력하게 두는 편이 낫다.
 *
 * `clock`은 `{ now(), sleep(ms) }`, `lastWriteAt()`은 마지막 출력 시각(ms, 출력이 없으면 null)이다.
 *
 * Examples
 * --------
 * | 상황                                         | 결과        |
 * |----------------------------------------------|-------------|
 * | 출력 뒤 0.5초 조용, 자식 프로세스 없음       | `'ready'`   |
 * | 출력 뒤 조용하지만 자식 프로세스(tmux 등)    | `'busy'`    |
 * | 8초 동안 계속 출력                           | `'timeout'` |
 */
async function waitForPrompt(
    {
        lastWriteAt,
        hasChildren,
        clock,
    },
) {
    const startedAt = clock.now();
    let outcome = 'timeout';
    while (outcome === 'timeout' && clock.now() - startedAt < TerminalRestore.PROMPT_TIMEOUT_MS) {
        const last = lastWriteAt();
        if (last !== null && clock.now() - last >= TerminalRestore.PROMPT_QUIET_MS) {
            outcome = (await hasChildren()) ? 'busy' : 'ready';
        } else {
            await clock.sleep(100);
        }
    }
    return outcome;
}

module.exports = { TerminalRestore, planRestore, noticeLine, waitForPrompt, withoutWslCd };
