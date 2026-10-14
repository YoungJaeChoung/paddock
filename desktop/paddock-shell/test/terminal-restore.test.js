const test = require('node:test');
const assert = require('node:assert/strict');
const { planRestore, noticeLine, waitForPrompt, withoutWslCd } = require('../terminal-restore');
const { accountTerminalOptions } = require('../account-launch');

const SESSION = '0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10';
const toUri = path => `file://${path}`;
const record = (extra = {}) => ({
    version: 1, output: 'screen', cwdNow: '/work/app/sub', agent: null, foreground: null,
    launcher: { command: '/bin/zsh', args: ['-l'], cwd: '/work/app', env: { CODEX_HOME: '/c' } },
    ...extra,
});

test('일반 터미널: 셸 옵션이 없으면 기록의 셸로 채우고 시작 폴더를 현재 폴더로 바꾼다', () => {
    const plan = planRestore({ options: { id: 'terminal-1', env: { WSLENV: 'x' } }, record: record(), toUri });
    assert.equal(plan.options.shellPath, '/bin/zsh');
    assert.deepEqual(plan.options.shellArgs, ['-l']);
    assert.deepEqual(plan.options.env, { CODEX_HOME: '/c', WSLENV: 'x' });
    assert.equal(plan.options.cwd, 'file:///work/app/sub');
    assert.equal(plan.options.id, 'terminal-1');
    assert.equal(plan.command, null);
    assert.equal(plan.notice, 'Previous shell ended. Screen restored; commands that were running were stopped.');
});

test('일반 터미널: 이미 셸 옵션이 있으면 셸은 그대로 두고 폴더만 바꾼다', () => {
    const plan = planRestore({ options: { shellPath: 'C:\\wsl.exe', shellArgs: ['-d', 'Ubuntu'], cwd: 'file:///first' }, record: record(), toUri });
    assert.equal(plan.options.shellPath, 'C:\\wsl.exe');
    assert.deepEqual(plan.options.shellArgs, ['-d', 'Ubuntu']);
    assert.equal(plan.options.cwd, 'file:///work/app/sub');
});

test('일반 터미널: 현재 폴더 기록이 없으면 시작 폴더를 건드리지 않는다', () => {
    const plan = planRestore({ options: { cwd: 'file:///first' }, record: record({ cwdNow: '' }), toUri });
    assert.equal(plan.options.cwd, 'file:///first');
});

test('일반 터미널: Claude·Codex 대화가 있으면 새 셸에 입력할 이어하기 명령을 만든다', () => {
    const claude = planRestore({ options: {}, record: record({ agent: { provider: 'claude', sessionId: SESSION, cwd: '/w' }, foreground: 'claude' }), toUri });
    assert.equal(claude.command, `claude --resume ${SESSION}`);
    assert.match(claude.notice, /Continuing the Claude conversation\.$/);
    const codex = planRestore({ options: {}, record: record({ agent: { provider: 'codex', sessionId: SESSION, cwd: '/w' }, foreground: 'codex' }), toUri });
    assert.equal(codex.command, `codex resume ${SESSION}`);
    assert.match(codex.notice, /Continuing the Codex conversation\.$/);
});

test('에이전트가 떠 있었는데 대화를 모르면 이어하지 않고 직접 이어하는 명령을 안내한다', () => {
    const plan = planRestore({ options: {}, record: record({ foreground: 'codex' }), toUri });
    assert.equal(plan.command, null);
    assert.match(plan.notice, /Codex was running but its conversation could not be identified; run `codex resume` to choose one\.$/);
    const unknownProvider = planRestore({ options: {}, record: record({ agent: { provider: 'gemini', sessionId: SESSION }, foreground: 'gemini' }), toUri });
    assert.equal(unknownProvider.command, null);
    assert.equal(unknownProvider.notice, 'Previous shell ended. Screen restored; commands that were running were stopped.');
});

test('계정 터미널: Claude 대화가 있으면 계정 시작 옵션에 --resume <id>를 넣고 입력 명령은 없다', () => {
    const profile = { id: 'a1', provider: 'claude', label: 'Work', runtime: 'native' };
    const launched = accountTerminalOptions({ profile, configDir: '/accounts/work' }, { cwd: '/work/app', isWindows: false });
    const plan = planRestore({ options: { ...launched, cwd: 'file:///work/app' }, record: record({ agent: { provider: 'claude', sessionId: SESSION, cwd: '/w' }, foreground: 'claude' }), toUri });
    assert.equal(plan.command, null);
    assert.ok(plan.options.shellArgs.join('\n').includes(SESSION));
    assert.equal(plan.options.cwd, 'file:///work/app');
    assert.equal(plan.options.env.CLAUDE_CONFIG_DIR, '/accounts/work');
    assert.match(plan.notice, /Continuing the Claude conversation\.$/);
});

