const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const hook = require('../memory-hook-settings');

// 훅은 가용 메모리를 /proc/meminfo에서 읽는다. 이 파일이 없는 macOS·Windows에서는 부족 안내가 나오지 않으므로, 안내가 나와야 하는 검사를 건너뛴다.
const MEMINFO_SKIP = fs.existsSync('/proc/meminfo') ? false : '/proc/meminfo가 없는 플랫폼';

const SCRIPT = '/home/me/.paddock/config/usage/memory-hook.sh';
const COMMAND = hook.memoryHookCommand(SCRIPT, 4096);
// 사용자가 직접 만든 훅(이름이 다르다).
const USER_ENTRY = { matcher: '*', hooks: [{ type: 'command', command: 'python3 ~/.claude/hooks/mem-guard.py' }] };

test('C-memhook-set.1: 명령은 기준값을 환경 변수로 넘기고 경로를 셸 한 단어로 감싼다', () => {
    assert.equal(COMMAND, "PADDOCK_MEMORY_MIN_MB=4096 sh '/home/me/.paddock/config/usage/memory-hook.sh'");
    assert.equal(hook.memoryHookCommand("/a b/it's/memory-hook.sh", 2048), "PADDOCK_MEMORY_MIN_MB=2048 sh '/a b/it'\\''s/memory-hook.sh'");
    assert.match(hook.memoryHookCommand(SCRIPT, 'abc'), /^PADDOCK_MEMORY_MIN_MB=4096 /);
    assert.match(hook.memoryHookCommand(SCRIPT, -5), /^PADDOCK_MEMORY_MIN_MB=4096 /);
});

test('C-memhook-set.2: 넣어도 사용자의 다른 훅과 설정은 그대로 두고, 입력 객체는 바꾸지 않는다', () => {
    const settings = { theme: 'dark', hooks: { PreToolUse: [USER_ENTRY], Stop: [{ hooks: [] }] } };
    const before = JSON.stringify(settings);
    const next = hook.installMemoryHook(settings, COMMAND);
    assert.equal(JSON.stringify(settings), before);
    assert.equal(next.theme, 'dark');
    assert.deepEqual(next.hooks.Stop, settings.hooks.Stop);
    assert.deepEqual(next.hooks.PreToolUse[0], USER_ENTRY);
    assert.deepEqual(next.hooks.PreToolUse[1], { matcher: 'Bash|Agent|Task', hooks: [{ type: 'command', command: COMMAND }] });
    assert.equal(hook.hasMemoryHook(next), true);
    assert.equal(hook.hasMemoryHook(settings), false);
});

test('C-memhook-set.3: 여러 번 넣어도 한 번만 들어가고 기준값이 바뀌면 교체된다', () => {
    const once = hook.installMemoryHook({}, COMMAND);
    const twice = hook.installMemoryHook(once, COMMAND);
    assert.equal(twice.hooks.PreToolUse.length, 1);
    const changed = hook.installMemoryHook(twice, hook.memoryHookCommand(SCRIPT, 8192));
    assert.equal(changed.hooks.PreToolUse.length, 1);
    assert.match(changed.hooks.PreToolUse[0].hooks[0].command, /^PADDOCK_MEMORY_MIN_MB=8192 /);
});

test('C-memhook-set.4: 빼면 이 모듈의 훅만 사라지고, 같은 항목 안의 사용자 훅은 남는다', () => {
    const mixed = { hooks: { PreToolUse: [{ matcher: '*', hooks: [USER_ENTRY.hooks[0], { type: 'command', command: COMMAND }] }] } };
    const removed = hook.removeMemoryHook(mixed);
    assert.deepEqual(removed.hooks.PreToolUse, [{ matcher: '*', hooks: [USER_ENTRY.hooks[0]] }]);
    assert.equal(hook.hasMemoryHook(removed), false);
});

