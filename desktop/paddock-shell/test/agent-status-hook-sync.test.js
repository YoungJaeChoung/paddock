const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { AgentStatusHookSync } = require('../workspace/agent-status-hook');
const { AccountProfiles } = require('../account-profiles');
const { CodexHooks, releaseOwnCodexHooks, syncCodexStatusHook } = require('../codex-hooks');
const { AgentStatusHook, agentStatusHookCommand, hasAgentStatusHook } = require('../agent-status-hook-settings');

function workspace(
    { value, fetchJson },
) {
    const requests = [];
    const warnings = [];
    return {
        requests,
        warnings,
        preferences: { get: (name, fallback) => (name === 'paddock.agents.statusHook' && value !== undefined ? value : fallback) },
        fetchJson: async (...args) => { requests.push(args); return fetchJson ? fetchJson(...args) : { state: 'on' }; },
        messages: { warn: async (text) => { warnings.push(text); } },
    };
}

test('B1-sync-1: 설정이 없으면 켜진 것으로 POST하고 false면 enabled=false로 보낸다', async () => {
    const on = workspace({});
    await new AgentStatusHookSync(on).syncAll();
    assert.deepEqual(on.requests, [['/paddock/agent-status-hook', 'POST', '?enabled=true']]);
    const off = workspace({ value: false });
    await new AgentStatusHookSync(off).syncAll();
    assert.deepEqual(off.requests, [['/paddock/agent-status-hook', 'POST', '?enabled=false']]);
    assert.equal(new AgentStatusHookSync(workspace({ value: true })).isEnabled(), true);
    assert.equal(new AgentStatusHookSync(workspace({})).isPreference('paddock.agents.statusHook'), true);
    assert.equal(new AgentStatusHookSync(workspace({})).isPreference('paddock.agents.memoryNotice'), false);
});

test('B1-sync-2: 요청은 하나씩 차례로 보내고, 같은 실패는 경고를 한 번만 띄운다', async () => {
    let running = 0;
    let peak = 0;
    const failing = workspace({
        fetchJson: async () => {
            running += 1;
            peak = Math.max(peak, running);
            await new Promise(resolve => setTimeout(resolve, 10));
            running -= 1;
            throw new Error('backend is down');
        },
    });
    const sync = new AgentStatusHookSync(failing);
    await Promise.all([sync.syncAll(), sync.syncAll(), sync.syncAll()]);
    assert.equal(peak, 1, '동시에 한 요청만');
    assert.equal(failing.requests.length, 3);
    assert.deepEqual(failing.warnings, ['Agent status hook: backend is down']);
});

function codexFixture(
    context,
) {
    const home = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'paddock-codex-hook-'));
    context.after(() => fs.rmSync(home, { recursive: true, force: true }));
    const configuration = { homeDirectory: home, configDirectory: path.join(home, '.paddock', 'config'), claudeDirectory: path.join(home, '.claude'), codexDirectory: path.join(home, '.codex'), windowsUsersDirectory: path.join(home, 'Users') };
    fs.mkdirSync(configuration.codexDirectory, { recursive: true });
    fs.mkdirSync(path.join(configuration.configDirectory, 'usage'), { recursive: true });
    const accountDirectory = path.join(home, 'account-codex');
    fs.mkdirSync(accountDirectory);
    return { ...configuration, accounts: new AccountProfiles(configuration), accountDirectory, script: path.join(configuration.configDirectory, 'usage', AgentStatusHook.SCRIPT_NAME), state: path.join(configuration.configDirectory, 'usage', 'state.json') };
}

test('B1-sync-3: Codex 계정 훅 — 스크립트가 없으면 기록하지 않고, 있으면 쓰며 신뢰 안내를 돌려주고, 끄면 지운다', context => {
    const { accounts, accountDirectory, script, state } = codexFixture(context);
    const hooks = path.join(accountDirectory, 'hooks.json');
    assert.equal(accounts.syncCodexStatusHook(accountDirectory), null);
    assert.equal(fs.existsSync(hooks), false, '스크립트가 아직 없다');
    fs.writeFileSync(script, '#!/bin/sh\n');
    assert.equal(accounts.syncCodexStatusHook(accountDirectory), CodexHooks.TRUST_NOTICE);
    assert.equal(hasAgentStatusHook(JSON.parse(fs.readFileSync(hooks, 'utf8'))), true);
    assert.equal(accounts.syncCodexStatusHook(accountDirectory), null, '같은 내용이면 안내하지 않는다');
    fs.writeFileSync(state, JSON.stringify({ statusHook: { enabled: false } }));
    assert.equal(accounts.syncCodexStatusHook(accountDirectory), null);
    assert.equal(fs.existsSync(hooks), false, '끄면 지운다');
});

