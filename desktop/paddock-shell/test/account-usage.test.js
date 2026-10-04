const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { AccountProfiles, AccountProfile } = require('../account-profiles');
const { AccountUsage, wslFileMapping } = require('../account-usage');
const { applyStatusLine } = require('../claude-usage-settings');
const usage = require('../usage-model');

function fixture(
    context,
) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-account-usage-'));
    const accounts = new AccountProfiles({ configDirectory: path.join(directory, 'app-config'), homeDirectory: directory, claudeDirectory: path.join(directory, '.claude'), codexDirectory: path.join(directory, '.codex') });
    const sourceScript = path.join(__dirname, '..', 'claude-statusline.cjs');
    const commandForNative = (...files) => [process.execPath, ...files].map(file => usage.quoteStatusLineArgument(file, process.platform === 'win32')).join(' ');
    const liveCodexUsage = { read: async () => ({ windows: [], updatedAt: null }) };
    const reader = new AccountUsage({ accounts, sourceScript, commandForNative, liveCodexUsage, readWslInfo: async () => { throw new Error('Native usage must not query WSL.'); } });
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return { directory, accounts, reader, sourceScript, commandForNative, liveCodexUsage };
}

function writeRecord(
    file,
    record,
) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify(record));
}

function codexRecord(
    used,
) {
    return { type: 'event_msg', payload: { type: 'token_count', rate_limits: { primary: { used_percent: used, window_minutes: 300, resets_at: 1999999999 } } } };
}

test('AU01 a registered account with no records remains empty and lookup creates no usage files', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    assert.equal(fs.existsSync(scope.usageDirectory), false);
    const value = await reader.read(profile.id);
    assert.deepEqual(value, { claude: { state: 'unset', windows: [], updatedAt: null }, codex: { windows: [], updatedAt: null } });
    assert.equal(fs.existsSync(scope.usageDirectory), false);
});

test('AU02 each Claude account reads only its own summary and never fills Codex with a default account', async context => {
    const { directory, accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'claude', label: 'First' });
    const second = await accounts.create({ provider: 'claude', label: 'Second' });
    const firstScope = await reader.resolve(first.id);
    const secondScope = await reader.resolve(second.id);
    writeRecord(path.join(directory, '.paddock', 'config', 'usage', 'claude.json'), { rate_limits: { five_hour: { used_percentage: 99 } }, updated_at: 1000 });
    writeRecord(path.join(firstScope.usageDirectory, 'state.json'), { claude: 'on' });
    writeRecord(path.join(firstScope.usageDirectory, 'claude.json'), { rate_limits: { five_hour: { used_percentage: 12, resets_at: 9999 } }, updated_at: 100 });
    writeRecord(path.join(secondScope.usageDirectory, 'claude.json'), { rate_limits: { five_hour: { used_percentage: 43, resets_at: 9999 } }, updated_at: 200 });
    const one = await reader.read(first.id);
    const two = await reader.read(second.id);
    assert.equal(one.claude.windows[0].used, 12);
    assert.equal(two.claude.windows[0].used, 43);
    assert.equal(one.claude.state, 'on');
    assert.deepEqual(one.codex, { windows: [], updatedAt: null });
});

test('AU03 Codex reads its own session limits and never returns conversation content or the other provider', async context => {
    const { accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'codex' });
    const second = await accounts.create({ provider: 'codex' });
    const firstScope = await reader.resolve(first.id);
    const secondScope = await reader.resolve(second.id);
    const session = path.join(firstScope.sessionsDirectory, '2026', '10', '05', 'rollout-fixture.jsonl');
    writeRecord(session, codexRecord(21));
    fs.appendFileSync(session, '\n{"type":"message","text":"fixture conversation must stay local"}\n');
    writeRecord(path.join(secondScope.sessionsDirectory, '2026', '10', '05', 'rollout-other.jsonl'), codexRecord(89));
    const value = await reader.read(first.id);
    assert.equal(value.codex.windows[0].used, 21);
    assert.deepEqual(value.claude, { state: 'unset', windows: [], updatedAt: null });
    assert.equal(JSON.stringify(value).includes('fixture conversation'), false);
});

test('AU04 malformed, empty, duplicate-query, and removed identifiers never become default requests', async context => {
    const { accounts, reader } = fixture(context);
    for (const id of ['', undefined, ['first', 'second'], '../.claude', { id: 'anything' }]) {
        await assert.rejects(reader.read(id), error => error.statusCode === 400);
    }
    const profile = await accounts.create({ provider: 'claude' });
    await accounts.remove(profile.id);
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 404);
    await assert.rejects(reader.setClaude(profile.id, true), error => error.statusCode === 404);
    const codex = await accounts.create({ provider: 'codex' });
    await assert.rejects(reader.setClaude(codex.id, true), error => error.statusCode === 400);
});

