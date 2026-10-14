/**
 * "메모리가 부족할 때 에이전트에게 알리는 훅"의 명령·스크립트·설정 항목을 다룬다.
 *
 * 이 훅은 사용자 설정 파일이 아니라 Paddock 전용 플래그 설정 파일(claude-flag-settings.js)에 들어간다.
 * 이 모듈이 넣은 항목은 명령에 `memory-hook.sh`가 들어 있는 것으로 알아본다. 사용자가 직접 만든 훅과 구분하는 기준이다.
 * 예전 판이 사용자 설정 파일에 넣어 둔 항목을 걷어 낼 때도 같은 기준을 쓴다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { assertPlainPath } = require('./usage-files');

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
 * 메모리 안내의 스크립트 복사와 꺼짐 표시 파일을 맞추고 결과 상태('on'|'off')를 돌려준다.
 *
 * 설정 화면의 켜기·끄기가 이 함수로 이어지게 하려고 있다. 설정(훅 항목)은 플래그 설정 파일을 다시 쓰는 쪽이 맡는다.
 * 훅 항목이 플래그 파일에서 사라져도 이미 떠 있는 세션이 스크립트를 계속 부를 수 있어 끌 때는 꺼짐 표시 파일을 먼저 둔다.
 *
 * Examples
 * --------
 * | enabled | scriptFile 있음 | 하는 일                                                   |
 * | ------- | --------------- | --------------------------------------------------------- |
 * | true    | 상관없음        | 스크립트 복사 → 꺼짐 표시 삭제                            |
 * | false   | 있음            | 꺼짐 표시 생성                                            |
 * | false   | 없음            | 아무 파일도 만들지 않는다(켠 적 없는 곳에 흔적을 두지 않음) |
 */
function setMemoryNotice(
    { scriptFile, source, enabled },
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
    return enabled ? 'on' : 'off';
}

module.exports = {
    MemoryHook,
    memoryHookCommand,
    isMemoryHookCommand,
    installMemoryHook,
    removeMemoryHook,
    hasMemoryHook,
    setMemoryNotice,
};
