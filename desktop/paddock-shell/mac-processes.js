// macOS 셸에서 지금 실행 중인 프로그램(터미널의 앞쪽 프로세스)을 `ps`로 찾는다.
// macOS에는 Linux의 /proc가 없어 `ps`를 실행해야 한다. 백엔드는 터미널의 입력·출력도 중계하므로, `ps`를 동기로 실행하면
// 그동안 키 입력이 셸에 늦게 닿는다. 그래서 모든 셸을 한 번에 묻고, 기다리는 동안 백엔드가 멈추지 않게 비동기로 실행한다.

const { execFile } = require('node:child_process');

/**
 * `ps -o pid=,<칸>=` 출력을 pid별 나머지 칸 문자열로 나눈다. 앞뒤 공백은 지우고, pid가 숫자가 아닌 줄은 버린다.
 *
 * Examples
 * --------
 * | 출력                              | 결과                                        |
 * | --------------------------------- | ------------------------------------------- |
 * | `  101   205\n  102     0\n`      | `Map { 101 → '205', 102 → '0' }`            |
 * | ` 205 node /usr/bin/claude -c\n`  | `Map { 205 → 'node /usr/bin/claude -c' }`   |
 * | `''`                              | `Map {}`                                    |
 */
function parsePsColumns(
    stdout,
) {
    const columns = new Map();
    for (const line of String(stdout || '').split('\n')) {
        const match = /^\s*(\d+)\s+(.*?)\s*$/.exec(line);
        if (match) columns.set(Number(match[1]), match[2]);
    }
    return columns;
}

/** `ps`를 실행해 표준 출력을 돌려준다. 없는 pid가 섞이면 ps가 실패 코드로 끝나도 나머지 줄은 출력하므로 출력을 그대로 쓴다. */
function runPs(
    args,
) {
    return new Promise(resolve => execFile('ps', args, { encoding: 'utf8' }, (error, stdout) => resolve(stdout || '')));
}

/**
 * 셸 pid마다 앞쪽 프로그램의 `{ argv, env: null }`. 앞쪽 프로세스 그룹이 없으면 셸 자신의 명령줄이고, 명령줄을 읽지 못한 셸은 null이다.
 *
 * `ps` 실행은 셸 수와 상관없이 두 번(앞쪽 그룹 → 명령줄)이다. macOS에서는 다른 프로세스의 환경을 읽지 않으므로 env는 늘 null이다.
 * `run`은 `ps` 인자를 받아 출력을 돌려주는 함수로, 기본값은 실제 `ps` 실행이다.
 *
 * Examples
 * --------
 * | 셸 pid  | `ps -o pid=,tpgid=` | `ps -o pid=,args=`           | 결과                                                      |
 * | ------- | ------------------- | ---------------------------- | --------------------------------------------------------- |
 * | 101     | `101 205`           | `205 claude`                 | `Map { 101 → { argv: ['claude'], env: null } }`           |
 * | 102     | `102 0`             | `102 /bin/zsh -il`           | `Map { 102 → { argv: ['/bin/zsh', '-il'], env: null } }`  |
 * | 103     | (줄 없음)           | (줄 없음)                    | `Map { 103 → null }`                                      |
 */
async function readMacForegrounds(
    shellPids,
    run = runPs,
) {
    const result = new Map(shellPids.map(pid => [pid, null]));
    if (shellPids.length) {
        const groups = parsePsColumns(await run(['-o', 'pid=,tpgid=', '-p', shellPids.join(',')]));
        // 앞쪽 그룹 번호는 그 그룹을 시작한 프로세스의 pid다. 그룹이 없으면(0·-1) 셸 자신을 본다.
        const foregroundOf = new Map(shellPids.map(pid => [pid, Number(groups.get(pid)) > 0 ? Number(groups.get(pid)) : pid]));
        const commandLines = parsePsColumns(await run(['-o', 'pid=,args=', '-p', [...new Set(foregroundOf.values())].join(',')]));
        for (const pid of shellPids) {
            const args = commandLines.get(foregroundOf.get(pid));
            if (args) result.set(pid, { argv: args.split(/\s+/), env: null });
        }
    }
    return result;
}

module.exports = { parsePsColumns, readMacForegrounds };
