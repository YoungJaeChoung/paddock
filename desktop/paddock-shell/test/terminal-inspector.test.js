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
        assert.equal(await currentFolder(shell.pid), directory);
        // 셸에 터미널이 없어 앞쪽 프로세스 그룹은 없고, 셸이 exec로 바꾼 sleep 자신이 보인다.
        assert.ok(foregroundProcess(shell.pid).argv.length > 0);
        const inspector = createTerminalInspector({ homeDirectory: directory });
        const info = await inspector.inspect({ pid: shell.pid, options: { options: { env: {} } } });
        assert.equal(info.cwd, directory);
        assert.equal(info.agent, null);
    } finally {
        shell.kill();
    }
});

const mac = { skip: process.platform !== 'darwin' ? 'macOS의 ps·lsof를 쓰는 시험이다' : false };

test('macOS: 셸의 현재 폴더와 앞쪽 프로그램을 ps·lsof로 기다리지 않고 읽는다', mac, async () => {
    const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-inspect-')));
    const shell = childProcess.spawn('sh', ['-c', 'sleep 30'], { cwd: directory, stdio: 'ignore' });
    try {
        await new Promise(resolve => setTimeout(resolve, 200));
        const inspector = createTerminalInspector({ homeDirectory: directory });
        const pending = inspector.inspect({ pid: shell.pid, options: { options: { env: {} } } });
        // 결과는 Promise로 와서, ps·lsof가 끝날 때까지 백엔드가 멈추지 않는다.
        assert.ok(pending instanceof Promise);
        const info = await pending;
        assert.equal(info.cwd, directory);
        assert.equal(info.agent, null);
    } finally {
        shell.kill();
    }
});

test('WSL 터미널은 스캔 결과의 폴더와 Claude 대화를 Linux 셸과 같은 판정으로 읽고, 스캔에 없으면 null이다', async () => {
    const inspector = createTerminalInspector();
    const terminal = { options: { options: { env: { PADDOCK_TERMINAL: 'terminal-1' } } } };
    const ID = '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10';
    const cwd = '\\\\wsl.localhost\\Ubuntu\\home\\me\\app';
    const record = (extra = {}) => JSON.stringify({ pid: 55, sessionId: ID, kind: 'interactive', ...extra });
    assert.equal(await inspector.inspect(terminal), null);
    inspector.noteWsl({ 'terminal-1': { cwd, program: 'claude', pid: '55', sessionRecord: record(), configDir: '/home/me/acc' } });
    assert.deepEqual(await inspector.inspect(terminal), { cwd, agent: { provider: 'claude', sessionId: ID, cwd, configDir: '/home/me/acc' }, foreground: 'claude' });
    // 비대화형(claude -p)·pid 불일치·깨진 기록·다른 프로그램은 대화로 보지 않는다.
    for (const entry of [
        { program: 'claude', pid: '55', sessionRecord: record({ kind: 'sdk' }) },
        { program: 'claude', pid: '56', sessionRecord: record() },
        { program: 'claude', pid: '55', sessionRecord: '{broken' },
        { program: 'bash', pid: '55', sessionRecord: record() },
    ]) {
        inspector.noteWsl({ 'terminal-1': { cwd, ...entry } });
        assert.equal((await inspector.inspect(terminal)).agent, null);
    }
});
