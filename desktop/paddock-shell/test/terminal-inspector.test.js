const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const childProcess = require('node:child_process');
const { createTerminalInspector, foregroundProcess, currentFolder } = require('../terminal-inspector');

const linux = { skip: process.platform !== 'linux' ? 'Linux의 /proc을 쓰는 시험이다' : false };

test('Linux: 셸의 현재 폴더와 앞쪽 프로그램을 /proc에서 읽는다', linux, async () => {
    const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-inspect-')));
    const shell = childProcess.spawn('sh', ['-c', 'sleep 30'], { cwd: directory, stdio: 'ignore' });
    try {
        await new Promise(resolve => setTimeout(resolve, 200));
        assert.equal(currentFolder(shell.pid), directory);
        // 셸에 터미널이 없어 앞쪽 프로세스 그룹은 없고, 셸이 exec로 바꾼 sleep 자신이 보인다.
        assert.ok(foregroundProcess(shell.pid).argv.length > 0);
        const inspector = createTerminalInspector({ homeDirectory: directory });
        const info = inspector.inspect({ pid: shell.pid, options: { options: { env: {} } } });
        assert.equal(info.cwd, directory);
        assert.equal(info.agent, null);
    } finally {
        shell.kill();
    }
});

test('WSL 터미널은 스캔 결과의 폴더와 Claude 대화를 쓰고, 스캔에 없으면 null이다', () => {
    const inspector = createTerminalInspector();
    const terminal = { options: { options: { env: { PADDOCK_TERMINAL: 'terminal-1' } } } };
    assert.equal(inspector.inspect(terminal), null);
    inspector.noteWsl({ 'terminal-1': { cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app', program: 'claude', sessionId: '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10' } });
    assert.deepEqual(inspector.inspect(terminal), {
        cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
        agent: { provider: 'claude', sessionId: '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10', cwd: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app' },
        foreground: 'claude',
    });
    inspector.noteWsl({ 'terminal-1': { cwd: '/x', program: 'bash', sessionId: '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10' } });
    assert.equal(inspector.inspect(terminal).agent, null);
    inspector.noteWsl({ 'terminal-1': { cwd: '/x', program: 'claude', sessionId: 'bad' } });
    assert.equal(inspector.inspect(terminal).agent, null);
});
