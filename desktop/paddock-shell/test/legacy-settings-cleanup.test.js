const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { withoutPaddockEntries, cleanSettingsFile, cleanSettingsFiles, describeCleanup } = require('../legacy-settings-cleanup');
const { memoryHookCommand, installMemoryHook } = require('../memory-hook-settings');

const OLD_STATUS_LINE = { type: 'command', command: '"/usr/local/bin/node" "/Users/me/.paddock/config/usage/claude-statusline.cjs" "/Users/me/.paddock/config/usage/claude.json" "/Users/me/.paddock/config/usage/previous-statusline.json"' };
const USER_HOOK = { matcher: 'Agent|Bash', hooks: [{ type: 'command', command: 'python3 ~/.claude/hooks/mem-guard.py' }] };

function folder(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'legacy-cleanup-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

test('LC01 only the entries that name the Paddock scripts are removed; the same object comes back when there is nothing to remove', () => {
    const mine = { theme: 'dark', statusLine: OLD_STATUS_LINE };
    assert.deepEqual(withoutPaddockEntries(mine), { settings: { theme: 'dark' }, removed: ['status line'] });
    const userLine = { statusLine: { type: 'command', command: 'npx ccstatusline' }, hooks: { PreToolUse: [USER_HOOK] } };
    const untouched = withoutPaddockEntries(userLine);
    assert.equal(untouched.settings, userLine);
    assert.deepEqual(untouched.removed, []);
    const withHook = installMemoryHook({ hooks: { PreToolUse: [USER_HOOK] } }, memoryHookCommand('/home/me/.paddock/config/usage/memory-hook.sh', 2048));
    const cleaned = withoutPaddockEntries({ ...withHook, statusLine: OLD_STATUS_LINE });
    assert.deepEqual(cleaned.removed, ['status line', 'memory hook']);
    assert.deepEqual(cleaned.settings, { hooks: { PreToolUse: [USER_HOOK] } });
});

test('LC02 the status line the old entry replaced is put back when it is readable and not itself a Paddock line', () => {
    const settings = { statusLine: OLD_STATUS_LINE };
    const original = { type: 'command', command: 'my-line', padding: 1 };
    assert.deepEqual(withoutPaddockEntries(settings, original).settings, { statusLine: original });
    // The restored line keeps the position of the entry it replaces.
    assert.deepEqual(Object.keys(withoutPaddockEntries({ a: 1, statusLine: OLD_STATUS_LINE, z: 2 }, original).settings), ['a', 'statusLine', 'z']);
    assert.deepEqual(withoutPaddockEntries(settings, OLD_STATUS_LINE).settings, {});
    assert.deepEqual(withoutPaddockEntries(settings, { type: 'text' }).settings, {});
});

test('LC03 a file is rewritten with its own indentation and permissions, and nothing else in it changes', { skip: process.platform === 'win32' }, context => {
    const directory = folder(context);
    const file = path.join(directory, 'settings.json');
    fs.writeFileSync(file, `{\n    "theme": "dark",\n    "statusLine": ${JSON.stringify(OLD_STATUS_LINE)},\n    "permissions": {"allow": ["Bash(ls:*)"]}\n}\n`, { mode: 0o640 });
    const result = cleanSettingsFile({ file });
    assert.deepEqual(result.removed, ['status line']);
    assert.equal(fs.readFileSync(file, 'utf8'), '{\n    "theme": "dark",\n    "permissions": {\n        "allow": [\n            "Bash(ls:*)"\n        ]\n    }\n}\n');
    assert.equal(fs.statSync(file).mode & 0o777, 0o640);
    assert.deepEqual(fs.readdirSync(directory), ['settings.json']);
    assert.deepEqual(cleanSettingsFile({ file }).removed, [], 'a second run finds nothing');
});

test('LC04 a linked settings file is edited at its real location and the link stays', { skip: process.platform === 'win32' }, context => {
    const directory = folder(context);
    const repository = path.join(directory, 'repo', 'claude', 'settings.json');
    fs.mkdirSync(path.dirname(repository), { recursive: true });
    fs.writeFileSync(repository, JSON.stringify({ statusLine: OLD_STATUS_LINE, model: 'opus' }, null, 2));
    const file = path.join(directory, '.claude', 'settings.json');
    fs.mkdirSync(path.dirname(file));
    fs.symlinkSync(repository, file);
    const result = cleanSettingsFile({ file });
    assert.equal(result.isLinked, true);
    assert.equal(result.target, repository);
    assert.equal(fs.lstatSync(file).isSymbolicLink(), true);
    assert.equal(fs.readlinkSync(file), repository);
    assert.deepEqual(JSON.parse(fs.readFileSync(repository, 'utf8')), { model: 'opus' });
    assert.equal(fs.readFileSync(repository, 'utf8').endsWith('\n'), false, 'no trailing newline was added');
    assert.deepEqual(describeCleanup([result]), [`Removed the old Paddock status line from ${file} (a link to ${repository}). That repository file changed: upload it, for example with sync.sh.`]);
});

test('LC05 a dry run reports the removal without writing; missing, unreadable, and non-object files are left alone', context => {
    const directory = folder(context);
    const file = path.join(directory, 'settings.json');
    const text = JSON.stringify({ statusLine: OLD_STATUS_LINE });
    fs.writeFileSync(file, text);
    assert.deepEqual(cleanSettingsFile({ file, dryRun: true }).removed, ['status line']);
    assert.equal(fs.readFileSync(file, 'utf8'), text);
    assert.deepEqual(cleanSettingsFile({ file: path.join(directory, 'absent.json') }).removed, []);
    for (const broken of ['{ broken', '[]']) {
        fs.writeFileSync(file, broken);
        const result = cleanSettingsFile({ file });
        assert.deepEqual(result.removed, []);
        assert.notEqual(result.isSkipped, '');
        assert.equal(fs.readFileSync(file, 'utf8'), broken);
    }
});

test('LC06 the replaced status line comes back from its record file', context => {
    const directory = folder(context);
    const record = path.join(directory, 'previous-statusline.json');
    fs.writeFileSync(record, JSON.stringify({ type: 'command', command: 'my-line' }));
    const file = path.join(directory, 'settings.json');
    const command = `"/usr/bin/node" "${directory}/claude-statusline.cjs" "${directory}/claude.json" "${record}"`;
    fs.writeFileSync(file, JSON.stringify({ statusLine: { type: 'command', command } }));
    const readPrevious = candidate => (candidate === record ? JSON.parse(fs.readFileSync(candidate, 'utf8')) : null);
    assert.deepEqual(cleanSettingsFile({ file, readPrevious }).removed, ['status line']);
    assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), { statusLine: { type: 'command', command: 'my-line' } });
});

