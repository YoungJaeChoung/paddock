// Windows 앱의 WSL 터미널이 WSL 안에서 지금 어느 폴더에 있고 무엇을 실행 중인지 읽는 순수 함수.
// Windows에서는 터미널 프로세스(wsl.exe)의 현재 폴더·앞쪽 프로그램을 알 수 없어, 터미널마다 붙인 표지(환경 변수)로
// WSL 안의 셸을 찾고 Linux와 같은 방식(/proc)으로 읽는다. 백엔드가 `wsl.exe -e sh -c <script>`로 실행한다.

/** WSL 안의 셸에 전달하는 터미널 표지. 값은 Paddock 터미널 id다. */
const MARKER = 'PADDOCK_TERMINAL';

/**
 * WSL 터미널에 넘길 WSLENV 값. WSL은 Windows 환경 변수 중 WSLENV에 이름을 적은 것만 Linux 셸에 넘긴다.
 *
 * 사용자가 정해 둔 WSLENV 항목을 앞에 그대로 두고, 터미널 표지와 `COLORTERM`을 덧붙인다.
 * `COLORTERM=truecolor`는 터미널이 1600만 색을 그린다는 표시다. 빠지면 WSL 안의 Claude Code·Codex 같은 프로그램이
 * 색을 가까운 기본 16색으로 바꿔 그려, 은은한 짙은 회색 배경이 밝은 회색(#666666) 판으로 보인다.
 * 이미 적힌 이름(`COLORTERM/u`처럼 변환 표시가 붙은 것 포함)은 다시 붙이지 않는다.
 *
 * Examples
 * --------
 * | windowsWslEnv   | 결과                                       |
 * |-----------------|--------------------------------------------|
 * | ''              | 'PADDOCK_TERMINAL:COLORTERM'               |
 * | 'USERPROFILE/p' | 'USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM' |
 * | 'COLORTERM/u'   | 'COLORTERM/u:PADDOCK_TERMINAL'             |
 */
function forwardedWslEnv(
    windowsWslEnv,
) {
    const entries = String(windowsWslEnv || '').split(':').filter(Boolean);
    const names = new Set(entries.map(entry => entry.split('/')[0]));
    for (const name of [MARKER, 'COLORTERM']) {
        if (!names.has(name)) entries.push(name);
    }
    return entries.join(':');
}

/** 셸 실행 파일이 wsl.exe인지. 경로 구분자와 대소문자는 가리지 않는다. */
function isWslShell(
    shellPath,
) {
    return /(^|[\\/])wsl(\.exe)?$/i.test(String(shellPath || ''));
}

/**
 * 표지가 붙은 셸마다 `id \t Windows 경로로 바꾼 현재 폴더 \t 앞쪽 프로그램 명령줄(인자 사이 \x1f)`을 한 줄씩 출력하는 sh 스크립트.
 * 셸에서 실행한 프로그램도 표지를 물려받으므로, 부모에게 같은 표지가 없는 프로세스를 셸로 본다.
 * 다른 사용자의 프로세스처럼 읽을 수 없는 항목은 조용히 건너뛴다.
 */
function script() {
    return [
        `for f in $(grep -alz '^${MARKER}=' /proc/[0-9]*/environ 2>/dev/null); do`,
        '  p=${f#/proc/}; p=${p%/environ}',
        `  id=$({ tr '\\0' '\\n' < "$f"; } 2>/dev/null | sed -n 's/^${MARKER}=//p')`,
        '  s=$(cat /proc/$p/stat 2>/dev/null) || continue',
        '  set -- ${s##*) }',
        `  parent=$({ tr '\\0' '\\n' < /proc/$2/environ; } 2>/dev/null | sed -n 's/^${MARKER}=//p')`,
        '  [ -n "$id" ] && [ "$parent" != "$id" ] || continue',
        '  fg=$6; [ "$fg" -gt 0 ] 2>/dev/null || fg=$p',
        '  cwd=$(wslpath -w "$(readlink /proc/$p/cwd)" 2>/dev/null)',
        `  argv=$({ tr '\\0' '\\037' < /proc/$fg/cmdline; } 2>/dev/null)`,
        `  printf '%s\\t%s\\t%s\\n' "$id" "$cwd" "$argv"`,
        'done',
    ].join('\n');
}

/**
 * 스크립트 출력을 `{ [터미널 id]: { cwd, argv } }`로 바꾼다. 현재 폴더는 Windows 경로(\\wsl.localhost\… 또는 C:\…)이고,
 * 읽지 못한 값은 빈 문자열·빈 배열이다. 형식이 맞지 않는 줄은 버린다.
 *
 * Examples
 * --------
 * | 출력 줄                                                    | 결과                                                             |
 * | ---------------------------------------------------------- | ---------------------------------------------------------------- |
 * | `t1\t\\wsl.localhost\Ubuntu\home\me\app\tclaude`            | `{ t1: { cwd: '\\wsl.localhost\Ubuntu\home\me\app', argv: ['claude'] } }` |
 * | `t2\t\t-bash`                                              | `{ t2: { cwd: '', argv: ['-bash'] } }`                           |
 */
function parse(
    output,
) {
    const terminals = {};
    for (const line of String(output || '').split('\n')) {
        const [id, cwd, argv] = line.replace(/\r$/, '').split('\t');
        if (id && cwd !== undefined && argv !== undefined) {
            terminals[id] = { cwd, argv: argv.split('\x1f').filter(Boolean) };
        }
    }
    return terminals;
}

module.exports = { MARKER, forwardedWslEnv, isWslShell, script, parse };
