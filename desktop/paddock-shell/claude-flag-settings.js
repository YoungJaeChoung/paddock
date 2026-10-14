/**
 * Claude Code가 실행 인자(`--settings <파일>`)로 덧붙여 읽는 Paddock 전용 설정 파일을 만든다.
 *
 * 상태 줄(사용량 표시)과 메모리 안내 훅은 이 컴퓨터의 Paddock 설치 경로를 가리키는 값이다.
 * 사용자의 `settings.json`에 적으면 그 파일을 git으로 공유하는 다른 컴퓨터까지 넘어가 깨지므로, 이 컴퓨터의 Paddock 설정 폴더에 따로 두고
 * 실행할 때만 덧붙인다. 플래그 설정은 사용자 설정보다 앞서므로 상태 줄은 이 파일의 것이 쓰이고, 훅은 사용자 훅과 합쳐진다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { installMemoryHook } = require('./memory-hook-settings');
const { assertPlainPath } = require('./usage-files');

class FlagSettings {
    // Paddock 설정 폴더(`THEIA_CONFIG_DIR`) 아래 위치. 사용자 홈의 dotfiles 저장소 밖이다.
    static DIRECTORY = 'claude';
    static FILE = 'flag-settings.json';
}

/**
 * Paddock 설정 폴더 기준 플래그 설정 파일 경로.
 *
 * WSL 안의 Linux 경로를 만들 때는 `join`에 `path.posix.join`을 넘긴다.
 *
 * Examples
 * --------
 * | configDirectory          | 결과                                                |
 * | ------------------------ | --------------------------------------------------- |
 * | /home/me/.paddock/config | /home/me/.paddock/config/claude/flag-settings.json  |
 */
function flagSettingsPath(
    configDirectory,
    join = path.join,
) {
    return join(configDirectory, FlagSettings.DIRECTORY, FlagSettings.FILE);
}

/**
 * 켜져 있는 Paddock 기능만 담은 플래그 설정 객체. 둘 다 꺼져 있으면 빈 객체다.
 *
 * Examples
 * --------
 * | statusLineCommand | memoryHookCommand | 결과                                              |
 * | ----------------- | ----------------- | ------------------------------------------------- |
 * | `node s.cjs u`    | null              | `{ statusLine: { type: 'command', command } }`    |
 * | null              | `sh m.sh`         | `{ hooks: { PreToolUse: [메모리 안내 훅] } }`     |
 * | null              | null              | `{}`                                              |
 */
function buildFlagSettings(
    { statusLineCommand = null, memoryHookCommand = null } = {},
) {
    let settings = {};
    if (statusLineCommand) settings.statusLine = { type: 'command', command: statusLineCommand };
    if (memoryHookCommand) settings = installMemoryHook(settings, memoryHookCommand);
    return settings;
}

/**
 * 플래그 설정 파일을 임시 파일과 이름 바꾸기로 쓴다(0600). 내용이 같으면 쓰지 않는다.
 * 상위 폴더는 없으면 만든다. 돌려주는 값은 'written'(새로 썼음) 또는 'unchanged'다.
 */
function writeFlagSettings(
    file,
    settings,
) {
    assertPlainPath(file, 'file');
    const text = `${JSON.stringify(settings, null, 2)}\n`;
    let current = null;
    try {
        current = fs.readFileSync(file, 'utf8');
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    let result = 'unchanged';
    if (current !== text) {
        fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
        const temporary = `${file}.${randomUUID()}.tmp`;
        try {
            fs.writeFileSync(temporary, text, { mode: 0o600, flag: 'wx' });
            fs.renameSync(temporary, file);
        } finally {
            fs.rmSync(temporary, { force: true });
        }
        result = 'written';
    }
    return result;
}

module.exports = { FlagSettings, flagSettingsPath, buildFlagSettings, writeFlagSettings };
