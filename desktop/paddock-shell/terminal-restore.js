// 앱이 터미널 프로세스째 사라진 뒤 그 터미널 탭을 어떻게 다시 여는지 정하는 순수 함수.
// 저장해 둔 기록(화면·셸·폴더·에이전트 대화)에서 새 셸의 실행 옵션, 새 셸에 입력할 이어하기 명령, 화면에 보일 안내 문구를 만든다.
const { refreshAccountResume } = require('./account-launch');

class TerminalRestore {
    /** 에이전트별 이어하기 명령. 대화 식별자는 UUID로 검증한 값만 들어온다. */
    static COMMANDS = Object.freeze({
        claude: id => `claude --resume ${id}`,
        codex: id => `codex resume ${id}`,
    });
    /** 대화를 못 찾았을 때 사용자에게 알려 줄 직접 이어하기 명령. */
    static MANUAL = Object.freeze({ claude: 'claude --resume', codex: 'codex resume' });
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
        if (folder) planned.cwd = folder;
        if (agent) {
            command = TerminalRestore.COMMANDS[agent.provider](agent.sessionId);
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

module.exports = { TerminalRestore, planRestore, noticeLine };
