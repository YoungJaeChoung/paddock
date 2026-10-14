// Windows 앱의 WSL 터미널이 WSL 안에서 지금 어느 폴더에 있고 무엇을 실행 중인지 읽는 순수 함수.
// Windows에서는 터미널 프로세스(wsl.exe)의 현재 폴더·앞쪽 프로그램을 알 수 없어, 터미널마다 붙인 표지(환경 변수)로
// WSL 안의 셸을 찾고 Linux와 같은 방식(/proc)으로 읽는다. 백엔드가 `wsl.exe -e sh -c <script>`로 실행한다.
const { AgentAccount, parseEnviron } = require('./agent-account');

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
 * 표지가 붙은 셸마다 `id \t Windows 경로로 바꾼 현재 폴더 \t 앞쪽 프로그램 명령줄(인자 사이 \x1f) \t 앞쪽 프로그램의 계정 환경 \t WSL 배포판 이름 \t Claude sessions 기록 본문 \t 앞쪽 프로세스 pid`를
 * 한 줄씩 출력하는 sh 스크립트. 기록 본문은 앞쪽 프로세스의 pid로 된 Claude sessions 기록이며 없으면 빈 칸이다(그 프로세스가 Claude인지는 명령줄로 가린다). 계정 환경은 CODEX_HOME·CLAUDE_CONFIG_DIR·HOME의 `이름=값`만 \x1f로 이은 것이다(다른 변수는 읽지 않는다).
 * 배포판 이름은 셸 환경의 WSL_DISTRO_NAME이다. 홈 경로가 같은 두 배포판에서 한쪽에만 등록된 계정을 다른 쪽 터미널에 붙이지 않으려고 읽는다.
 * 셸에서 실행한 프로그램도 표지를 물려받으므로, 부모에게 같은 표지가 없고 터미널이 붙은 프로세스를 셸로 본다.
 * 다른 사용자의 프로세스처럼 읽을 수 없는 항목은 조용히 건너뛴다.
 *
 * 터미널 줄 앞뒤에 메모리 줄 두 종류가 섞인다. `parse`는 이 줄을 버리고 `parseMemory`가 읽는다.
 * - `#memory \t MemTotal(kB) \t MemAvailable(kB) \t SwapTotal(kB) \t SwapFree(kB)` 한 줄(맨 앞).
 * - `#rss \t 터미널 id \t 고유 상주 페이지 수` 표지가 붙은 프로세스마다 한 줄(셸이 아닌 자식 프로세스 포함).
 *   고유 상주 페이지 = 상주 페이지 − 다른 프로세스와 나눠 쓰는 페이지(`/proc/<pid>/statm`의 둘째·셋째 칸).
 */
function script() {
    return [
        // 내장 명령만 쓴다: 프로세스를 새로 띄우면 2초마다 도는 스캔 비용이 늘어난다.
        '{ while read -r key value _; do case $key in MemTotal:) t=$value;; MemAvailable:) a=$value;; SwapTotal:) w=$value;; SwapFree:) f=$value;; esac; done < /proc/meminfo; printf \'#memory\\t%s\\t%s\\t%s\\t%s\\n\' "$t" "$a" "$w" "$f"; } 2>/dev/null',
        `for f in $(grep -alz '^${MARKER}=' /proc/[0-9]*/environ 2>/dev/null); do`,
        '  p=${f#/proc/}; p=${p%/environ}',
        `  id=$({ tr '\\0' '\\n' < "$f"; } 2>/dev/null | sed -n 's/^${MARKER}=//p')`,
        `  { read -r _ rss shared _ < /proc/$p/statm; } 2>/dev/null && [ -n "$id" ] && printf '#rss\\t%s\\t%s\\n' "$id" "$((rss - shared))"`,
        '  s=$(cat /proc/$p/stat 2>/dev/null) || continue',
        '  set -- ${s##*) }',
        `  parent=$({ tr '\\0' '\\n' < /proc/$2/environ; } 2>/dev/null | sed -n 's/^${MARKER}=//p')`,
        // A background helper (such as Codex's app-server) also inherits the marker but has no terminal (tty 0); it is not the shell.
        '  [ -n "$id" ] && [ "$parent" != "$id" ] && [ "$5" != 0 ] || continue',
        '  fg=$6; [ "$fg" -gt 0 ] 2>/dev/null || fg=$p',
        '  cwd=$(wslpath -w "$(readlink /proc/$p/cwd)" 2>/dev/null)',
        `  argv=$({ tr '\\0' '\\037' < /proc/$fg/cmdline; } 2>/dev/null)`,
        `  env=$({ tr '\\0' '\\n' < /proc/$fg/environ; } 2>/dev/null | grep -E '^(${AgentAccount.READ.join('|')})=' | tr '\\n' '\\037')`,
        `  distro=$({ tr '\\0' '\\n' < "$f"; } 2>/dev/null | sed -n 's/^WSL_DISTRO_NAME=//p')`,
        // Claude가 이 앞쪽 프로세스에 대해 남긴 sessions 기록의 대화 식별자. 계정 폴더는 위에서 읽은 환경(env)에서 얻는다.
        `  cfg=$(printf %s "$env" | tr '\\037' '\\n' | sed -n 's/^CLAUDE_CONFIG_DIR=//p')`,
        `  hm=$(printf %s "$env" | tr '\\037' '\\n' | sed -n 's/^HOME=//p')`,
        // 기록 본문(탭·줄바꿈을 지운 한 줄)을 그대로 내보낸다. 대화형·pid 일치 판정은 읽는 쪽(agent-resume.js의 claudeSessionOf)이 Linux 셸과 같은 규칙으로 한다.
        `  sess=$(head -c 4096 "\${cfg:-$hm/.claude}/sessions/$fg.json" 2>/dev/null | tr -d '\\t\\r\\n')`,
        `  printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' "$id" "$cwd" "$argv" "$env" "$distro" "$sess" "$fg"`,
        'done',
    ].join('\n');
}

