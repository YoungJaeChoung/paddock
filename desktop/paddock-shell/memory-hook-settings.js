/**
 * Claude Code 설정 파일에 "메모리가 부족할 때 에이전트에게 알리는 훅"을 넣거나 뺀다.
 *
 * 사용자가 직접 만든 훅은 건드리지 않는다. 이 모듈이 넣은 항목은 명령에 `memory-hook.sh`가 들어 있는 것으로 알아본다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { assertPlainPath, usageError } = require('./usage-files');
const { writeJson } = require('./claude-usage-settings');

class MemoryHook {
    // 에이전트가 메모리를 쓰는 도구에만 반응한다(memory-hook.sh가 같은 목록을 한 번 더 거른다).
    static MATCHER = 'Bash|Agent|Task';
    static SCRIPT_NAME = 'memory-hook.sh';
    // 스크립트와 같은 폴더에 두면 스크립트가 아무것도 하지 않는다(memory-hook.sh가 확인한다).
    static OFF_MARKER = 'memory-hook.off';
    static DEFAULT_MINIMUM_MB = 4096;
}

/** 값을 POSIX 셸 한 단어로 감싼다. 작은따옴표는 닫고 이스케이프한 뒤 다시 연다. */
function shellQuote(
    value,
) {
    return `'${String(value).replace(/'/g, "'\\''")}'`;
}

/**
 * 훅 명령 문자열. 기준값은 환경 변수로 넘겨 스크립트 파일을 바꾸지 않고도 설정으로 달라진다.
 *
 * Examples
 * --------
 * | script                    | minimumMb | 결과                                                       |
 * | ------------------------- | --------- | ---------------------------------------------------------- |
 * | /home/me/u/memory-hook.sh | 4096      | `PADDOCK_MEMORY_MIN_MB=4096 sh '/home/me/u/memory-hook.sh'` |
 */
function memoryHookCommand(
    script,
    minimumMb = MemoryHook.DEFAULT_MINIMUM_MB,
) {
    const megabytes = Number.isFinite(Number(minimumMb)) && Number(minimumMb) >= 0 ? Math.floor(Number(minimumMb)) : MemoryHook.DEFAULT_MINIMUM_MB;
    return `PADDOCK_MEMORY_MIN_MB=${megabytes} sh ${shellQuote(script)}`;
}

/** 이 모듈이 넣은 훅 명령인지. */
function isMemoryHookCommand(
    command,
) {
    return typeof command === 'string' && command.includes(MemoryHook.SCRIPT_NAME);
}

/** PreToolUse 항목 목록에서 이 모듈의 훅만 뺀 새 목록. 훅이 비게 된 항목은 버리고, 사용자의 다른 훅은 그대로 둔다. */
function withoutMemoryHook(
    entries,
) {
    const kept = [];
    for (const entry of entries) {
        if (entry && Array.isArray(entry.hooks)) {
            const hooks = entry.hooks.filter(hook => !isMemoryHookCommand(hook?.command));
            if (hooks.length === entry.hooks.length) kept.push(entry);
            else if (hooks.length) kept.push({ ...entry, hooks });
        } else {
            kept.push(entry);
        }
    }
    return kept;
}

/**
 * 설정에 훅을 넣은 새 설정. 이미 있으면 같은 명령으로 바꾸므로 여러 번 불러도 한 번만 들어간다.
 * 입력 설정은 바꾸지 않는다.
 */
function installMemoryHook(
    settings,
    command,
) {
    const existing = Array.isArray(settings.hooks?.PreToolUse) ? settings.hooks.PreToolUse : [];
    const entry = { matcher: MemoryHook.MATCHER, hooks: [{ type: 'command', command }] };
    return { ...settings, hooks: { ...(settings.hooks || {}), PreToolUse: [...withoutMemoryHook(existing), entry] } };
}

/**
 * 설정에서 이 모듈의 훅을 뺀 새 설정. 빼고 나서 비게 된 PreToolUse·hooks는 지워 설정이 원래 모양으로 돌아가게 한다.
 * 넣은 훅이 없으면 받은 설정 객체를 그대로 돌려준다(같은 참조라 호출부가 변경 여부를 알 수 있다).
 */
function removeMemoryHook(
    settings,
) {
    const existing = settings.hooks?.PreToolUse;
    let result = settings;
    if (Array.isArray(existing)) {
        const kept = withoutMemoryHook(existing);
        if (kept.length !== existing.length || kept.some((entry, index) => entry !== existing[index])) {
            const { PreToolUse, ...otherHooks } = settings.hooks;
            const hooks = kept.length ? { ...otherHooks, PreToolUse: kept } : otherHooks;
            const { hooks: previousHooks, ...rest } = settings;
            result = Object.keys(hooks).length ? { ...rest, hooks } : rest;
        }
    }
    return result;
}

