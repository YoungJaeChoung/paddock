const test = require('node:test');
const assert = require('node:assert/strict');
const { parsePsColumns, readMacForegrounds } = require('../mac-processes');

test('C-macproc.1: ps 출력을 pid별 나머지 칸으로 나누고 숫자가 아닌 줄은 버린다', () => {
    assert.deepEqual(parsePsColumns('  101   205\n  102     0\nPID TPGID\n'), new Map([[101, '205'], [102, '0']]));
    assert.deepEqual(parsePsColumns(' 205 node /usr/bin/claude -c \n'), new Map([[205, 'node /usr/bin/claude -c']]));
    assert.deepEqual(parsePsColumns(''), new Map());
});

test('C-macproc.2: 셸 수와 상관없이 ps를 두 번만 실행하고, 앞쪽 그룹이 없으면 셸 자신의 명령줄이다', async () => {
    const calls = [];
    const run = async (args) => {
        calls.push(args);
        return args[1] === 'pid=,tpgid=' ? '101 205\n102 0\n' : '205 claude --resume\n102 /bin/zsh -il\n';
    };
    const result = await readMacForegrounds([101, 102, 103], run);
    assert.equal(calls.length, 2);
    assert.deepEqual(calls[0], ['-o', 'pid=,tpgid=', '-p', '101,102,103']);
    assert.deepEqual(calls[1], ['-o', 'pid=,args=', '-p', '205,102,103']);
    assert.deepEqual(result.get(101), { pid: 205, argv: ['claude', '--resume'], env: null });
    assert.deepEqual(result.get(102), { pid: 102, argv: ['/bin/zsh', '-il'], env: null });
    // 이미 끝난 셸은 명령줄이 없어 null이다.
    assert.equal(result.get(103), null);
});

test('C-macproc.3: 셸이 없으면 ps를 실행하지 않는다', async () => {
    let n_calls = 0;
    const result = await readMacForegrounds([], async () => { n_calls += 1; return ''; });
    assert.equal(n_calls, 0);
    assert.equal(result.size, 0);
});

test('C-macproc.4: 실제 ps로 이 프로세스의 명령줄을 읽는다', { skip: process.platform !== 'darwin' }, async () => {
    const result = await readMacForegrounds([process.pid]);
    assert.ok(result.get(process.pid)?.argv.length > 0);
});
