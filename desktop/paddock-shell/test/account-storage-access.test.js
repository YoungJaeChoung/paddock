const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { AccountProfiles } = require('../account-profiles');
const { WslScripts } = require('../account-profile-wsl');
const { spawnSync } = require('node:child_process');

function fixture(
    context,
) {
    const homeDirectory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'paddock-storage-access-'));
    const configDirectory = path.join(homeDirectory, '.paddock', 'config');
    const accounts = new AccountProfiles({ homeDirectory, configDirectory });
    const restore = [];
    context.after(() => {
        for (const directory of restore) fs.chmodSync(directory, 0o700);
        fs.rmSync(homeDirectory, { recursive: true, force: true });
    });
    return { homeDirectory, configDirectory, accounts, restore };
}

test('AC01 diagnoses without mutation and repairs owned managed folders only after an explicit action', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'claude' });
    const prepared = await setup.accounts.prepare(account.id);
    const history = path.join(prepared.configDir, 'history.jsonl');
    fs.writeFileSync(history, 'synthetic preserved data', { mode: 0o400 });
    const directory = path.join(setup.configDirectory, 'agent-profiles');
    setup.restore.push(directory);
    fs.chmodSync(directory, 0o500);
    const scope = { accountId: account.id };
    assert.equal((await setup.accounts.inspectStorageAccess(scope)).status, 'repair');
    assert.equal(fs.statSync(directory).mode & 0o777, 0o500);
    assert.equal((await setup.accounts.allowStorageAccess(scope)).status, 'ready');
    assert.equal(fs.statSync(directory).mode & 0o777, 0o700);
    assert.equal(fs.readFileSync(history, 'utf8'), 'synthetic preserved data');
    assert.equal(fs.statSync(history).mode & 0o777, 0o400);
    assert.deepEqual(await setup.accounts.list(), [account]);
    assert.equal((await setup.accounts.prepare(account.id)).profile.id, account.id);
});

test('AC02 repairs blocked parent before retrying account creation without creating a duplicate', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    const parent = path.join(setup.homeDirectory, '.paddock');
    fs.mkdirSync(parent, { mode: 0o500 });
    setup.restore.push(parent);
    await assert.rejects(setup.accounts.create({ provider: 'codex', label: 'Saved draft' }));
    assert.equal((await setup.accounts.inspectStorageAccess({ runtime: 'native' })).status, 'repair');
    await setup.accounts.allowStorageAccess({ runtime: 'native' });
    const account = await setup.accounts.create({ provider: 'codex', label: 'Saved draft' });
    assert.equal(account.label, 'Saved draft');
    assert.deepEqual(await setup.accounts.list(), [account]);
});

test('AC03 rejects caller paths and refuses symlinked roots without touching their target', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    const outside = path.join(setup.homeDirectory, 'outside');
    fs.mkdirSync(outside, { mode: 0o500 });
    setup.restore.push(outside);
    fs.mkdirSync(path.dirname(setup.configDirectory));
    fs.symlinkSync(outside, setup.configDirectory);
    for (const method of ['inspectStorageAccess', 'allowStorageAccess', 'openStorageSettings']) {
        await assert.rejects(setup.accounts[method]({ path: outside }));
        await assert.rejects(setup.accounts[method]({ accountId: '../../outside' }));
    }
    assert.equal((await setup.accounts.inspectStorageAccess({ runtime: 'native' })).status, 'blocked');
    await assert.rejects(setup.accounts.allowStorageAccess({ runtime: 'native' }), /regular|link|restore/i);
    assert.equal(fs.statSync(outside).mode & 0o777, 0o500);
    assert.deepEqual(fs.readdirSync(outside), []);
});

test('AC04 never repairs another owner or a CLI-owned file', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'claude' });
    const prepared = await setup.accounts.prepare(account.id);
    const stat = fs.lstatSync.bind(fs);
    context.mock.method(fs, 'lstatSync', (...args) => {
        const value = stat(...args);
        if (args[0] === prepared.configDir) Object.defineProperty(value, 'uid', { value: process.getuid() + 1 });
        return value;
    });
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'settings');
    await assert.rejects(setup.accounts.allowStorageAccess({ accountId: account.id }), /owner|settings|permission/i);
    assert.equal(fs.statSync(prepared.configDir).mode & 0o777, 0o700);
});

test('AC05 pins an unreadable Linux folder before recovering it and preserves the saved account', { skip: process.platform !== 'linux' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'codex' });
    setup.restore.push(setup.configDirectory);
    fs.chmodSync(setup.configDirectory, 0o000);
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'repair');
    assert.equal((await setup.accounts.allowStorageAccess({ accountId: account.id })).status, 'ready');
    assert.deepEqual(await setup.accounts.list(), [account]);
});

test('AC06 WSL fixed program inspects without mutation, repairs owned Linux folders, and rejects links', { skip: process.platform !== 'linux' }, context => {
    const setup = fixture(context);
    const base = path.join(setup.homeDirectory, '.paddock');
    const profiles = path.join(base, 'agent-profiles');
    fs.mkdirSync(profiles, { recursive: true });
    setup.restore.push(base);
    fs.chmodSync(base, 0o000);
    const invoke = action => {
        const result = spawnSync('sh', ['-c', WslScripts.ACCESS, 'paddock-access', action, '', ''], { encoding: 'utf8', env: { ...process.env, HOME: setup.homeDirectory } });
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.split('\0')[0];
    };
    assert.equal(invoke('inspect'), 'repair');
    assert.equal(fs.statSync(base).mode & 0o777, 0o000);
    assert.equal(invoke('repair'), 'ready');
    assert.equal(fs.statSync(base).mode & 0o777, 0o700);
    fs.rmdirSync(profiles);
    const outside = path.join(setup.homeDirectory, 'outside');
    fs.mkdirSync(outside, { mode: 0o500 });
    setup.restore.push(outside);
    fs.symlinkSync(outside, profiles);
    assert.equal(invoke('repair'), 'blocked');
    assert.equal(fs.statSync(outside).mode & 0o777, 0o500);
});