/** 설정에 이 모듈의 훅이 들어 있는지. */
function hasMemoryHook(
    settings,
) {
    const entries = Array.isArray(settings.hooks?.PreToolUse) ? settings.hooks.PreToolUse : [];
    return entries.some(entry => Array.isArray(entry?.hooks) && entry.hooks.some(hook => isMemoryHookCommand(hook?.command)));
}

/**
 * Claude Code 설정 파일 하나에 훅을 넣거나 빼고 결과 상태('on'|'off')를 돌려준다.
 * 이미 원하는 상태면 파일을 다시 쓰지 않는다(사용자 파일의 서식이 바뀌고 백업이 생기는 것을 막는다).
 * 처음 바꿀 때 한 번만 원본을 `.paddock-backup`으로 남긴다. 설정 파일이 없고 끄려는 경우엔 아무것도 만들지 않는다.
 */
function applyMemoryHook(
    { settingsPath, command, enabled },
) {
    for (const file of [settingsPath, `${settingsPath}.paddock-backup`]) assertPlainPath(file, 'file');
    let settings = {};
    if (fs.existsSync(settingsPath)) {
        try {
            settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
        } catch {
            throw usageError('Claude settings contain invalid JSON. Repair the settings before changing the memory notice.');
        }
        if (!settings || typeof settings !== 'object' || Array.isArray(settings)) throw usageError('Claude settings must be a JSON object.');
    }
    const next = enabled ? installMemoryHook(settings, command) : removeMemoryHook(settings);
    const isUnchanged = enabled
        ? Array.isArray(settings.hooks?.PreToolUse) && JSON.stringify(settings.hooks.PreToolUse) === JSON.stringify(next.hooks.PreToolUse)
        : next === settings;
    if (!isUnchanged) {
        fs.mkdirSync(path.dirname(settingsPath), { recursive: true, mode: 0o700 });
        if (fs.existsSync(settingsPath) && !fs.existsSync(`${settingsPath}.paddock-backup`)) {
            fs.copyFileSync(settingsPath, `${settingsPath}.paddock-backup`, fs.constants.COPYFILE_EXCL);
        }
        writeJson(settingsPath, next);
    }
    return enabled ? 'on' : 'off';
}

/**
 * 한 Claude Code 설정 위치에 메모리 안내를 켜거나 끄고 결과 상태('on'|'off')를 돌려준다.
 *
 * 설정 화면의 켜기·끄기가 이 함수 하나로 이어지게 하려고 있다. 설정 파일 수정(applyMemoryHook)에 더해
 * 스크립트 복사와 꺼짐 표시 파일을 함께 맞춘다. 설정에서 훅을 지워도 이미 떠 있는 세션이 스크립트를 계속 부를 수 있어
 * 끌 때는 꺼짐 표시 파일을 먼저 둔다.
 *
 * `scriptFile`은 이 앱이 파일을 쓰는 경로, `commandScript`는 Claude Code가 훅을 실행할 때 보는 같은 파일의 경로다
 * (WSL 안의 Claude Code는 Windows 쪽 UNC 경로가 아니라 Linux 경로로 부른다).
 *
 * Examples
 * --------
 * | enabled | scriptFile 있음 | 하는 일                                                         |
 * | ------- | --------------- | --------------------------------------------------------------- |
 * | true    | 상관없음        | 스크립트 복사 → 꺼짐 표시 삭제 → 설정에 훅 한 줄 추가           |
 * | false   | 있음            | 꺼짐 표시 생성 → 설정에서 훅 한 줄 제거                         |
 * | false   | 없음            | 설정에서 훅 한 줄 제거(켠 적 없으면 아무 파일도 만들지 않는다)  |
 */
function setMemoryNotice(
    { settingsPath, scriptFile, commandScript, source, enabled, minimumMb },
) {
    const offMarker = path.join(path.dirname(scriptFile), MemoryHook.OFF_MARKER);
    for (const file of [scriptFile, offMarker]) assertPlainPath(file, 'file');
    assertPlainPath(path.dirname(scriptFile), 'directory');
    if (enabled) {
        fs.mkdirSync(path.dirname(scriptFile), { recursive: true, mode: 0o700 });
        fs.copyFileSync(source, scriptFile);
        fs.rmSync(offMarker, { force: true });
    } else if (fs.existsSync(scriptFile)) {
        fs.writeFileSync(offMarker, '');
    }
    return applyMemoryHook({ settingsPath, command: memoryHookCommand(commandScript, minimumMb), enabled });
}

module.exports = {
    MemoryHook,
    memoryHookCommand,
    isMemoryHookCommand,
    installMemoryHook,
    removeMemoryHook,
    hasMemoryHook,
    applyMemoryHook,
    setMemoryNotice,
};
