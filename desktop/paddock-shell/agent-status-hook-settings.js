/**
 * "에이전트 상태 훅"(agent-status-hook.sh)의 명령·설정 항목·스크립트 복사를 다룬다.
 *
 * 이 훅은 에이전트(Claude Code)가 훅 이벤트마다 부르는 스크립트로, 이벤트를 터미널 상태 신호(OSC 7501)로 바꿔 터미널에 쓴다.
 * 에이전트가 신호를 스스로 보내지 못하는 터미널(Windows의 WSL 터미널)에서도 Paddock이 작업 중·입력 필요·끝남을 알게 하려는 것이다.
 *
 * 메모리 안내 훅(memory-hook-settings.js)과 같은 틀이다. 훅은 사용자 설정 파일이 아니라 Paddock 전용 플래그 설정 파일(claude-flag-settings.js)에 들어가고,
 * 이 모듈이 넣은 항목은 명령에 `agent-status-hook.sh`가 들어 있는 것으로 알아본다. 사용자가 직접 만든 훅과 구분하는 기준이다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { assertPlainPath } = require('./usage-files');
const { shellQuote } = require('./memory-hook-settings');

class AgentStatusHook {
    static SCRIPT_NAME = 'agent-status-hook.sh';
    // 스크립트와 같은 폴더에 두면 스크립트가 아무것도 하지 않는다(agent-status-hook.sh가 확인한다).
    static OFF_MARKER = 'agent-status-hook.off';
    // 신호 본문의 app 칸 값. 스크립트가 같은 목록을 한 번 더 거른다.
    static APPS = Object.freeze({ CLAUDE: 'claude-code', CODEX: 'codex' });
    // 훅이 부를 이벤트(Claude Code 이름). Codex는 같은 이름을 쓰며 Interrupt가 더해진다(codex-hooks.js).
    static CLAUDE_EVENTS = Object.freeze(['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PermissionRequest', 'Notification', 'Elicitation', 'Stop', 'StopFailure', 'SessionEnd']);
    // 상태 표시가 에이전트를 오래 붙잡지 않게 훅 한 번에 주는 시간(초).
    static TIMEOUT_SECONDS = 5;
}

/**
 * 훅 명령 문자열. 신호의 app 칸은 환경 변수로 넘겨 스크립트 파일 하나를 Claude Code와 Codex가 함께 쓴다.
 *
 * Examples
 * --------
 * | script                         | app         | 결과                                                         |
 * | ------------------------------ | ----------- | ------------------------------------------------------------ |
 * | /home/me/u/agent-status-hook.sh | claude-code | `PADDOCK_STATUS_APP=claude-code sh '/home/me/u/agent-status-hook.sh'` |
 */
function agentStatusHookCommand(
    script,
    app = AgentStatusHook.APPS.CLAUDE,
) {
    const name = Object.values(AgentStatusHook.APPS).includes(app) ? app : AgentStatusHook.APPS.CLAUDE;
    return `PADDOCK_STATUS_APP=${name} sh ${shellQuote(script)}`;
}

/** 이 모듈이 넣은 훅 명령인지. */
function isAgentStatusHookCommand(
    command,
) {
    return typeof command === 'string' && command.includes(AgentStatusHook.SCRIPT_NAME);
}

/** 이벤트 항목 목록에서 이 모듈의 훅만 뺀 새 목록. 훅이 비게 된 항목은 버리고, 사용자의 다른 훅은 그대로 둔다. */
function withoutAgentStatusHook(
    entries,
) {
    const kept = [];
    for (const entry of entries) {
        if (entry && Array.isArray(entry.hooks)) {
            const hooks = entry.hooks.filter(hook => !isAgentStatusHookCommand(hook?.command));
            if (hooks.length === entry.hooks.length) kept.push(entry);
            else if (hooks.length) kept.push({ ...entry, hooks });
        } else {
            kept.push(entry);
        }
    }
    return kept;
}

