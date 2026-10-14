/**
 * Removes the entries older Paddock builds wrote into the user's Claude Code settings files, once.
 *
 * Those builds put an absolute path to the Paddock status line script (`claude-statusline.cjs`) and to the memory notice hook
 * (`memory-hook.sh`) into `settings.json`. A settings file shared between computers through a dotfiles repository then carried one
 * computer's path to the others, where it broke. Current builds pass the same entries with `--settings` instead (claude-flag-settings.js),
 * so the old entries are dead weight at best and a broken path at worst.
 *
 * Only entries that name those two Paddock files are removed. Every other key stays as it was, and a settings file that is a link
 * (a dotfiles repository file) is edited at its real location so the link survives. A status line the entry replaced is put back when
 * its record (`previous-statusline.json`) can be read.
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { removeMemoryHook } = require('./memory-hook-settings');
const usage = require('./usage-model');

class LegacyCleanup {
    // 예전 상태 줄 명령에 들어 있는 스크립트 이름. 사용자의 다른 상태 줄과 구분하는 기준이다.
    static STATUS_LINE_SCRIPT = 'claude-statusline.cjs';
    static MARKER_FILE = 'legacy-cleanup.json';
    static MARKER_VERSION = 1;
}

/**
 * 설정에서 예전 Paddock 항목만 뺀 새 설정과, 뺀 항목의 이름 목록.
 * 뺄 것이 없으면 받은 설정 객체를 그대로 돌려준다(같은 참조라 호출부가 변경 여부를 안다).
 * `previousStatusLine`은 그 상태 줄 항목이 가려 두었던 원래 상태 줄이다. 읽을 수 있고 Paddock 항목이 아닐 때만 되돌려 놓는다.
 *
 * Examples
 * --------
 * | 설정                                                         | removed                      | 결과                        |
 * | ------------------------------------------------------------ | ---------------------------- | --------------------------- |
 * | `{ statusLine: <claude-statusline.cjs 명령>, theme }`        | `['status line']`            | `{ theme }`                 |
 * | 위 + previousStatusLine=`{ type, command: 'my-line' }`       | `['status line']`            | `{ theme, statusLine: 원래 }` |
 * | `{ hooks: { PreToolUse: [<memory-hook.sh 훅>] } }`           | `['memory hook']`            | `{}`                        |
 * | `{ statusLine: <사용자 명령> }`                              | `[]`                         | 받은 객체 그대로            |
 */
function withoutPaddockEntries(
    settings,
    previousStatusLine = null,
) {
    const removed = [];
    let next = settings;
    const command = settings.statusLine?.command;
    if (typeof command === 'string' && command.includes(LegacyCleanup.STATUS_LINE_SCRIPT)) {
        const isRestorable = previousStatusLine?.type === 'command' && typeof previousStatusLine.command === 'string'
            && !previousStatusLine.command.includes(LegacyCleanup.STATUS_LINE_SCRIPT);
        // 키 순서를 지키려고 같은 자리에서 값만 바꾸거나 빼며, 사용자의 다른 키 순서는 그대로 둔다.
        next = Object.fromEntries(Object.entries(next).flatMap(([key, value]) => {
            let entries = [[key, value]];
            if (key === 'statusLine') entries = isRestorable ? [[key, previousStatusLine]] : [];
            return entries;
        }));
        removed.push('status line');
    }
    const withoutHook = removeMemoryHook(next);
    if (withoutHook !== next) {
        next = withoutHook;
        removed.push('memory hook');
    }
    return { settings: next, removed };
}

/**
 * 설정 파일 한 곳에서 예전 Paddock 항목을 걷어 낸다. 파일이 없거나 읽을 수 없는 JSON이면 아무것도 하지 않는다.
 *
 * `file`이 링크면 링크를 따라간 실제 위치에 쓴다(`resolveLink`가 실제 위치를 돌려준다). 같은 폴더에 임시 파일을 만들어 이름을 바꾸므로
 * 링크는 그대로 남고, 들여쓰기·끝 줄바꿈·권한은 원래 파일을 따른다. `dryRun`이면 계산만 하고 쓰지 않는다.
 * `readPrevious`는 상태 줄 항목이 가리키는 기록 파일 경로를 받아 원래 상태 줄 객체를 돌려준다(없으면 null).
 *
 * 돌려주는 값은 `{ file, target, isLinked, removed }`. `removed`가 비어 있으면 파일은 그대로다. `isSkipped`는 건드릴 수 없었던 이유를 담는다.
 */