test('AU05 WSL lookup uses the saved distribution and maps spaced Linux homes without preparing a terminal', async context => {
    const setup = fixture(context);
    const profile = { id: '00000000-0000-4000-8000-000000000001', provider: 'codex', label: 'WSL', runtime: 'wsl', wslDistribution: 'Debian Saved' };
    writeRecord(path.join(setup.accounts.configDirectory, AccountProfile.METADATA_FILE), { version: 1, profiles: [profile] });
    const calls = [];
    const reader = new AccountUsage({ ...setup, readWslInfo: async distribution => {
        calls.push(distribution);
        return { ready: true, linuxHome: '/home/person with space', home: '\\\\wsl.localhost\\Debian Saved\\home\\person with space', node: '/usr/bin/node' };
    } });
    const scope = await reader.resolve(profile.id);
    assert.deepEqual(calls, ['Debian Saved']);
    assert.equal(scope.configDir, `/home/person with space/.paddock/agent-profiles/${profile.id}/codex`);
    assert.equal(scope.toLocal(scope.sessionsDirectory), `\\\\wsl.localhost\\Debian Saved\\home\\person with space\\.paddock\\agent-profiles\\${profile.id}\\codex\\sessions`);
    assert.equal(fs.existsSync(path.join(setup.accounts.configDirectory, 'agent-profiles')), false);
    assert.throws(() => wslFileMapping({ ready: true, linuxHome: '/home/user', home: '\\\\wsl.localhost\\Ubuntu\\home\\user' }, 'Debian Saved'), error => error.statusCode === 503);
    assert.throws(() => wslFileMapping({ ready: false }, 'Debian Saved'), error => error.statusCode === 503);
});

test('AU06 Claude setup preserves the existing status line, updates only one account, and restores it when disabled', async context => {
    const { accounts, reader } = fixture(context);
    const first = await accounts.create({ provider: 'claude' });
    const second = await accounts.create({ provider: 'claude' });
    const firstScope = await reader.resolve(first.id);
    const secondScope = await reader.resolve(second.id);
    const settings = { theme: 'dark', statusLine: { type: 'command', command: 'printf original', padding: 2 } };
    writeRecord(firstScope.settingsPath, settings);
    writeRecord(secondScope.settingsPath, { theme: 'light' });
    assert.deepEqual(await reader.setClaude(first.id, true), { state: 'on' });
    assert.deepEqual(await reader.setClaude(first.id, true, true), { state: 'on' });
    const previousFile = path.join(firstScope.usageDirectory, 'previous-statusline.json');
    assert.deepEqual(JSON.parse(fs.readFileSync(previousFile, 'utf8')), settings.statusLine);
    assert.equal((await reader.read(first.id)).claude.state, 'on');
    assert.deepEqual(JSON.parse(fs.readFileSync(secondScope.settingsPath, 'utf8')), { theme: 'light' });
    assert.equal(fs.existsSync(secondScope.usageDirectory), false);
    assert.deepEqual(await reader.setClaude(first.id, false), { state: 'off' });
    assert.deepEqual(JSON.parse(fs.readFileSync(firstScope.settingsPath, 'utf8')), settings);
    assert.deepEqual(JSON.parse(fs.readFileSync(`${firstScope.settingsPath}.paddock-backup`, 'utf8')), settings);
    assert.equal(fs.existsSync(previousFile), false);
});

test('AU07 disabling collection preserves a status line changed by the user after installation', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    await reader.setClaude(profile.id, true);
    const changed = { statusLine: { type: 'command', command: 'printf new-user-status' } };
    writeRecord(scope.settingsPath, changed);
    await reader.setClaude(profile.id, false);
    assert.deepEqual(JSON.parse(fs.readFileSync(scope.settingsPath, 'utf8')), changed);
});

test('AU08 the account status script saves only limit fields in that account', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    await reader.setClaude(profile.id, true);
    const result = spawnSync(process.execPath, [path.join(scope.usageDirectory, 'claude-statusline.cjs'), path.join(scope.usageDirectory, 'claude.json'), path.join(scope.usageDirectory, 'previous-statusline.json')], {
        encoding: 'utf8', input: JSON.stringify({ model: { display_name: 'Fixture' }, session_id: 'fixture-private-field', rate_limits: { five_hour: { used_percentage: 37, resets_at: 1999999999 } } }),
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal((await reader.read(profile.id)).claude.windows[0].used, 37);
    const record = JSON.parse(fs.readFileSync(path.join(scope.usageDirectory, 'claude.json'), 'utf8'));
    assert.deepEqual(Object.keys(record).sort(), ['rate_limits', 'updated_at']);
});

test('AU09 settings corruption is reported rather than replaced', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    fs.writeFileSync(scope.settingsPath, '{unfinished');
    await assert.rejects(reader.setClaude(profile.id, true), /invalid JSON/);
    assert.equal(fs.readFileSync(scope.settingsPath, 'utf8'), '{unfinished');
});

