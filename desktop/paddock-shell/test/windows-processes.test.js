const test = require('node:test');
const assert = require('node:assert/strict');
const windows = require('../windows-processes');

const item = (pid, parentPid, commandLine, createdAt = pid) => ({ pid, parentPid, commandLine, createdAt });

test('C-winproc.1: 큰따옴표로 묶인 경로는 한 인자로 나눈다', () => {
    assert.deepEqual(windows.splitCommandLine('"C:\\Program Files\\nodejs\\node.exe" "C:\\a b\\cli.js" --x'), ['C:\\Program Files\\nodejs\\node.exe', 'C:\\a b\\cli.js', '--x']);
    assert.deepEqual(windows.splitCommandLine('bash.exe  --login -i'), ['bash.exe', '--login', '-i']);
    assert.deepEqual(windows.splitCommandLine(''), []);
});

test('C-winproc.2: 셸의 가장 최근 자식을 고르고 콘솔 도우미는 건너뛴다', () => {
    const processes = [
        item(10, 1, '"C:\\Program Files\\Git\\bin\\bash.exe" --login -i'),
        item(11, 10, '\\??\\C:\\WINDOWS\\system32\\conhost.exe 0x4', 50),
        item(12, 10, 'ls.exe', 12),
        item(13, 10, '"C:\\Program Files\\nodejs\\node.exe" "C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"', 13),
    ];
    assert.deepEqual(windows.foregroundArgv(processes, 10), ['C:\\Program Files\\nodejs\\node.exe', 'C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js']);
});

test('C-winproc.3: 중간 실행기(cmd·sh)는 자식으로 내려가고, 실행 중인 것이 없으면 셸 자신이다', () => {
    const processes = [
        item(20, 1, 'powershell.exe -NoLogo'),
        item(21, 20, 'C:\\WINDOWS\\system32\\cmd.exe /d /s /c "claude.cmd"'),
        item(22, 21, '"node.exe" "C:\\npm\\codex\\bin\\codex.js"'),
        item(23, 22, 'rg.exe --files'),
    ];
    assert.deepEqual(windows.foregroundArgv(processes, 20), ['node.exe', 'C:\\npm\\codex\\bin\\codex.js']);
    assert.deepEqual(windows.foregroundArgv([item(30, 1, 'cmd.exe')], 30), ['cmd.exe']);
    assert.equal(windows.foregroundArgv([], 40), null);
});