function cleanSettingsFile(
    { file, resolveLink = realLocation, readPrevious = () => null, dryRun = false },
) {
    const result = { file, target: file, isLinked: false, removed: [], isSkipped: '' };
    let info = null;
    try {
        info = fs.lstatSync(file);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    if (info) {
        result.isLinked = info.isSymbolicLink();
        result.target = resolveLink(file);
        let text = null;
        let data = null;
        try {
            text = fs.readFileSync(result.target, 'utf8');
            data = JSON.parse(text);
        } catch {
            result.isSkipped = 'unreadable';
        }
        if (data !== null && (typeof data !== 'object' || Array.isArray(data))) result.isSkipped = 'not an object';
        if (!result.isSkipped && data !== null) {
            const owner = usage.paddockStatusLineFiles(data.statusLine?.command);
            const previous = owner?.previousFile ? readPrevious(owner.previousFile) : null;
            const cleaned = withoutPaddockEntries(data, previous);
            result.removed = cleaned.removed;
            if (cleaned.removed.length && !dryRun) {
                const indent = /\n( +|\t)"/.exec(text)?.[1] ?? '  ';
                const output = JSON.stringify(cleaned.settings, null, indent) + (text.endsWith('\n') ? '\n' : '');
                const temporaryFile = path.join(path.dirname(result.target), `.${path.basename(result.target)}.${randomUUID()}.tmp`);
                try {
                    fs.writeFileSync(temporaryFile, output, { mode: fs.statSync(result.target).mode & 0o777, flag: 'wx' });
                    fs.renameSync(temporaryFile, result.target);
                } finally {
                    fs.rmSync(temporaryFile, { force: true });
                }
            }
        }
    }
    return result;
}

/** 링크를 모두 따라간 실제 경로. 링크가 아니면 받은 경로를 그대로 돌려준다. */
function realLocation(
    file,
) {
    let result = file;
    try {
        result = fs.realpathSync(file);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    return result;
}

/**
 * 여러 설정 파일을 걷어 내고 같은 실제 파일은 한 번만 다룬다(계정 프로필의 설정이 기본 파일을 가리키는 링크일 때 같은 파일이 여러 번 나온다).
 * 한 파일이 실패해도 나머지를 계속하며, 실패한 파일은 `isSkipped`에 이유를 담아 돌려준다.
 */
function cleanSettingsFiles(
    entries,
    { dryRun = false } = {},
) {
    const results = [];
    const seen = new Set();
    for (const entry of entries) {
        let result;
        try {
            const target = (entry.resolveLink || realLocation)(entry.file);
            if (!seen.has(target)) {
                seen.add(target);
                result = cleanSettingsFile({ ...entry, dryRun });
            }
        } catch (error) {
            result = { file: entry.file, target: entry.file, isLinked: false, removed: [], isSkipped: error instanceof Error ? error.message : String(error) };
        }
        if (result) results.push(result);
    }
    return results;
}

/**
 * 걷어 낸 결과를 사용자에게 보일 문장으로 만든다. 걷어 낸 것이 없으면 빈 목록이다.
 * 링크였던 파일(저장소 파일)은 저장소가 바뀐 것이므로 올리라는 안내를 붙인다.
 *
 * Examples
 * --------
 * | 결과                                                   | 문장                                                                                      |
 * | ------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
 * | 일반 파일 `~/.claude/settings.json`에서 status line    | `Removed the old Paddock status line from /h/.claude/settings.json.`                      |
 * | 링크 → `/repo/claude/settings.json`에서 둘 다          | `Removed the old Paddock status line and memory hook from /h/.claude/settings.json (a link to /repo/claude/settings.json). That repository file changed: upload it, for example with sync.sh.` |
 */
function describeCleanup(
    results,
) {
    return results.filter(result => result.removed.length).map(result => {
        const where = result.isLinked ? `${result.file} (a link to ${result.target})` : result.file;
        const repository = result.isLinked ? ' That repository file changed: upload it, for example with sync.sh.' : '';
        return `Removed the old Paddock ${result.removed.join(' and ')} from ${where}.${repository}`;
    });
}

module.exports = { LegacyCleanup, withoutPaddockEntries, cleanSettingsFile, cleanSettingsFiles, describeCleanup };
