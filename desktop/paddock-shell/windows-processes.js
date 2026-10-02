// Windows 셸(Git Bash·PowerShell·명령 프롬프트)에서 지금 실행 중인 프로그램을 프로세스 목록으로 찾는 순수 함수.
// Windows에는 Linux의 /proc처럼 "터미널의 앞쪽 프로세스"가 없어, 셸의 자식 프로세스 중 가장 최근에 시작한 것을 앞쪽 프로그램으로 본다.

// 콘솔을 붙이려고 Windows가 띄우는 도우미 프로세스. 사용자가 실행한 프로그램이 아니다.
const CONSOLE_HELPERS = ['conhost.exe', 'openconsole.exe'];
// 다른 프로그램을 대신 실행하는 중간 프로세스(npm의 .cmd·셸 스크립트 실행기). 자식이 있으면 자식을 본다.
const LAUNCHERS = ['cmd.exe', 'sh.exe', 'bash.exe', 'env.exe'];
// 중간 프로세스를 따라 내려가는 최대 깊이. 에이전트가 띄운 하위 도구(ripgrep 등)까지 내려가지 않게 한다.
const N_MAX_DEPTH = 3;

/**
 * Windows 명령줄을 인자 배열로 나눈다. 큰따옴표로 묶인 부분은 한 인자이고, 따옴표 자체는 뺀다.
 *
 * Examples
 * --------
 * | 명령줄                                              | 결과                                          |
 * | --------------------------------------------------- | --------------------------------------------- |
 * | `"C:\Program Files\nodejs\node.exe" "C:\a b\cli.js"` | `['C:\Program Files\nodejs\node.exe', 'C:\a b\cli.js']` |
 * | `bash.exe --login -i`                               | `['bash.exe', '--login', '-i']`               |
 */
function splitCommandLine(
    commandLine,
) {
    const argv = [];
    let current = '';
    let isQuoted = false;
    let hasPart = false;
    for (const character of String(commandLine || '')) {
        if (character === '"') {
            isQuoted = !isQuoted;
            hasPart = true;
        } else if (/\s/.test(character) && !isQuoted) {
            if (hasPart) argv.push(current);
            current = '';
            hasPart = false;
        } else {
            current += character;
            hasPart = true;
        }
    }
    if (hasPart) argv.push(current);
    return argv;
}

/**
 * 셸 프로세스에서 지금 실행 중인 프로그램의 명령줄 인자. 실행 중인 프로그램이 없으면 셸 자신의 인자이고, 셸이 목록에 없으면 null이다.
 * `processes`는 `{ pid, parentPid, commandLine, createdAt }` 목록이다(createdAt은 크기만 비교한다).
 * 셸의 자식 중 가장 최근에 시작한 프로세스를 고르고, 그것이 중간 실행기(cmd·sh)이고 자식이 있으면 한 단계씩 내려간다.
 */
function foregroundArgv(
    processes,
    shellPid,
) {
    const name = item => (splitCommandLine(item.commandLine)[0] || '').split(/[\\/]/).pop().toLowerCase();
    const newestChild = parentPid => processes
        .filter(item => item.parentPid === parentPid && item.pid !== parentPid && !CONSOLE_HELPERS.includes(name(item)))
        .sort((left, right) => right.createdAt - left.createdAt)[0] ?? null;
    const shell = processes.find(item => item.pid === shellPid) ?? null;
    let current = shell ? newestChild(shellPid) : null;
    let child = current && LAUNCHERS.includes(name(current)) ? newestChild(current.pid) : null;
    for (let n_depth = 1; child && n_depth < N_MAX_DEPTH; n_depth += 1) {
        current = child;
        child = LAUNCHERS.includes(name(current)) ? newestChild(current.pid) : null;
    }
    const chosen = current ?? shell;
    return chosen ? splitCommandLine(chosen.commandLine) : null;
}

module.exports = { splitCommandLine, foregroundArgv };
