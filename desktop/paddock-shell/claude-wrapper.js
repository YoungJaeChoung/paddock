/**
 * 일반 터미널에서 친 `claude`에도 Paddock 전용 설정(`--settings <플래그 파일>`)이 붙게 하는 실행 래퍼.
 *
 * 계정 터미널은 시작 명령에 `--settings`를 직접 넣지만, 일반 터미널은 사용자가 `claude`를 직접 치므로
 * Paddock 설정 폴더의 `bin` 폴더를 터미널 PATH 맨 앞에 두고 거기에 같은 이름의 래퍼를 놓는다.
 * 래퍼는 자기 폴더를 PATH에서 뺀 뒤 진짜 `claude`를 `--settings`와 함께 실행하고, 나머지 인자는 그대로 넘긴다.
 * 플래그 파일이 없거나 첫 인자가 `remote-control`이면 인자를 붙이지 않는다.
 *
 * Pen 연동 MCP 설정 파일(`--mcp-config=<파일>`)도 같은 규칙으로 붙인다. 이 파일은 네이티브 앱만 쓰고 이 앱은 쓰지 않지만,
 * 두 앱이 같은 래퍼 글자를 놓아 서로 덮어써도 달라지지 않도록 래퍼 본문은 네이티브 앱과 같은 글자로 만든다.
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
 * cmd 래퍼가 파일을 가리키는 글자. 래퍼 폴더(`ownDirectory`)의 부모 폴더 아래 파일이면 래퍼 위치에서 펼치는 `%~dp0..\<나머지>`이고,
 * 아니면 경로 그대로다.
 *
 * 왜: cmd는 배치 파일을 콘솔 코드 페이지(한국어 Windows는 949)로 읽어, UTF-8로 적어 둔 한글 경로가 깨지고 `if exist`가 거짓이 된다.
 * `%~dp0`(이 배치 파일의 폴더)은 실행 중에 펼쳐지므로 코드 페이지와 상관없이 맞다.
 *
 * Examples
 * --------
 * | file                                 | ownDirectory               | 결과                    |
 * | ------------------------------------ | -------------------------- | ----------------------- |
 * | `C:\Users\영재\config\claude\f.json` | `C:\Users\영재\config\bin` | `%~dp0..\claude\f.json` |
 * | `D:\other\f.json`                    | `C:\config\bin`            | `D:\other\f.json`       |
 */
function cmdReference(
    file,
    ownDirectory,
) {
    const own = String(ownDirectory).replace(/\\+$/, '');
    const cut = own.lastIndexOf('\\');
    const parent = cut >= 0 ? `${own.slice(0, cut)}\\` : null;
    const text = String(file);
    return parent !== null && text.startsWith(parent) ? `%~dp0..\\${text.slice(parent.length)}` : text;
}

/**
 * 래퍼 스크립트 본문. 네이티브 앱(`claude_wrapper_script`)과 같은 글자를 낸다.
 *
 * `flagFile`은 Paddock 플래그 설정 파일, `mcpConfigFile`은 Pen 연동 MCP 설정 파일이고, 각각 있을 때만 붙는다.
 * `ownDirectory`는 래퍼 폴더의 경로(cmd 래퍼가 두 설정 파일을 자기 위치 기준으로 가리킬 때 쓴다, `cmdReference`. sh 래퍼는 쓰지 않는다).
 *
 * Examples
 * --------
 * | kind | 하는 일                                                                                          |
 * | ---- | ------------------------------------------------------------------------------------------------ |
 * | sh   | 자기 폴더를 PATH에서 뺌 → 플래그 파일이 있으면 `--settings <파일>`, Pen 설정 파일이 있으면 `--mcp-config=<파일>`을 앞에 더해 `exec claude … "$@"` |
 * | cmd  | 자기 폴더(`%~dp0`)를 PATH에서 뺌 → 같은 규칙으로 `claude --settings "%~dp0..\<파일>" --mcp-config="%~dp0..\<파일>" %*` |
 * | 둘 다 | `claude remote-control ...`은 두 파일이 있어도 인자를 더하지 않고 그대로 넘긴다                  |
 */
