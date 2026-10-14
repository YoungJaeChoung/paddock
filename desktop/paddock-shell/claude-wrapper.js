/**
 * 일반 터미널에서 친 `claude`에도 Paddock 전용 설정(`--settings <플래그 파일>`)이 붙게 하는 실행 래퍼.
 *
 * 계정 터미널은 시작 명령에 `--settings`를 직접 넣지만, 일반 터미널은 사용자가 `claude`를 직접 치므로
 * Paddock 설정 폴더의 `bin` 폴더를 터미널 PATH 맨 앞에 두고 거기에 같은 이름의 래퍼를 놓는다.
 * 래퍼는 자기 폴더를 PATH에서 뺀 뒤 진짜 `claude`를 `--settings`와 함께 실행하고, 나머지 인자는 그대로 넘긴다.
 * 플래그 파일이 없으면 인자를 붙이지 않는다.
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');

class ClaudeWrapper {
    // Paddock 설정 폴더 아래 래퍼 폴더 이름.
    static DIRECTORY = 'bin';
    // `sh`는 Linux·macOS·WSL용, `cmd`는 Windows 명령 프롬프트·PowerShell용이다.
    static FILES = Object.freeze({ sh: 'claude', cmd: 'claude.cmd' });
    // ~/.bashrc에 래퍼 폴더를 PATH 앞에 넣을 때 쓰는 표지. 이 표지가 든 줄만 Paddock이 넣은 것으로 알아본다.
    static BASHRC_MARKER = '# paddock-claude-wrapper';
}

/** 값을 POSIX 셸 한 단어로 감싼다. */
function shellWord(
    value,
) {
    return `'${String(value).replace(/'/g, "'\\''")}'`;
}

/**
 * 래퍼 스크립트 본문.
 *
 * `ownDirectory`는 래퍼 폴더의 경로(cmd 래퍼가 PATH에서 자기 폴더를 뺄 때 쓴다. sh 래퍼는 실행 위치로 스스로 안다).
 *
 * Examples
 * --------
 * | kind | 하는 일                                                                                          |
 * | ---- | ------------------------------------------------------------------------------------------------ |
 * | sh   | 자기 폴더를 PATH에서 뺌 → 플래그 파일이 있으면 `exec claude --settings <파일> "$@"`, 없으면 `exec claude "$@"` |
 * | cmd  | 자기 폴더를 PATH에서 뺌 → 같은 규칙으로 `claude --settings "<파일>" %*`                         |
 */
function claudeWrapperScript(
    { kind, flagFile, ownDirectory },
) {
    let script;
    if (kind === 'sh') {
        script = [
            '#!/bin/sh',
            '# Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.',
            'own=$(cd "$(dirname "$0")" && pwd)',
            // 래퍼 폴더만 PATH에서 빼 진짜 claude를 찾게 한다. 빈 PATH 조각은 현재 폴더를 뜻하므로 함께 버린다.
            'PATH=$(printf %s "$PATH" | tr \':\' \'\\n\' | grep -vxF -e "$own" -e \'\' | paste -sd: -)',
            'export PATH',
            `flag=${shellWord(flagFile)}`,
            'if [ -f "$flag" ]; then exec claude --settings "$flag" "$@"; fi',
            'exec claude "$@"',
            '',
        ].join('\n');
    } else {
        const own = String(ownDirectory);
        script = [
            '@echo off',
            'rem Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.',
            'setlocal',
            `set "PATH=%PATH:${own};=%"`,
            `set "PATH=%PATH:;${own}=%"`,
            `if not exist "${flagFile}" goto plain`,
            `claude --settings "${flagFile}" %*`,
            'exit /b %ERRORLEVEL%',
            ':plain',
            'claude %*',
            'exit /b %ERRORLEVEL%',
            '',
        ].join('\r\n');
    }
    return script;
}

/**
 * 래퍼를 `directory`에 쓴다(실행 권한 0700). 내용이 같으면 쓰지 않고, 돌려주는 값은 쓴 파일 경로다.
 *
 * `directory`는 이 앱이 파일을 쓰는 경로이고, `flagFile`·`ownDirectory`는 래퍼가 실행되는 환경에서 보는 경로다
 * (Windows 앱이 WSL 안에 놓는 래퍼는 앞의 것이 \\wsl.localhost 경로, 뒤의 것이 Linux 경로다).
 * 같은 이름의 심볼릭 링크나 폴더가 있으면 따라가 쓰지 않고 오류를 던진다.
 */
function installClaudeWrapper(
    { directory, kind, flagFile, ownDirectory },
) {
    const file = path.join(directory, ClaudeWrapper.FILES[kind]);
    const script = claudeWrapperScript({ kind, flagFile, ownDirectory });
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
    let existing = null;
    try {
        existing = fs.lstatSync(file);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    if (existing && (!existing.isFile() || existing.isSymbolicLink())) throw new Error('The Claude launcher is not a regular file. Restore it before trying again.');
    if (!existing || fs.readFileSync(file, 'utf8') !== script) {
        const temporary = path.join(directory, `.${ClaudeWrapper.FILES[kind]}.${randomUUID()}.tmp`);
        try {
            fs.writeFileSync(temporary, script, { mode: 0o700, flag: 'wx' });
            fs.renameSync(temporary, file);
        } finally {
            fs.rmSync(temporary, { force: true });
        }
    }
    return file;
}

/**
 * 터미널 환경의 PATH 맨 앞에 래퍼 폴더를 붙인 새 환경을 돌려준다. 받은 환경 객체는 바꾸지 않는다.
 *
 * 기준 PATH는 `env`의 PATH(Windows는 대소문자 무관)이고, 없으면 `baseEnv`(보통 이 프로세스 환경)의 것이다.
 * 이미 맨 앞에 있으면 다시 붙이지 않는다. Windows의 `Path` 키는 이름을 유지한다.
 *
 * Examples
 * --------
 * | env.PATH      | wrapperDirectory | 결과 PATH           |
 * | ------------- | ---------------- | ------------------- |
 * | /usr/bin      | /p/bin           | /p/bin:/usr/bin     |
 * | /p/bin:/usr/bin | /p/bin         | /p/bin:/usr/bin     |
 * | (없음, baseEnv.PATH=/x) | /p/bin | /p/bin:/x           |
 */
function withWrapperPath(
    env,
    wrapperDirectory,
    { baseEnv = process.env, isWindows = process.platform === 'win32' } = {},
) {
    const separator = isWindows ? ';' : ':';
    const keyOf = source => Object.keys(source || {}).find(name => (isWindows ? name.toLowerCase() === 'path' : name === 'PATH'));
    const ownKey = keyOf(env);
    const baseKey = keyOf(baseEnv);
    const key = ownKey || baseKey || (isWindows ? 'Path' : 'PATH');
    const current = ownKey && typeof env[ownKey] === 'string' ? env[ownKey] : baseKey && typeof baseEnv[baseKey] === 'string' ? baseEnv[baseKey] : '';
    const entries = current.split(separator).filter(Boolean);
    const next = entries[0] === wrapperDirectory ? current : [wrapperDirectory, ...entries].join(separator);
    return { ...(env || {}), [key]: next };
}

module.exports = { ClaudeWrapper, claudeWrapperScript, installClaudeWrapper, withWrapperPath };