test('LC07 several entries reaching the same real file are handled once, and one failing file does not stop the others', { skip: process.platform === 'win32' }, context => {
    const directory = folder(context);
    const shared = path.join(directory, 'repo', 'settings.json');
    fs.mkdirSync(path.dirname(shared));
    fs.writeFileSync(shared, JSON.stringify({ statusLine: OLD_STATUS_LINE }));
    const links = ['a', 'b'].map(name => {
        const link = path.join(directory, name, 'settings.json');
        fs.mkdirSync(path.dirname(link));
        fs.symlinkSync(shared, link);
        return link;
    });
    const other = path.join(directory, 'other.json');
    fs.writeFileSync(other, JSON.stringify({ statusLine: OLD_STATUS_LINE, theme: 'x' }));
    const results = cleanSettingsFiles([
        { file: links[0] },
        { file: links[1] },
        { file: path.join(directory, 'absent.json') },
        { file: other, resolveLink: () => { throw new Error('fixture failure'); } },
        { file: other },
    ]);
    assert.deepEqual(results.map(result => result.removed.length), [1, 0, 0, 1], 'the second link to the same file is not reported again');
    assert.equal(results.find(result => result.isSkipped === 'fixture failure')?.file, other);
    assert.deepEqual(JSON.parse(fs.readFileSync(other, 'utf8')), { theme: 'x' });
    assert.equal(describeCleanup(results).length, 2);
});