test('C-memhook-set.5: 훅만 있던 설정은 빼면 hooks 키까지 사라져 원래 모양이 된다', () => {
    const installed = hook.installMemoryHook({ theme: 'dark' }, COMMAND);
    assert.deepEqual(hook.removeMemoryHook(installed), { theme: 'dark' });
    const withOthers = hook.installMemoryHook({ hooks: { Stop: [{ hooks: [] }] } }, COMMAND);
    assert.deepEqual(hook.removeMemoryHook(withOthers), { hooks: { Stop: [{ hooks: [] }] } });
});

test('C-memhook-set.6: 넣은 훅이 없으면 빼기는 받은 객체를 그대로 돌려준다', () => {
    const settings = { hooks: { PreToolUse: [USER_ENTRY] } };
    assert.equal(hook.removeMemoryHook(settings), settings);
    const empty = {};
    assert.equal(hook.removeMemoryHook(empty), empty);
});

const SOURCE = path.join(__dirname, '..', 'memory-hook.sh');

function tempFolder() {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'memnotice-'));
}

test('C-memhook-set.7: 켜면 스크립트를 복사하고 꺼짐 표시가 있으면 지우며, 설정 파일은 만들지 않는다', () => {
    const folder = tempFolder();
    const scriptFile = path.join(folder, 'usage', 'memory-hook.sh');
    fs.mkdirSync(path.dirname(scriptFile));
    fs.writeFileSync(path.join(path.dirname(scriptFile), 'memory-hook.off'), '');
    const state = hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: true });
    assert.equal(state, 'on');
    assert.equal(fs.readFileSync(scriptFile, 'utf8'), fs.readFileSync(SOURCE, 'utf8'));
    assert.equal(fs.existsSync(path.join(path.dirname(scriptFile), 'memory-hook.off')), false);
    assert.deepEqual(fs.readdirSync(path.dirname(scriptFile)).sort(), ['memory-hook.sh']);
});

test('C-memhook-set.8: 끄면 꺼짐 표시를 두고, 스크립트는 남겨 다시 켜면 그대로 쓴다', () => {
    const folder = tempFolder();
    const scriptFile = path.join(folder, 'memory-hook.sh');
    hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: true });
    assert.equal(hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: false }), 'off');
    assert.equal(fs.existsSync(path.join(folder, 'memory-hook.off')), true);
    assert.equal(fs.existsSync(scriptFile), true);
});

test('C-memhook-set.9: 켠 적 없는 곳에서 끄면 스크립트·꺼짐 표시를 만들지 않는다', () => {
    const folder = tempFolder();
    const scriptFile = path.join(folder, 'usage', 'memory-hook.sh');
    assert.equal(hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: false }), 'off');
    assert.equal(fs.existsSync(path.dirname(scriptFile)), false);
});

test('C-memhook-set.10: 스크립트 자리가 심볼릭 링크면 따라가 쓰지 않고 거부한다', () => {
    const folder = tempFolder();
    const real = path.join(folder, 'real.sh');
    fs.writeFileSync(real, '');
    const scriptFile = path.join(folder, 'memory-hook.sh');
    fs.symlinkSync(real, scriptFile);
    assert.throws(() => hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: true }), /redirected/);
});

test('C-memhook-set.11: 꺼짐 표시를 둔 스크립트는 메모리가 부족해도 아무것도 출력하지 않는다', { skip: MEMINFO_SKIP }, () => {
    const { execFileSync } = require('node:child_process');
    const folder = tempFolder();
    const scriptFile = path.join(folder, 'memory-hook.sh');
    const run = () => execFileSync('sh', [scriptFile], {
        input: JSON.stringify({ tool_name: 'Bash' }),
        env: { ...process.env, PADDOCK_MEMORY_MIN_MB: '999999999' },
        encoding: 'utf8',
    });
    hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: true });
    assert.notEqual(run(), '');
    hook.setMemoryNotice({ scriptFile, source: SOURCE, enabled: false });
    assert.equal(run(), '');
});