test('AU10 redirected usage files, settings, and profile directories are rejected', { skip: process.platform === 'win32' }, async context => {
    const { directory, accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const outside = path.join(directory, 'outside.json');
    fs.writeFileSync(outside, '{}');
    fs.mkdirSync(scope.usageDirectory);
    const usageFile = path.join(scope.usageDirectory, 'claude.json');
    fs.symlinkSync(outside, usageFile);
    await assert.rejects(reader.read(profile.id), /redirected/);
    fs.unlinkSync(usageFile);
    fs.symlinkSync(outside, scope.settingsPath);
    await assert.rejects(reader.setClaude(profile.id, true), /redirected/);
    assert.equal(fs.readFileSync(outside, 'utf8'), '{}');
});

test('AU11 a Codex scan does not follow a session file link outside the account', { skip: process.platform === 'win32' }, async context => {
    const { directory, accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'codex' });
    const scope = await reader.resolve(profile.id);
    const outside = path.join(directory, 'unrelated.jsonl');
    writeRecord(outside, codexRecord(95));
    const day = path.join(scope.sessionsDirectory, '2026', '10', '05');
    fs.mkdirSync(day, { recursive: true });
    fs.symlinkSync(outside, path.join(day, 'rollout-linked.jsonl'));
    assert.deepEqual((await reader.read(profile.id)).codex, { windows: [], updatedAt: null });
});

test('AU12 scoped installation does not inherit a previous command from another account', context => {
    const { directory } = fixture(context);
    const settingsPath = path.join(directory, 'settings.json');
    const previousFile = path.join(directory, 'previous-statusline.json');
    const otherScript = path.join(directory, 'other-account', 'claude-statusline.cjs');
    const otherPrevious = path.join(directory, 'other-account', 'previous-statusline.json');
    fs.mkdirSync(path.dirname(otherScript));
    fs.writeFileSync(otherScript, '// existing collector');
    writeRecord(otherPrevious, { command: 'printf should-not-be-inherited' });
    const original = { statusLine: { type: 'command', command: `"node" "${otherScript}" "${path.join(directory, 'other-account', 'claude.json')}" "${otherPrevious}"` } };
    writeRecord(settingsPath, original);
    const input = { settingsPath, previousFile, command: 'new-collector', enabled: true, toLocal: file => file, scoped: true };
    assert.equal(applyStatusLine({ ...input, isAutomatic: true }), 'unset');
    assert.throws(() => applyStatusLine(input), /outside this account/);
    assert.equal(fs.existsSync(previousFile), false);
    assert.deepEqual(JSON.parse(fs.readFileSync(settingsPath, 'utf8')), original);
});

test('AU13 malformed or unreadable usage records are unavailable instead of silently empty', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const record = path.join(scope.usageDirectory, 'claude.json');
    fs.mkdirSync(scope.usageDirectory);
    fs.writeFileSync(record, '{unfinished');
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 503);
    writeRecord(record, { rate_limits: {} });
    const originalRead = fs.readFileSync;
    context.mock.method(fs, 'readFileSync', (file, ...argumentsList) => {
        if (file === record) throw Object.assign(new Error('fixture denied'), { code: 'EACCES' });
        return originalRead(file, ...argumentsList);
    });
    await assert.rejects(reader.read(profile.id), error => error.statusCode === 503);
});

test('AU14 corrupt previous status-line records preserve both installed settings and the recovery file', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    writeRecord(scope.settingsPath, { statusLine: { type: 'command', command: 'printf original' } });
    await reader.setClaude(profile.id, true);
    const installed = fs.readFileSync(scope.settingsPath, 'utf8');
    const previousFile = path.join(scope.usageDirectory, 'previous-statusline.json');
    fs.writeFileSync(previousFile, '{unfinished');
    await assert.rejects(reader.setClaude(profile.id, false), error => error.statusCode === 503);
    assert.equal(fs.readFileSync(scope.settingsPath, 'utf8'), installed);
    assert.equal(fs.readFileSync(previousFile, 'utf8'), '{unfinished');
});

