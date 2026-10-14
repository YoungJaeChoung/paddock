const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { flagSettingsPath, buildFlagSettings, writeFlagSettings } = require('../claude-flag-settings');
const { memoryHookCommand, hasMemoryHook } = require('../memory-hook-settings');

function folder(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'flag-settings-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

test('C-flag.1: 경로는 Paddock 설정 폴더 아래 claude/flag-settings.json이다', () => {
    assert.equal(flagSettingsPath('/home/me/.paddock/config'), '/home/me/.paddock/config/claude/flag-settings.json');
    assert.equal(flagSettingsPath('/home/me/.paddock/config', path.posix.join), '/home/me/.paddock/config/claude/flag-settings.json');
});

test('C-flag.2: 켜져 있는 기능만 담고 둘 다 꺼져 있으면 빈 객체다', () => {
    assert.deepEqual(buildFlagSettings({}), {});
    assert.deepEqual(buildFlagSettings({ statusLineCommand: null, memoryHookCommand: null }), {});
    assert.deepEqual(buildFlagSettings({ statusLineCommand: 'node s.cjs u' }), { statusLine: { type: 'command', command: 'node s.cjs u' } });
    const command = memoryHookCommand('/u/memory-hook.sh', 2048);
    const memoryOnly = buildFlagSettings({ memoryHookCommand: command });
    assert.equal(hasMemoryHook(memoryOnly), true);
    assert.equal('statusLine' in memoryOnly, false);
    const both = buildFlagSettings({ statusLineCommand: 'node s.cjs u', memoryHookCommand: command });
    assert.equal(both.statusLine.command, 'node s.cjs u');
    assert.equal(hasMemoryHook(both), true);
});

test('C-flag.3: 쓰면 0600 파일이 생기고, 같은 내용은 다시 쓰지 않으며, 바뀌면 교체한다', context => {
    const file = path.join(folder(context), 'claude', 'flag-settings.json');
    assert.equal(writeFlagSettings(file, {}), 'written');
    assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), {});
    if (process.platform !== 'win32') assert.equal(fs.statSync(file).mode & 0o777, 0o600);
    const inode = fs.statSync(file).ino;
    assert.equal(writeFlagSettings(file, {}), 'unchanged');
    assert.equal(fs.statSync(file).ino, inode);
    assert.equal(writeFlagSettings(file, buildFlagSettings({ statusLineCommand: 'x' })), 'written');
    assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).statusLine.command, 'x');
    assert.deepEqual(fs.readdirSync(path.dirname(file)), ['flag-settings.json']);
});

test('C-flag.4: 대상이 심볼릭 링크면 따라가 쓰지 않고 거부한다', context => {
    const root = folder(context);
    const real = path.join(root, 'real.json');
    fs.writeFileSync(real, 'original');
    const link = path.join(root, 'flag-settings.json');
    fs.symlinkSync(real, link);
    assert.throws(() => writeFlagSettings(link, {}), /redirected/);
    assert.equal(fs.readFileSync(real, 'utf8'), 'original');
});