test('AC07 Windows access denial goes to OS settings without chmod and macOS unreadable folders are not offered unsafe repair', { skip: process.platform !== 'linux' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'claude' });
    const originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform');
    context.after(() => Object.defineProperty(process, 'platform', originalPlatform));
    const access = fs.accessSync.bind(fs);
    context.mock.method(fs, 'accessSync', (...args) => {
        if (args[0] === setup.configDirectory) throw Object.assign(new Error('synthetic ACL denial'), { code: 'EACCES' });
        return access(...args);
    });
    let n_chmod = 0;
    context.mock.method(fs, 'chmodSync', () => { n_chmod += 1; });
    Object.defineProperty(process, 'platform', { value: 'win32' });
    const windows = await setup.accounts.inspectStorageAccess({ accountId: account.id });
    assert.equal(windows.status, 'settings');
    assert.equal(windows.canOpenSettings, true);
    await assert.rejects(setup.accounts.allowStorageAccess({ accountId: account.id }));
    assert.equal(n_chmod, 0);
    context.mock.restoreAll();
    Object.defineProperty(process, 'platform', { value: 'darwin' });
    setup.restore.push(setup.configDirectory);
    fs.chmodSync(setup.configDirectory, 0o000);
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'settings');
    await assert.rejects(setup.accounts.allowStorageAccess({ accountId: account.id }));
    assert.equal(fs.statSync(setup.configDirectory).mode & 0o777, 0o000);
});

test('AC08 WSL without Python keeps manual recovery available and does not call healthy storage a permission failure', { skip: process.platform !== 'linux' }, context => {
    const setup = fixture(context);
    const base = path.join(setup.homeDirectory, '.paddock');
    fs.mkdirSync(base, { mode: 0o500 });
    setup.restore.push(base);
    const invoke = () => {
        const result = spawnSync('/bin/sh', ['-c', WslScripts.ACCESS, 'paddock-access', 'inspect', '', ''], { encoding: 'utf8', env: { HOME: setup.homeDirectory, PATH: '/nonexistent-paddock-test-bin' } });
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.split('\0');
    };
    assert.deepEqual(invoke().slice(0, 3), ['settings', base, 'python-unavailable']);
    assert.equal(fs.statSync(base).mode & 0o777, 0o500);
    fs.chmodSync(base, 0o700);
    assert.equal(invoke()[0], 'ready');
});

test('AC09 actual Windows write denial opens settings even when access() succeeds, stays account-scoped, and clears after a successful retry', { skip: process.platform !== 'linux' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'claude' });
    const other = await setup.accounts.create({ provider: 'codex' });
    const platform = Object.getOwnPropertyDescriptor(process, 'platform');
    context.after(() => Object.defineProperty(process, 'platform', platform));
    Object.defineProperty(process, 'platform', { value: 'win32' });
    const write = fs.writeFileSync.bind(fs);
    let blocked = true;
    context.mock.method(fs, 'writeFileSync', (...args) => {
        if (blocked && path.dirname(args[0]) === setup.configDirectory) {
            throw Object.assign(new Error('synthetic Windows ACL refusal'), { code: 'EACCES', path: args[0] });
        }
        return write(...args);
    });
    fs.accessSync(setup.configDirectory, fs.constants.R_OK | fs.constants.W_OK);
    await assert.rejects(setup.accounts.rename(account.id, 'Renamed'));
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'settings');
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: other.id })).status, 'ready');
    await assert.rejects(setup.accounts.allowStorageAccess({ accountId: account.id }));
    blocked = false;
    await setup.accounts.rename(account.id, 'Renamed');
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'ready');
    assert.equal((await setup.accounts.list()).find(item => item.id === account.id).label, 'Renamed');
});

test('AC10 Windows account-list read denial remains visible when conversation switching diagnoses the target account', { skip: process.platform !== 'linux' }, async context => {
    const setup = fixture(context);
    const account = await setup.accounts.create({ provider: 'claude' });
    const platform = Object.getOwnPropertyDescriptor(process, 'platform');
    context.after(() => Object.defineProperty(process, 'platform', platform));
    Object.defineProperty(process, 'platform', { value: 'win32' });
    const read = fs.readFileSync.bind(fs);
    let denied = true;
    context.mock.method(fs, 'readFileSync', (...args) => {
        if (denied && args[0] === path.join(setup.configDirectory, 'account-profiles.json')) {
            throw Object.assign(new Error('synthetic metadata ACL refusal'), { code: 'EACCES', path: args[0] });
        }
        return read(...args);
    });
    await assert.rejects(setup.accounts.list());
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'settings');
    denied = false;
    await setup.accounts.list();
    assert.equal((await setup.accounts.inspectStorageAccess({ accountId: account.id })).status, 'ready');
});