/**
 * 스크립트 출력을 `{ [터미널 id]: { cwd, argv, env, distribution, sessionRecord, pid } }`로 바꾼다. 현재 폴더는 Windows 경로(\\wsl.localhost\… 또는 C:\…)이고,
 * env는 앞쪽 프로그램의 계정 환경(CODEX_HOME·CLAUDE_CONFIG_DIR·HOME 중 읽힌 것), distribution은 셸의 WSL 배포판 이름, sessionRecord는 앞쪽 프로세스(pid)의 Claude sessions 기록 본문이다.
 * 읽지 못한 값은 빈 문자열·빈 배열·빈 객체다. 넷째~일곱째 칸이 없는 이전 형식의 줄도 받는다. 형식이 맞지 않는 줄은 버린다.
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
        const [id, cwd, argv, env = '', distribution = '', sessionRecord = '', pid = ''] = line.replace(/\r$/, '').split('\t');
        // `#`로 시작하는 줄은 메모리 줄이다(parseMemory가 읽는다). 터미널 id는 `#`로 시작하지 않는다.
        if (id && !id.startsWith('#') && cwd !== undefined && argv !== undefined) {
            terminals[id] = { cwd, argv: argv.split('\x1f').filter(Boolean), env: parseEnviron(env, '\x1f'), distribution, sessionRecord, pid };
        }
    }
    return terminals;
}

/** 상주 페이지 수를 바이트로 바꾸는 페이지 크기. WSL이 도는 x86_64·arm64 리눅스는 모두 4KiB다. */
const PAGE_BYTES = 4096;

/**
 * 스크립트 출력의 메모리 줄에서 `{ total, available, swap, swapUsed, terminals }`를 읽는다. 값은 모두 바이트이고,
 * terminals는 터미널 id별로 그 표지가 붙은 모든 프로세스의 고유 상주 메모리(상주 − 공유)를 더한 값이다.
 * 어느 터미널이 큰지 비교하는 용도라 파일 캐시·공유 라이브러리를 나눠 쓰는 몫은 뺀다.
 * `#memory` 줄이 없거나 값이 숫자가 아니면 `null`이다(WSL을 읽지 못한 경우).
 *
 * Examples
 * --------
 * | 출력                                                          | 결과                                                                                 |
 * | ------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
 * | `#memory\t100\t40\t10\t4` + `#rss\tt1\t3` + `#rss\tt1\t2`     | `{ total: 102400, available: 40960, swap: 10240, swapUsed: 6144, terminals: { t1: 20480 } }` |
 * | `t1\t\tbash` (메모리 줄 없음)                                 | `null`                                                                               |
 */
function parseMemory(
    output,
) {
    let memory = null;
    const rssPages = {};
    const isNumber = text => text !== undefined && text !== '' && Number.isFinite(Number(text));
    for (const line of String(output || '').split('\n')) {
        const [tag, first, second, third, fourth] = line.replace(/\r$/, '').split('\t');
        if (tag === '#memory') {
            if ([first, second, third, fourth].every(isNumber) && Number(first) > 0) {
                const [total, available, swap, swapFree] = [first, second, third, fourth].map(Number);
                memory = { total, available, swap, swapUsed: Math.max(0, swap - swapFree) };
            }
        } else if (tag === '#rss' && first && isNumber(second)) {
            rssPages[first] = (rssPages[first] || 0) + Math.max(0, Number(second));
        }
    }
    let result = null;
    if (memory) {
        const terminals = {};
        for (const [id, pages] of Object.entries(rssPages)) terminals[id] = pages * PAGE_BYTES;
        result = {
            total: memory.total * 1024,
            available: memory.available * 1024,
            swap: memory.swap * 1024,
            swapUsed: memory.swapUsed * 1024,
            terminals,
        };
    }
    return result;
}

module.exports = { MARKER, forwardedWslEnv, isWslShell, script, parse, parseMemory };