test('계정 터미널: 대화가 없거나 Codex면 시작 옵션을 그대로 둔다', () => {
    const profile = { id: 'a1', provider: 'codex', label: 'Work', runtime: 'native' };
    const launched = accountTerminalOptions({ profile, configDir: '/accounts/work' }, { cwd: '/work/app', isWindows: false });
    const plan = planRestore({ options: launched, record: record({ agent: { provider: 'codex', sessionId: SESSION, cwd: '/w' }, foreground: 'codex' }), toUri });
    assert.deepEqual(plan.options.shellArgs, launched.shellArgs);
    assert.equal(plan.command, null);
    assert.match(plan.notice, /Codex was running but its conversation could not be identified/);
});

test('noticeLine: 줄을 바꾸고 흐린 한 줄로 쓴다', () => {
    assert.equal(noticeLine('hello'), '\x1b[0m\r\n\x1b[2m— hello —\x1b[0m\r\n');
});

test('WSL 터미널: Windows 홈의 --cd ~가 복원할 폴더를 덮지 않게 --cd 쌍을 걷어 내고 배포판 인자는 남긴다', () => {
    assert.deepEqual(withoutWslCd(['--cd', '~', '-d', 'Ubuntu']), ['-d', 'Ubuntu']);
    assert.deepEqual(withoutWslCd(['-d', 'Ubuntu', '--cd=/tmp']), ['-d', 'Ubuntu']);
    assert.deepEqual(withoutWslCd(['-d', 'Ubuntu']), ['-d', 'Ubuntu']);
    const options = { shellPath: 'C:\\Windows\\System32\\wsl.exe', shellArgs: ['--cd', '~', '-d', 'Ubuntu'], cwd: 'file:///C:/Users/me' };
    const plan = planRestore({ options, record: record({ cwdNow: '\\\\wsl.localhost\\Ubuntu\\home\\me\\app' }), toUri });
    assert.deepEqual(plan.options.shellArgs, ['-d', 'Ubuntu']);
    assert.equal(plan.options.cwd, 'file://\\\\wsl.localhost\\Ubuntu\\home\\me\\app');
    // 복원할 폴더가 없으면 인자를 건드리지 않는다. WSL이 아닌 셸의 --cd도 건드리지 않는다.
    assert.deepEqual(planRestore({ options, record: record({ cwdNow: '' }), toUri }).options.shellArgs, ['--cd', '~', '-d', 'Ubuntu']);
    assert.deepEqual(planRestore({ options: { shellPath: '/bin/zsh', shellArgs: ['--cd', 'x'] }, record: record(), toUri }).options.shellArgs, ['--cd', 'x']);
});

test('이어하기 명령: 계정 폴더를 지정해 실행했으면 POSIX 셸에서만 같은 폴더를 앞붙인다', () => {
    const agent = { provider: 'claude', sessionId: SESSION, cwd: '/w', configDir: "/acc/it's" };
    const posix = planRestore({ options: { shellPath: '/bin/bash' }, record: record({ agent, foreground: 'claude' }), toUri });
    assert.equal(posix.command, `CLAUDE_CONFIG_DIR='/acc/it'"'"'s' claude --resume ${SESSION}`);
    const wsl = planRestore({ options: { shellPath: 'C:\\Windows\\System32\\wsl.exe' }, record: record({ agent, foreground: 'claude' }), toUri });
    assert.match(wsl.command, /^CLAUDE_CONFIG_DIR=/);
    const powershell = planRestore({ options: { shellPath: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' }, record: record({ agent, foreground: 'claude' }), toUri });
    assert.equal(powershell.command, `claude --resume ${SESSION}`);
});

test('waitForPrompt: 출력이 멎고 자식 프로세스가 없을 때만 ready, 자식이 있으면 busy, 계속 출력하면 timeout', async () => {
    const fake = () => {
        const clock = { time: 0, now: () => clock.time, sleep: async ms => { clock.time += ms; } };
        return clock;
    };
    let clock = fake();
    assert.equal(await waitForPrompt({ lastWriteAt: () => (clock.time >= 300 ? 300 : null), hasChildren: async () => false, clock }), 'ready');
    clock = fake();
    assert.equal(await waitForPrompt({ lastWriteAt: () => 0, hasChildren: async () => true, clock }), 'busy');
    clock = fake();
    assert.equal(await waitForPrompt({ lastWriteAt: () => clock.time, hasChildren: async () => false, clock }), 'timeout');
    clock = fake();
    assert.equal(await waitForPrompt({ lastWriteAt: () => null, hasChildren: async () => false, clock }), 'timeout');
});