/**
 * 설정의 모든 대상 이벤트에 훅을 넣은 새 설정. 이미 있으면 같은 명령으로 바꾸므로 여러 번 불러도 이벤트마다 한 번만 들어간다.
 * 입력 설정은 바꾸지 않는다. 다른 훅(메모리 안내 훅 포함)은 그대로 둔다.
 */
function installAgentStatusHook(
    settings,
    command,
    events = AgentStatusHook.CLAUDE_EVENTS,
) {
    const hooks = { ...(settings.hooks || {}) };
    for (const event of events) {
        const existing = Array.isArray(hooks[event]) ? hooks[event] : [];
        hooks[event] = [...withoutAgentStatusHook(existing), { hooks: [{ type: 'command', command, timeout: AgentStatusHook.TIMEOUT_SECONDS }] }];
    }
    return { ...settings, hooks };
}

/**
 * 설정에서 이 모듈의 훅을 뺀 새 설정. 빼고 나서 비게 된 이벤트·hooks는 지워 설정이 원래 모양으로 돌아가게 한다.
 * 넣은 훅이 없으면 받은 설정 객체를 그대로 돌려준다(같은 참조라 호출부가 변경 여부를 알 수 있다).
 */
function removeAgentStatusHook(
    settings,
) {
    let result = settings;
    if (settings.hooks && typeof settings.hooks === 'object') {
        const hooks = {};
        let isChanged = false;
        for (const [event, entries] of Object.entries(settings.hooks)) {
            if (Array.isArray(entries)) {
                const kept = withoutAgentStatusHook(entries);
                if (kept.length !== entries.length || kept.some((entry, index) => entry !== entries[index])) isChanged = true;
                if (kept.length) hooks[event] = kept;
            } else {
                hooks[event] = entries;
            }
        }
        if (isChanged) {
            const { hooks: previous, ...rest } = settings;
            result = Object.keys(hooks).length ? { ...rest, hooks } : rest;
        }
    }
    return result;
}

/** 설정에 이 모듈의 훅이 하나라도 들어 있는지. */
function hasAgentStatusHook(
    settings,
) {
    return Object.values(settings.hooks || {}).some(entries => Array.isArray(entries)
        && entries.some(entry => Array.isArray(entry?.hooks) && entry.hooks.some(hook => isAgentStatusHookCommand(hook?.command))));
}

/**
 * 스크립트 복사와 꺼짐 표시 파일을 맞추고 결과 상태('on'|'off')를 돌려준다.
 * 설정(훅 항목)은 플래그 설정 파일을 다시 쓰는 쪽이 맡는다. 훅 항목이 사라져도 이미 떠 있는 세션이 스크립트를 계속 부를 수 있어
 * 끌 때는 꺼짐 표시 파일을 먼저 둔다. 켠 적 없는 곳(스크립트가 없는 곳)을 끌 때는 아무 파일도 만들지 않는다.
 *
 * Examples
 * --------
 * | enabled | scriptFile 있음 | 하는 일                                 |
 * | ------- | --------------- | --------------------------------------- |
 * | true    | 상관없음        | 스크립트 복사 → 꺼짐 표시 삭제          |
 * | false   | 있음            | 꺼짐 표시 생성                          |
 * | false   | 없음            | 아무 파일도 만들지 않는다               |
 */
function setAgentStatusHook(
    { scriptFile, source, enabled },
) {
    const offMarker = path.join(path.dirname(scriptFile), AgentStatusHook.OFF_MARKER);
    for (const file of [scriptFile, offMarker]) assertPlainPath(file, 'file');
    assertPlainPath(path.dirname(scriptFile), 'directory');
    if (enabled) {
        fs.mkdirSync(path.dirname(scriptFile), { recursive: true, mode: 0o700 });
        fs.copyFileSync(source, scriptFile);
        fs.rmSync(offMarker, { force: true });
    } else if (fs.existsSync(scriptFile)) {
        fs.writeFileSync(offMarker, '');
    }
    return enabled ? 'on' : 'off';
}

module.exports = {
    AgentStatusHook,
    agentStatusHookCommand,
    isAgentStatusHookCommand,
    installAgentStatusHook,
    removeAgentStatusHook,
    hasAgentStatusHook,
    setAgentStatusHook,
};
