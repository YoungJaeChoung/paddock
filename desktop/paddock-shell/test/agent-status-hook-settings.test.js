const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const hook = require('../agent-status-hook-settings');
const { installMemoryHook, memoryHookCommand } = require('../memory-hook-settings');

const SCRIPT = '/home/me/.paddock/config/usage/agent-status-hook.sh';
const COMMAND = hook.agentStatusHookCommand(SCRIPT);
const USER_ENTRY = { matcher: '*', hooks: [{ type: 'command', command: 'python3 ~/.claude/hooks/review.py' }] };

test('B1-set-1: 명령은 app을 환경 변수로 넘기고 경로를 셸 한 단어로 감싼다. 모르는 app은 claude-code다', () => {
    assert.equal(COMMAND, "PADDOCK_STATUS_APP=claude-code sh '/home/me/.paddock/config/usage/agent-status-hook.sh'");
    assert.equal(hook.agentStatusHookCommand("/a b/it's/agent-status-hook.sh", 'codex'), "PADDOCK_STATUS_APP=codex sh '/a b/it'\\''s/agent-status-hook.sh'");
    assert.match(hook.agentStatusHookCommand(SCRIPT, 'x; rm -rf'), /^PADDOCK_STATUS_APP=claude-code /);
});

test('B1-set-2: 넣으면 대상 이벤트마다 훅이 하나 들어가고 사용자의 훅·설정은 그대로이며 입력 객체는 바뀌지 않는다', () => {
    const settings = { theme: 'dark', hooks: { PreToolUse: [USER_ENTRY], Stop: [USER_ENTRY] } };
    const before = JSON.stringify(settings);
    const next = hook.installAgentStatusHook(settings, COMMAND);
    assert.equal(JSON.stringify(settings), before);
    assert.equal(next.theme, 'dark');
    assert.deepEqual(Object.keys(next.hooks).sort(), [...hook.AgentStatusHook.CLAUDE_EVENTS, 'PreToolUse', 'Stop'].filter((name, index, all) => all.indexOf(name) === index).sort());
    for (const event of hook.AgentStatusHook.CLAUDE_EVENTS) {
        const ours = next.hooks[event].filter(entry => entry.hooks.some(item => item.command === COMMAND));
        assert.equal(ours.length, 1, event);
        assert.deepEqual(ours[0], { hooks: [{ type: 'command', command: COMMAND, timeout: hook.AgentStatusHook.TIMEOUT_SECONDS }] });
    }
    assert.deepEqual(next.hooks.Stop[0], USER_ENTRY);
    assert.equal(hook.hasAgentStatusHook(next), true);
    assert.equal(hook.hasAgentStatusHook(settings), false);
});

test('B1-set-3: 여러 번 넣어도 이벤트마다 한 번만 들어가고 명령이 바뀌면 교체된다', () => {
    const once = hook.installAgentStatusHook({}, COMMAND);
    const twice = hook.installAgentStatusHook(once, COMMAND);
    assert.deepEqual(twice, once);
    const changed = hook.installAgentStatusHook(twice, hook.agentStatusHookCommand('/other/agent-status-hook.sh'));
    assert.equal(changed.hooks.Stop.length, 1);
    assert.match(changed.hooks.Stop[0].hooks[0].command, /'\/other\/agent-status-hook\.sh'/);
});

test('B1-set-4: 빼면 이 모듈의 훅만 사라지고 같은 항목 안의 사용자 훅·메모리 안내 훅은 남는다. 넣지 않았으면 같은 객체를 돌려준다', () => {
    const memory = memoryHookCommand('/x/memory-hook.sh', 4096);
    const withBoth = hook.installAgentStatusHook(installMemoryHook({ hooks: { PreToolUse: [USER_ENTRY] } }, memory), COMMAND);
    const removed = hook.removeAgentStatusHook(withBoth);
    assert.equal(hook.hasAgentStatusHook(removed), false);
    assert.equal(removed.hooks.PreToolUse.length, 2);
    assert.deepEqual(removed.hooks.PreToolUse[0], USER_ENTRY);
    assert.equal(removed.hooks.PreToolUse[1].hooks[0].command, memory);
    assert.deepEqual(Object.keys(removed.hooks), ['PreToolUse']);
    const mixed = { hooks: { Stop: [{ matcher: '*', hooks: [USER_ENTRY.hooks[0], { type: 'command', command: COMMAND }] }] } };
    assert.deepEqual(hook.removeAgentStatusHook(mixed).hooks.Stop, [{ matcher: '*', hooks: [USER_ENTRY.hooks[0]] }]);
    // 설정이 원래 모양으로 돌아가고, 넣은 것이 없으면 같은 참조다.
    assert.deepEqual(hook.removeAgentStatusHook(hook.installAgentStatusHook({ theme: 'dark' }, COMMAND)), { theme: 'dark' });
    const plain = { hooks: { Stop: [USER_ENTRY] } };
    assert.equal(hook.removeAgentStatusHook(plain), plain);
});

test('B1-set-5: 스크립트 복사와 꺼짐 표시 파일을 맞춘다. 켠 적 없는 곳을 끄면 아무것도 만들지 않는다', () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-status-set-'));
    try {
        const source = path.join(directory, 'source.sh');
        fs.writeFileSync(source, '#!/bin/sh\nexit 0\n');
        const scriptFile = path.join(directory, 'usage', 'agent-status-hook.sh');
        const marker = path.join(directory, 'usage', 'agent-status-hook.off');
        assert.equal(hook.setAgentStatusHook({ scriptFile, source, enabled: false }), 'off');
        assert.equal(fs.existsSync(path.join(directory, 'usage')), false);
        assert.equal(hook.setAgentStatusHook({ scriptFile, source, enabled: true }), 'on');
        assert.equal(fs.readFileSync(scriptFile, 'utf8'), '#!/bin/sh\nexit 0\n');
        assert.equal(fs.existsSync(marker), false);
        assert.equal(hook.setAgentStatusHook({ scriptFile, source, enabled: false }), 'off');
        assert.equal(fs.existsSync(marker), true);
        assert.equal(hook.setAgentStatusHook({ scriptFile, source, enabled: true }), 'on');
        assert.equal(fs.existsSync(marker), false);
    } finally {
        fs.rmSync(directory, { recursive: true, force: true });
    }
});
