/**
 * Paddock 소유 Codex 계정 폴더(`CODEX_HOME`)의 `hooks.json`에 에이전트 상태 훅을 넣거나 뺀다.
 *
 * Codex는 Claude Code와 달리 터미널 상태 신호(OSC 7501)를 보내지 않으므로, 같은 상태 훅 스크립트(agent-status-hook.sh)를
 * Codex 훅으로 걸어 Work 목록에 작업 중·입력 필요·끝남을 보인다. 훅은 `PADDOCK_STATUS_APP=codex`로 부른다.
 *
 * 일반 `~/.codex`는 건드리지 않는다. 계정 폴더의 `hooks.json`이 일반 `~/.codex/hooks.json`을 가리키는 링크이면(계정 간 공유)
 * 그 링크를 따라 쓰면 사용자의 파일이 바뀌므로 쓰지 않고 'shared'로 알린다.
 * Codex는 비관리 훅을 `/hooks`에서 신뢰해야 돌리고 내용이 바뀌면 다시 검토를 요구한다. 파일이 새로 쓰이거나 바뀐 경우 호출부가 안내를 띄운다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { installAgentStatusHook, removeAgentStatusHook, hasAgentStatusHook } = require('./agent-status-hook-settings');

class CodexHooks {
    static FILE = 'hooks.json';
    // Codex가 부르는 훅 이벤트 중 상태 신호에 쓰는 것. Claude의 이벤트 이름과 같고 Interrupt(사용자 중단)가 더해진다.
    static EVENTS = Object.freeze(['SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PermissionRequest', 'PostToolUse', 'Stop', 'Interrupt']);
    static TRUST_NOTICE = 'Codex asks you to trust Paddock\'s status hook the first time: run /hooks in the Codex terminal and trust it.';
}

/**
 * 계정 폴더의 `hooks.json`을 상태 훅 켜짐 여부(`command` 유무)에 맞춘다. 결과 상태를 돌려준다.
 *
 * | 결과        | 뜻                                                                          |
 * | ----------- | --------------------------------------------------------------------------- |
 * | `written`   | 훅을 넣었다(파일이 새로 생겼거나 내용이 바뀜). 신뢰 안내가 필요하다           |
 * | `removed`   | 훅을 뺐다(다른 내용이 없으면 파일도 지웠다)                                  |
 * | `unchanged` | 이미 원하는 모양이다                                                         |
 * | `shared`    | 파일이 다른 곳을 가리키는 링크라 쓰지 않았다                                  |
 * | `invalid`   | 파일이 JSON 객체가 아니라 사용자의 내용을 지키려고 쓰지 않았다                |
 *
 * `command`가 null이면 끈 것이다. 켠 적 없는 폴더를 끌 때는 아무 파일도 만들지 않는다.
 */
function syncCodexStatusHook(
    codexHome,
    command,
) {
    const file = path.join(codexHome, CodexHooks.FILE);
    let info = null;
    try {
        info = fs.lstatSync(file);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    let result = 'unchanged';
    if (info && (info.isSymbolicLink() || !info.isFile())) {
        result = 'shared';
    } else {
        let settings = {};
        if (info) {
            try {
                const parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
                if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) settings = parsed;
                else result = 'invalid';
            } catch {
                result = 'invalid';
            }
        }
        if (result !== 'invalid') {
            const next = command ? installAgentStatusHook(settings, command, CodexHooks.EVENTS) : removeAgentStatusHook(settings);
            const text = `${JSON.stringify(next, null, 2)}\n`;
            if (command) {
                if (!info || fs.readFileSync(file, 'utf8') !== text) {
                    writeAtomically(file, text);
                    result = 'written';
                }
            } else if (info && hasAgentStatusHook(settings)) {
                if (Object.keys(next).length === 0) fs.rmSync(file, { force: true });
                else writeAtomically(file, text);
                result = 'removed';
            }
        }
    }
    return result;
}

/**
 * 계정 폴더에 Paddock이 새로 쓴 `hooks.json`(내용이 상태 훅뿐)이 있는데 일반 `~/.codex/hooks.json`(`hostFile`)이 생겼으면 그 파일을 지운다.
 * 그래야 계정 준비의 공유 링크 단계가 그 자리에 일반 파일로 가는 링크를 만들어 계정이 사용자의 훅을 함께 받는다.
 * 사용자 내용이 섞인 파일·링크·호스트 파일이 없는 경우는 건드리지 않는다. 지웠으면 true다.
 */
function releaseOwnCodexHooks(
    codexHome,
    hostFile,
) {
    const file = path.join(codexHome, CodexHooks.FILE);
    let released = false;
    let hostInfo = null;
    let info = null;
    try {
        hostInfo = fs.statSync(hostFile);
        info = fs.lstatSync(file);
    } catch (error) {
        if (!['ENOENT', 'ENOTDIR'].includes(error.code)) throw error;
    }
    if (hostInfo?.isFile() && info?.isFile()) {
        let settings = null;
        try {
            settings = JSON.parse(fs.readFileSync(file, 'utf8'));
        } catch {
            settings = null;
        }
        if (settings && typeof settings === 'object' && hasAgentStatusHook(settings) && Object.keys(removeAgentStatusHook(settings)).length === 0) {
            fs.rmSync(file, { force: true });
            released = true;
        }
    }
    return released;
}

/** 파일을 임시 파일과 이름 바꾸기로 쓴다(0600). */
function writeAtomically(
    file,
    text,
) {
    fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
    const temporary = `${file}.${randomUUID()}.tmp`;
    try {
        fs.writeFileSync(temporary, text, { mode: 0o600, flag: 'wx' });
        fs.renameSync(temporary, file);
    } finally {
        fs.rmSync(temporary, { force: true });
    }
}

module.exports = { CodexHooks, syncCodexStatusHook, releaseOwnCodexHooks };