test('B1-sync-4: Paddock이 쓴 hooks.json은 호스트에 hooks.json이 생기면 지워 공유 링크가 대신 서게 한다. 사용자 내용이 섞인 파일은 지키고, 호스트 파일을 덮어쓰지 않는다', { skip: process.platform !== 'linux' && '링크는 Linux에서만 시험한다' }, context => {
    const { accounts, accountDirectory, script, codexDirectory } = codexFixture(context);
    fs.writeFileSync(script, '#!/bin/sh\n');
    const command = agentStatusHookCommand(script, 'codex');
    const hostFile = path.join(codexDirectory, 'hooks.json');
    // 호스트 파일이 없을 때는 계정 폴더에 우리 파일이 쓰인다.
    assert.equal(syncCodexStatusHook(accountDirectory, command), 'written');
    assert.equal(releaseOwnCodexHooks(accountDirectory, hostFile), false, '호스트 파일이 아직 없다');
    // 호스트에 파일이 생기면 우리 파일만 든 계정 파일은 지워진다.
    fs.writeFileSync(hostFile, '{"hooks":{}}\n');
    assert.equal(releaseOwnCodexHooks(accountDirectory, hostFile), true);
    assert.equal(fs.existsSync(path.join(accountDirectory, 'hooks.json')), false);
    // 그 자리에 링크가 서면 훅은 쓰지 않고 호스트 파일은 그대로다.
    fs.symlinkSync(hostFile, path.join(accountDirectory, 'hooks.json'));
    assert.equal(syncCodexStatusHook(accountDirectory, command), 'shared');
    assert.equal(fs.readFileSync(hostFile, 'utf8'), '{"hooks":{}}\n');
    // 사용자 내용이 섞인 파일은 지우지 않는다.
    fs.rmSync(path.join(accountDirectory, 'hooks.json'));
    fs.writeFileSync(path.join(accountDirectory, 'hooks.json'), JSON.stringify({ hooks: { Stop: [{ hooks: [{ type: 'command', command: 'python3 gate.py' }] }] } }));
    syncCodexStatusHook(accountDirectory, command);
    assert.equal(releaseOwnCodexHooks(accountDirectory, hostFile), false);
    assert.equal(fs.existsSync(path.join(accountDirectory, 'hooks.json')), true);
    assert.equal(accounts.codexDirectory, codexDirectory);
});

test('B1-sync-5: 계정 준비(prepareNative) 때 우리 hooks.json이 있던 계정도 호스트에 hooks.json이 생긴 뒤에는 그 파일의 링크를 받는다', { skip: process.platform !== 'linux' && '링크는 Linux에서만 시험한다' }, context => {
    const { accounts, script, codexDirectory, configDirectory } = codexFixture(context);
    fs.writeFileSync(script, '#!/bin/sh\n');
    const profile = { id: '11111111-1111-4111-8111-111111111111', provider: 'codex', runtime: 'native', label: 'Test' };
    const first = accounts.prepareNative(profile);
    const accountHooks = path.join(first.configDir, 'hooks.json');
    assert.equal(fs.lstatSync(accountHooks).isFile(), true, '호스트 파일이 없을 때는 Paddock 파일');
    assert.equal(first.warnings.includes(CodexHooks.TRUST_NOTICE), true);
    const hostFile = path.join(codexDirectory, 'hooks.json');
    fs.writeFileSync(hostFile, '{"hooks":{}}\n');
    const second = accounts.prepareNative(profile);
    assert.equal(fs.lstatSync(accountHooks).isSymbolicLink(), true, '이제 호스트 파일의 링크');
    assert.equal(fs.readFileSync(hostFile, 'utf8'), '{"hooks":{}}\n');
    assert.equal(second.warnings.includes(CodexHooks.TRUST_NOTICE), false);
    assert.equal(configDirectory.length > 0, true);
});