function claudeWrapperScript(
    { kind, flagFile, mcpConfigFile, ownDirectory },
) {
    let script;
    if (kind === 'sh') {
        script = [
            '#!/bin/sh',
            '# Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.',
            // PATH를 앞에서부터 훑어 자기 자신이 아닌 첫 claude를 찾는다. 같은 파일인지는 경로 표기(끝 슬래시·..)가 아니라 파일 동일성(-ef)으로 가려,
            // 래퍼 폴더가 다른 표기로 PATH에 남아 있어도 자기 자신을 다시 실행하는 무한 반복이 생기지 않는다. 빈 PATH 조각은 건너뛴다.
            'real=',
            'old_ifs=$IFS',
            "IFS=:",
            'for dir in $PATH; do',
            '    [ -n "$dir" ] || continue',
            '    if [ -f "$dir/claude" ] && [ -x "$dir/claude" ] && ! [ "$dir/claude" -ef "$0" ]; then real="$dir/claude"; break; fi',
            'done',
            'IFS=$old_ifs',
            'if [ -z "$real" ]; then printf \'%s\\n\' "claude was not found in PATH (Paddock\'s launcher found only itself). Install Claude Code, then try again." >&2; exit 127; fi',
            `flag=${shellWord(flagFile)}`,
            `mcp=${shellWord(mcpConfigFile)}`,
            // `claude remote-control`(원격 접속 서버)은 하위 명령 앞의 `--settings`를 자기가 띄우는 세션에 넘길 수 없다며 시작을 거부한다.
            // 그래서 이 명령일 때만 플래그 파일을 붙이지 않는다. 대화 세션의 `--remote-control`·`/remote-control`은 `--settings`와 함께 동작한다.
            // Pen 연동 설정 파일도 같은 규칙이다. `--mcp-config=` 한 단어로 붙여, 값을 여러 개 받는 옵션이 뒤의 프롬프트 인자를 삼키지 않게 한다.
            'if [ "$1" != remote-control ]; then',
            '    if [ -f "$mcp" ]; then set -- "--mcp-config=$mcp" "$@"; fi',
            '    if [ -f "$flag" ]; then set -- --settings "$flag" "$@"; fi',
            'fi',
            'exec "$real" "$@"',
            '',
        ].join('\n');
    } else {
        const flag = cmdReference(flagFile, ownDirectory);
        const mcp = cmdReference(mcpConfigFile, ownDirectory);
        script = [
            '@echo off',
            'rem Paddock가 만든 claude 실행 래퍼. Paddock 전용 설정(상태 줄·메모리 안내)을 --settings로 덧붙이고 진짜 claude를 실행한다.',
            'setlocal',
            // 자기 폴더와 설정 파일은 글자로 적지 않고 이 파일의 위치(`%~dp0`)에서 펼친다. 적어 둔 한글 경로는 콘솔 코드 페이지에 따라 깨진다.
            // 펼친 값으로 PATH를 고치려면 한 번 더 펼쳐야 해서 `call set`을 쓴다.
            'set "own=%~dp0"',
            'set "own=%own:~0,-1%"',
            'call set "PATH=%%PATH:%own%;=%%"',
            'call set "PATH=%%PATH:;%own%=%%"',
            // 위에서 못 뺀 다른 표기의 자기 폴더가 PATH에 남아도 자기 자신을 다시 실행하지 않도록, 찾은 claude가 이 파일이면 멈춘다.
            'set "real="',
            'for /f "delims=" %%i in (\'where claude 2^>nul\') do if not defined real if /i not "%%~fi"=="%~f0" set "real=%%i"',
            'if not defined real (',
            '    echo claude was not found in PATH ^(Paddock launcher found only itself^). Install Claude Code, then try again. 1>&2',
            '    exit /b 9009',
            ')',
            // sh 래퍼와 같은 이유로 `claude remote-control`에는 --settings·--mcp-config를 붙이지 않는다.
            // 파일 유무 네 경우를 변수에 모으지 않고 각각 한 줄로 펼친다. 경로를 변수에 담으면 `&`·`|`·`<`·`>`가 든 경로에서 명령이 깨진다.
            'if /i "%~1"=="remote-control" goto plain',
            `if exist "${flag}" goto flag`,
            `if exist "${mcp}" goto mcp`,
            'goto plain',
            ':flag',
            `if exist "${mcp}" goto both`,
            `"%real%" --settings "${flag}" %*`,
            'exit /b %ERRORLEVEL%',
            ':both',
            `"%real%" --settings "${flag}" --mcp-config="${mcp}" %*`,
            'exit /b %ERRORLEVEL%',
            ':mcp',
            `"%real%" --mcp-config="${mcp}" %*`,
            'exit /b %ERRORLEVEL%',
            ':plain',
            '"%real%" %*',
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
    { directory, kind, flagFile, mcpConfigFile, ownDirectory },
) {
    const file = path.join(directory, ClaudeWrapper.FILES[kind]);
    const script = claudeWrapperScript({ kind, flagFile, mcpConfigFile, ownDirectory });
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