test('AU15 a failed settings restore keeps the original status-line recovery record', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const original = { type: 'command', command: 'printf original' };
    writeRecord(scope.settingsPath, { statusLine: original });
    await reader.setClaude(profile.id, true);
    const installed = fs.readFileSync(scope.settingsPath, 'utf8');
    const rename = fs.renameSync;
    context.mock.method(fs, 'renameSync', (source, destination) => {
        if (destination === scope.settingsPath) throw Object.assign(new Error('fixture denied'), { code: 'EACCES' });
        return rename(source, destination);
    });
    await assert.rejects(reader.setClaude(profile.id, false), /fixture denied/);
    assert.equal(fs.readFileSync(scope.settingsPath, 'utf8'), installed);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(scope.usageDirectory, 'previous-statusline.json'), 'utf8')), original);
});

test('AU16 POSIX status-line paths preserve shell characters during execution and repeated install or removal', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    const configDirectory = path.join(setup.directory, 'app $(touch PWNED) `touch PWNED2` "quoted"');
    const accounts = new AccountProfiles({ configDirectory, homeDirectory: setup.directory, claudeDirectory: path.join(setup.directory, '.claude'), codexDirectory: path.join(setup.directory, '.codex') });
    const reader = new AccountUsage({ ...setup, accounts, readWslInfo: async () => { throw new Error('Unexpected WSL lookup'); } });
    const profile = await accounts.create({ provider: 'claude' });
    const scope = await reader.resolve(profile.id);
    const settings = { statusLine: { type: 'command', command: 'printf original' } };
    writeRecord(scope.settingsPath, settings);
    await reader.setClaude(profile.id, true);
    const command = JSON.parse(fs.readFileSync(scope.settingsPath, 'utf8')).statusLine.command;
    assert.equal(usage.paddockStatusLineFiles(command).script, path.join(scope.usageDirectory, 'claude-statusline.cjs'));
    const result = spawnSync(command, { shell: true, cwd: setup.directory, input: JSON.stringify({ model: { display_name: 'Fixture' }, rate_limits: { five_hour: { used_percentage: 17, resets_at: 1999999999 } } }), encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, 'original');
    assert.equal(fs.existsSync(path.join(setup.directory, 'PWNED')), false);
    assert.equal(fs.existsSync(path.join(setup.directory, 'PWNED2')), false);
    await reader.setClaude(profile.id, true);
    await reader.setClaude(profile.id, false);
    assert.deepEqual(JSON.parse(fs.readFileSync(scope.settingsPath, 'utf8')), settings);
});

test('AU17 a Codex usage scan stops after two recent day folders', async context => {
    const { accounts, reader } = fixture(context);
    const profile = await accounts.create({ provider: 'codex' });
    const scope = await reader.resolve(profile.id);
    for (const day of ['01', '02', '03']) writeRecord(path.join(scope.sessionsDirectory, '2026', '10', day, 'rollout-fixture.jsonl'), codexRecord(10));
    const scanned = [];
    const originalRead = fs.readdirSync;
    context.mock.method(fs, 'readdirSync', (directory, ...argumentsList) => {
        scanned.push(directory);
        return originalRead(directory, ...argumentsList);
    });
    await reader.read(profile.id);
    assert.equal(scanned.includes(path.join(scope.sessionsDirectory, '2026', '10', '01')), false);
});


test('AU18 Codex official usage works without a sessions folder and receives only its own account scope', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const scope = await setup.reader.resolve(profile.id);
    const calls = [];
    const expected = { windows: [{ label: 'week', used: 83, resetsAt: 1999999999 }], updatedAt: 100 };
    setup.liveCodexUsage.read = async input => { calls.push(input); return expected; };
    assert.equal(fs.existsSync(scope.sessionsDirectory), false);
    assert.deepEqual((await setup.reader.read(profile.id)).codex, expected);
    assert.equal(calls[0].configDir, scope.configDir);
    assert.equal(calls[0].profile.id, profile.id);
});

test('AU19 Codex live failures use available legacy limits but never turn an empty lookup into zero usage', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const scope = await setup.reader.resolve(profile.id);
    setup.liveCodexUsage.read = async () => { throw Object.assign(new Error('Codex usage unavailable'), { statusCode: 503 }); };
    await assert.rejects(setup.reader.read(profile.id), error => error.statusCode === 503);
    writeRecord(path.join(scope.sessionsDirectory, '2026', '10', '05', 'rollout-fixture.jsonl'), codexRecord(21));
    assert.equal((await setup.reader.read(profile.id)).codex.windows[0].used, 21);
});
