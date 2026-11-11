const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { CodexHooks, syncCodexStatusHook } = require('../codex-hooks');
const { agentStatusHookCommand, hasAgentStatusHook } = require('../agent-status-hook-settings');

const COMMAND = agentStatusHookCommand('/h/.paddock/config/usage/agent-status-hook.sh', 'codex');

function folder(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'codex-hooks-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

test('B1-codex-1: hooks.json이 없으면 새로 쓰고 Codex 이벤트마다 app=codex 훅을 넣는다. 다시 불러도 그대로다', context => {
    const home = path.join(folder(context), 'codex');
    assert.equal(syncCodexStatusHook(home, COMMAND), 'written');
    const written = JSON.parse(fs.readFileSync(path.join(home, 'hooks.json'), 'utf8'));
    assert.deepEqual(Object.keys(written.hooks).sort(), [...CodexHooks.EVENTS].sort());
    assert.match(written.hooks.Interrupt[0].hooks[0].command, /^PADDOCK_STATUS_APP=codex sh /);
    if (process.platform !== 'win32') assert.equal(fs.statSync(path.join(home, 'hooks.json')).mode & 0o777, 0o600);
    assert.equal(syncCodexStatusHook(home, COMMAND), 'unchanged');
});

test('B1-codex-2: 계정 폴더의 자기 hooks.json 안 사용자 훅은 지키고, 끄면 우리 훅만 뺀다. 우리만 있었으면 파일을 지운다', context => {
    const home = folder(context);
    const file = path.join(home, 'hooks.json');
    const user = { hooks: { Stop: [{ hooks: [{ type: 'command', command: 'python3 gate.py' }] }] } };
    fs.writeFileSync(file, JSON.stringify(user));
    assert.equal(syncCodexStatusHook(home, COMMAND), 'written');
    const merged = JSON.parse(fs.readFileSync(file, 'utf8'));
    assert.equal(merged.hooks.Stop.length, 2);
    assert.deepEqual(merged.hooks.Stop[0], user.hooks.Stop[0]);
    assert.equal(syncCodexStatusHook(home, null), 'removed');
    assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), user);
    const own = path.join(folder(context), 'codex');
    syncCodexStatusHook(own, COMMAND);
    assert.equal(syncCodexStatusHook(own, null), 'removed');
    assert.equal(fs.existsSync(path.join(own, 'hooks.json')), false);
    // 켠 적 없는 폴더를 끄면 아무것도 만들지 않는다.
    const untouched = path.join(folder(context), 'never');
    assert.equal(syncCodexStatusHook(untouched, null), 'unchanged');
    assert.equal(fs.existsSync(untouched), false);
});

test('B1-codex-3: hooks.json이 일반 ~/.codex의 파일을 가리키는 링크면 쓰지 않는다', { skip: process.platform === 'win32' && '심볼릭 링크 권한이 필요하다' }, context => {
    const directory = folder(context);
    const host = path.join(directory, 'host-hooks.json');
    fs.writeFileSync(host, '{"hooks":{}}\n');
    const home = path.join(directory, 'account');
    fs.mkdirSync(home);
    fs.symlinkSync(host, path.join(home, 'hooks.json'));
    assert.equal(syncCodexStatusHook(home, COMMAND), 'shared');
    assert.equal(fs.readFileSync(host, 'utf8'), '{"hooks":{}}\n');
    assert.equal(syncCodexStatusHook(home, null), 'shared');
});

test('B1-codex-4: JSON이 아닌 hooks.json은 사용자의 내용을 지키려고 쓰지 않는다', context => {
    const home = folder(context);
    fs.writeFileSync(path.join(home, 'hooks.json'), 'not json');
    assert.equal(syncCodexStatusHook(home, COMMAND), 'invalid');
    assert.equal(fs.readFileSync(path.join(home, 'hooks.json'), 'utf8'), 'not json');
    assert.equal(hasAgentStatusHook({}), false);
});
