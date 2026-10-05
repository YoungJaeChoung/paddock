/** Changes only Claude's status line while preserving the command it previously displayed. */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const usage = require('./usage-model');
const { assertPlainPath, usageError, readJson } = require('./usage-files');

function writeJson(
    file,
    value,
) {
    assertPlainPath(file, 'file');
    const temporary = `${file}.${randomUUID()}.tmp`;
    try {
        fs.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
        fs.renameSync(temporary, file);
    } finally {
        fs.rmSync(temporary, { force: true });
    }
}

/**
 * 기본 Claude 설정 파일이 심볼릭 링크면 링크를 모두 따라간 실제 파일 경로를, 링크가 아니거나 아직 없으면 받은 경로를 돌려준다.
 * 설정 파일을 dotfiles 저장소로 링크해 두는 경우가 흔하다. 링크 자리에 쓰면 rename이 링크를 일반 파일로 바꿔 연결이 끊기므로 실제 파일에 쓴다.
 * 대상이 없는 링크는 그대로 돌려주어 applyStatusLine이 거부하게 한다.
 * 등록 계정 폴더의 설정은 이 함수를 거치지 않는다 — 그쪽 링크는 계정 폴더 밖을 가리키는 경로라 거부 대상이다.
 */
function settingsTarget(
    file,
) {
    let result = file;
    try {
        if (fs.lstatSync(file).isSymbolicLink()) result = fs.realpathSync(file);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    return result;
}

/**
 * Claude Code 설정 파일 하나에 Paddock 상태 줄을 넣거나 되돌리고 결과 상태를 돌려준다.
 * 같은 설정 파일을 다른 Paddock 설정 폴더(설치본·시험용 실행)가 이미 쓰는 중이면
 * 자동 켜기는 빼앗지 않고 'unset'을 돌려준다. 그 스크립트가 지워졌으면 주인이 없는 것으로 보고 넘겨받는다.
 * `toLocal`은 명령 속 Linux 경로를 Windows에서 읽을 경로로 바꾼다. 계정별 설치는 이전 기록도 같은 폴더 안에 제한한다.
 */
function applyStatusLine(
    { settingsPath, previousFile, command, enabled, isAutomatic, toLocal, scoped = false },
) {
    for (const file of [settingsPath, `${settingsPath}.paddock-backup`, previousFile]) assertPlainPath(file, 'file');
    let settings = {};
    if (fs.existsSync(settingsPath)) {
        try {
            settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
        } catch {
            throw usageError('Claude settings contain invalid JSON. Repair the settings before changing usage display.');
        }
        if (!settings || typeof settings !== 'object' || Array.isArray(settings)) throw usageError('Claude settings must be a JSON object.');
    }
    const owner = usage.paddockStatusLineFiles(settings.statusLine?.command);
    const previousOwnerFile = owner ? toLocal(owner.previousFile) : null;
    const ownerScript = owner ? toLocal(owner.script) : null;
    const isTakenByOther = Boolean(owner) && settings.statusLine.command !== command && fs.existsSync(ownerScript);
    let state = enabled ? 'on' : 'off';
    if (enabled && isAutomatic && isTakenByOther) {
        state = 'unset';
    } else {
        if (enabled && scoped && owner && settings.statusLine.command !== command && previousOwnerFile !== previousFile) {
            throw usageError('Another usage collector points outside this account. Restore its original status line before replacing it.');
        }
        let next = settings;
        let removePrevious = false;
        if (enabled && settings.statusLine?.type === 'command' && settings.statusLine.command === command) {
            // 이미 같은 명령이 들어 있으면 파일을 다시 쓰지 않는다. 다시 쓰면 사용자 파일의 줄바꿈·서식이 바뀌고 백업 파일이 생긴다.
            next = settings;
        } else if (enabled) {
            const installed = usage.installStatusLine(settings, command);
            // 다른 Paddock 명령을 바꾸면 그 명령이 이어 부르던 원래 상태 줄을 넘겨받는다. Paddock 명령끼리는 잇지 않는다.
            const inheritedPath = installed.replaced ? toLocal(installed.replaced.previousFile) : null;
            if (inheritedPath) assertPlainPath(inheritedPath, 'file');
            const inherited = inheritedPath ? readJson(inheritedPath, null) : null;
            const previous = installed.previous || (inherited && !usage.paddockStatusLineFiles(inherited.command) ? inherited : null);
            if (previous) writeJson(previousFile, previous);
            next = installed.settings;
        } else if (settings.statusLine?.command === command) {
            next = usage.restoreStatusLine(settings, readJson(previousFile, null));
            removePrevious = true;
        }
        if (next !== settings) {
            fs.mkdirSync(path.dirname(settingsPath), { recursive: true, mode: 0o700 });
            // 처음 바꿀 때 한 번만 원본을 남긴다.
            if (fs.existsSync(settingsPath) && !fs.existsSync(`${settingsPath}.paddock-backup`)) fs.copyFileSync(settingsPath, `${settingsPath}.paddock-backup`, fs.constants.COPYFILE_EXCL);
            writeJson(settingsPath, next);
            if (removePrevious) fs.rmSync(previousFile, { force: true });
        }
    }
    return state;
}

module.exports = { applyStatusLine, settingsTarget, writeJson };
