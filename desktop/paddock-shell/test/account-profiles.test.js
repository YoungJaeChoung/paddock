const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { AccountProfile, AccountProfiles } = require('../account-profiles');
const { WslScripts } = require('../account-profile-wsl');
const { BrowserBridge } = require('../account-browser');

function fixture(
    context,
) {
    const homeDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-accounts-'));
    const configDirectory = path.join(homeDirectory, '.paddock', 'config');
    const claudeDirectory = path.join(homeDirectory, '.claude');
    const codexDirectory = path.join(homeDirectory, '.codex');
    const configuration = { homeDirectory, configDirectory, claudeDirectory, codexDirectory };
    context.after(() => fs.rmSync(homeDirectory, { recursive: true, force: true }));
    return { ...configuration, accounts: new AccountProfiles(configuration) };
}

function skill(
    directory,
    name,
) {
    const skillDirectory = path.join(directory, 'skills', name);
    fs.mkdirSync(skillDirectory, { recursive: true });
    fs.writeFileSync(path.join(skillDirectory, 'SKILL.md'), '---\nname: example\n---\nShared instructions.\n');
    return skillDirectory;
}

function wslRuntime(
    context,
) {
    const previousDistribution = process.env.WSL_DISTRO_NAME;
    process.env.WSL_DISTRO_NAME = 'PaddockTest';
    context.after(() => {
        if (previousDistribution === undefined) delete process.env.WSL_DISTRO_NAME;
        else process.env.WSL_DISTRO_NAME = previousDistribution;
    });
}

test('A01 saves two providers with independent identifiers, private folders, and metadata only', async context => {
    const setup = fixture(context);
    const first = await setup.accounts.create({ provider: 'claude' });
    const second = await setup.accounts.create({ provider: 'codex' });
    assert.match(first.id, AccountProfile.ID);
    assert.notEqual(first.id, second.id);
    assert.equal(first.label, 'Claude 1');
    assert.equal(second.label, 'Codex 1');
    const claude = await setup.accounts.prepare(first.id);
    const codex = await setup.accounts.prepare(second.id);
    assert.deepEqual(claude.env, { CLAUDE_CONFIG_DIR: claude.configDir });
    assert.deepEqual(codex.env, { CODEX_HOME: codex.configDir });
    assert.notEqual(claude.configDir, codex.configDir);
    assert.deepEqual(Object.keys(first).sort(), ['id', 'label', 'provider', 'runtime']);
    assert.equal(fs.existsSync(path.join(claude.configDir, '.credentials.json')), false);
    assert.equal(fs.existsSync(path.join(codex.configDir, 'auth.json')), false);
    if (process.platform !== 'win32') {
        assert.equal(fs.statSync(claude.configDir).mode & 0o777, 0o700);
        assert.equal(fs.statSync(path.join(setup.configDirectory, AccountProfile.METADATA_FILE)).mode & 0o777, 0o600);
    }
    assert.deepEqual(await new AccountProfiles(setup).list(), [first, second]);
});

test('A02 rejects invalid providers, control names, caller paths, and directory traversal', async context => {
    const { accounts, homeDirectory } = fixture(context);
    for (const input of [null, [], { provider: '__proto__' }, { provider: 'claude', label: ' ' }, { provider: 'claude', label: 'A\nB' }, { provider: 'claude', label: 'a'.repeat(65) }, { provider: 'claude', runtime: 'remote' }, { provider: 'claude', shareSkills: false }, { provider: 'claude', configDir: '/tmp/arbitrary' }]) {
        await assert.rejects(accounts.create(input));
    }
    await assert.rejects(accounts.prepare('../.claude'), /identifier is invalid/);
    await assert.rejects(accounts.rename('../../auth.json', 'Another'), /identifier is invalid/);
    await assert.rejects(accounts.remove('/tmp/anything'), /identifier is invalid/);
    assert.deepEqual(fs.readdirSync(homeDirectory), []);
});

test('A03 serializes concurrent creation and rejects duplicate names without losing saved entries', async context => {
    const { accounts } = fixture(context);
    const profiles = await Promise.all([accounts.create({ provider: 'claude' }), accounts.create({ provider: 'claude' })]);
    assert.deepEqual(profiles.map(profile => profile.label), ['Claude 1', 'Claude 2']);
    await assert.rejects(accounts.rename(profiles[1].id, ' claude 1 '), /already has an account/);
    const renamed = await accounts.rename(profiles[1].id, 'Extra');
    assert.equal(renamed.id, profiles[1].id);
    assert.equal(renamed.label, 'Extra');
    assert.equal((await accounts.list()).length, 2);
});

test('A04 links existing skill sources for both providers and never reads credentials or copies settings', async context => {
    const setup = fixture(context);
    const readFile = fs.readFileSync;
    context.mock.method(fs, 'readFileSync', (file, ...argumentsList) => {
        assert.doesNotMatch(String(file), /(?:\.credentials|auth|settings)\.json$/);
        return readFile(file, ...argumentsList);
    });
    for (const provider of ['claude', 'codex']) {
        const sourceDirectory = setup[`${provider}Directory`];
        const sourceSkill = skill(sourceDirectory, 'shared-writing');
        skill(sourceDirectory, 'synced');
        skill(sourceDirectory, '.system');
        fs.writeFileSync(path.join(sourceDirectory, 'settings.json'), '{}');
        const first = await setup.accounts.create({ provider });
        const second = await setup.accounts.create({ provider });
        for (const profile of [first, second]) {
            const prepared = await setup.accounts.prepare(profile.id);
            assert.equal(fs.realpathSync(path.join(prepared.configDir, 'skills', 'shared-writing')), fs.realpathSync(sourceSkill));
            assert.deepEqual(fs.readdirSync(path.join(prepared.configDir, 'skills')), ['shared-writing']);
            assert.equal(fs.existsSync(path.join(prepared.configDir, 'settings.json')), false);
        }
        assert.equal(fs.readFileSync(path.join(sourceSkill, 'SKILL.md'), 'utf8'), '---\nname: example\n---\nShared instructions.\n');
    }
});

test('A05 shares new skills at launch and preserves a conflicting account skill', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'claude' });
    const initial = await setup.accounts.prepare(profile.id);
    skill(setup.claudeDirectory, 'new-skill');
    skill(initial.configDir, 'new-skill');
    const prepared = await setup.accounts.prepare(profile.id);
    assert.deepEqual(prepared.warnings, ['An existing skill named new-skill was kept.']);
    assert.equal(fs.lstatSync(path.join(initial.configDir, 'skills', 'new-skill')).isSymbolicLink(), false);
    skill(setup.claudeDirectory, 'added-later');
    await setup.accounts.prepare(profile.id);
    assert.equal(fs.lstatSync(path.join(initial.configDir, 'skills', 'added-later')).isSymbolicLink(), true);
});

test('A06 keeps original shared skill links instead of replacing their targets', async context => {
    const setup = fixture(context);
    const original = skill(setup.claudeDirectory, 'writing');
    const profile = await setup.accounts.create({ provider: 'claude' });
    const first = await setup.accounts.prepare(profile.id);
    const second = await setup.accounts.prepare(profile.id);
    assert.deepEqual(second.warnings, []);
    assert.equal(fs.realpathSync(path.join(first.configDir, 'skills', 'writing')), original);
});

test('A07 legacy accounts with disabled sharing automatically use existing skills without changing identity', async context => {
    const setup = fixture(context);
    fs.mkdirSync(setup.configDirectory, { recursive: true });
    const metadata = path.join(setup.configDirectory, AccountProfile.METADATA_FILE);
    for (const provider of ['claude', 'codex']) {
        const source = skill(setup[`${provider}Directory`], 'writing');
        const legacyProfile = { id: '00000000-0000-4000-8000-000000000001', provider, label: 'Existing', runtime: 'native', shareSkills: false };
        fs.writeFileSync(metadata, JSON.stringify({ version: 1, profiles: [legacyProfile] }));
        const prepared = await setup.accounts.prepare(legacyProfile.id);
        assert.equal(prepared.profile.id, legacyProfile.id);
        assert.equal(Object.hasOwn(prepared.profile, 'shareSkills'), false);
        assert.equal(fs.realpathSync(path.join(prepared.configDir, 'skills', 'writing')), fs.realpathSync(source));
        const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
        assert.deepEqual(prepared.env, { [variable]: prepared.configDir });
        assert.deepEqual(await setup.accounts.list(), [prepared.profile]);
        await setup.accounts.rename(legacyProfile.id, 'Renamed');
        assert.equal(Object.hasOwn(JSON.parse(fs.readFileSync(metadata, 'utf8')).profiles[0], 'shareSkills'), false);
    }
});

test('A08 removes only metadata and retains the CLI data folder', async context => {
    const { accounts } = fixture(context);
    const profile = await accounts.create({ provider: 'claude' });
    const prepared = await accounts.prepare(profile.id);
    const history = path.join(prepared.configDir, 'history.jsonl');
    fs.writeFileSync(history, 'fixture conversation\n');
    await accounts.remove(profile.id);
    assert.deepEqual(await accounts.list(), []);
    assert.equal(fs.readFileSync(history, 'utf8'), 'fixture conversation\n');
    await assert.rejects(accounts.prepare(profile.id), /no longer available/);
});

test('A09 refuses a redirected account root and saves no partially prepared profile', async context => {
    const setup = fixture(context);
    const outside = path.join(setup.homeDirectory, 'outside');
    fs.mkdirSync(outside);
    fs.mkdirSync(setup.configDirectory, { recursive: true });
    fs.symlinkSync(outside, path.join(setup.configDirectory, 'agent-profiles'), process.platform === 'win32' ? 'junction' : 'dir');
    await assert.rejects(setup.accounts.create({ provider: 'claude' }), /not a regular directory/);
    assert.deepEqual(await setup.accounts.list(), []);
    assert.deepEqual(fs.readdirSync(outside), []);
});

test('A10 refuses a redirected profile or skills folder before writing shared links', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'claude' });
    const prepared = await setup.accounts.prepare(profile.id);
    const outside = path.join(setup.homeDirectory, 'outside');
    fs.mkdirSync(outside);
    fs.rmdirSync(path.join(prepared.configDir, 'skills'));
    fs.symlinkSync(outside, path.join(prepared.configDir, 'skills'), process.platform === 'win32' ? 'junction' : 'dir');
    skill(setup.claudeDirectory, 'writing');
    await assert.rejects(setup.accounts.prepare(profile.id), /not a regular directory/);
    assert.deepEqual(fs.readdirSync(outside), []);
});

test('A11 rejects corrupt or redirected metadata rather than overwriting the account list', async context => {
    const setup = fixture(context);
    fs.mkdirSync(setup.configDirectory, { recursive: true });
    const metadata = path.join(setup.configDirectory, AccountProfile.METADATA_FILE);
    fs.writeFileSync(metadata, '{unfinished');
    await assert.rejects(setup.accounts.create({ provider: 'claude' }), /saved account list cannot be read/);
    assert.equal(fs.readFileSync(metadata, 'utf8'), '{unfinished');
    fs.unlinkSync(metadata);
    const otherDirectory = path.join(setup.homeDirectory, 'other-data');
    fs.mkdirSync(otherDirectory);
    const outside = path.join(otherDirectory, 'saved.json');
    fs.writeFileSync(outside, '{"version":1,"profiles":[]}');
    // Windows directory junctions require no elevated symlink privilege and must also be rejected as metadata.
    fs.symlinkSync(process.platform === 'win32' ? otherDirectory : outside, metadata, process.platform === 'win32' ? 'junction' : 'file');
    await assert.rejects(setup.accounts.list(), /saved account list cannot be read/);
    assert.equal(fs.readFileSync(outside, 'utf8'), '{"version":1,"profiles":[]}');
});

test('A12 rejects unavailable WSL runtime without creating a native substitute', { skip: process.platform === 'win32' }, async context => {
    const setup = fixture(context);
    await assert.rejects(setup.accounts.create({ provider: 'claude', runtime: 'wsl' }), /only be added from Windows/);
    assert.deepEqual(await setup.accounts.list(), []);
    assert.deepEqual(fs.readdirSync(setup.homeDirectory), []);
});

test('A15 keeps a broken conflicting skill link and reports that it was not shared', async context => {
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'claude' });
    const prepared = await setup.accounts.prepare(profile.id);
    skill(setup.claudeDirectory, 'writing');
    const missingDirectory = path.join(setup.homeDirectory, 'missing-skill');
    fs.symlinkSync(missingDirectory, path.join(prepared.configDir, 'skills', 'writing'), process.platform === 'win32' ? 'junction' : 'dir');
    const result = await setup.accounts.prepare(profile.id);
    assert.deepEqual(result.warnings, ['An existing skill named writing was kept.']);
    assert.equal(fs.existsSync(missingDirectory), false);
});

test('A13 WSL preparation script passes POSIX shell syntax checking without touching account folders', { skip: process.platform === 'win32' }, () => {
    const result = spawnSync('sh', ['-n', '-c', WslScripts.PREPARE], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, '');
});

test('A14 WSL invokes a fixed script with separate arguments and hides subprocess errors', async context => {
    const childProcess = require('node:child_process');
    const calls = [];
    let shouldFail = false;
    context.mock.method(childProcess, 'execFile', (command, argumentsList, options, callback) => {
        calls.push({ command, argumentsList, options });
        if (shouldFail) callback(new Error('fixture-private-process-output'));
        else callback(null, '/home/fixture/account\0/home/fixture/account/paddock-bin\0');
    });
    const modulePath = require.resolve('../account-profile-wsl');
    const previousModule = require.cache[modulePath];
    delete require.cache[modulePath];
    context.after(() => { require.cache[modulePath] = previousModule; });
    const { runWsl } = require('../account-profile-wsl');
    const input = ['00000000-0000-4000-8000-000000000002', 'codex', BrowserBridge.NAME, BrowserBridge.SCRIPT];
    assert.deepEqual(await runWsl(WslScripts.PREPARE, input, 'Ubuntu Test'), ['/home/fixture/account', '/home/fixture/account/paddock-bin']);
    assert.deepEqual(calls[0].argumentsList, ['--distribution', 'Ubuntu Test', '--exec', 'sh', '-c', WslScripts.PREPARE, 'paddock-accounts', ...input]);
    assert.equal(calls[0].command, 'wsl.exe');
    assert.equal(calls[0].options.shell, undefined);
    shouldFail = true;
    await assert.rejects(runWsl(WslScripts.PREPARE, input, 'Ubuntu Test'), error => error.message.includes('could not be prepared') && !error.message.includes('fixture-private-process-output'));
});

test('A16 native WSL prepares a private fixed browser helper and refreshes it without storing browser data in account metadata', { skip: process.platform !== 'linux' }, async context => {
    wslRuntime(context);
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const first = await setup.accounts.prepare(profile.id);
    assert.equal(first.browserDirectory, path.join(first.configDir, 'paddock-bin'));
    const commandFile = path.join(first.browserDirectory, BrowserBridge.NAME);
    assert.equal(fs.statSync(first.browserDirectory).mode & 0o777, 0o700);
    assert.equal(fs.statSync(commandFile).mode & 0o777, 0o700);
    assert.equal(fs.readFileSync(commandFile, 'utf8'), BrowserBridge.SCRIPT);
    fs.writeFileSync(commandFile, '#!/bin/sh\n# old browser helper\n');
    const second = await setup.accounts.prepare(profile.id);
    assert.equal(second.browserDirectory, first.browserDirectory);
    assert.equal(fs.readFileSync(commandFile, 'utf8'), BrowserBridge.SCRIPT);
    assert.deepEqual(fs.readdirSync(first.browserDirectory), [BrowserBridge.NAME]);
    assert.deepEqual(await setup.accounts.list(), [profile]);
    assert.equal(Object.hasOwn(profile, 'browserDirectory'), false);
});

test('A17 native WSL refuses symlink or directory browser helpers without changing their targets', { skip: process.platform !== 'linux' }, async context => {
    wslRuntime(context);
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'claude' });
    const prepared = await setup.accounts.prepare(profile.id);
    const commandFile = path.join(prepared.browserDirectory, BrowserBridge.NAME);
    const outside = path.join(setup.homeDirectory, 'existing-script');
    fs.writeFileSync(outside, 'unchanged');
    fs.unlinkSync(commandFile);
    fs.symlinkSync(outside, commandFile);
    await assert.rejects(setup.accounts.prepare(profile.id), /browser helper is not a regular file/);
    assert.equal(fs.readFileSync(outside, 'utf8'), 'unchanged');
    fs.unlinkSync(commandFile);
    fs.mkdirSync(commandFile);
    await assert.rejects(setup.accounts.prepare(profile.id), /browser helper is not a regular file/);
    assert.deepEqual(fs.readdirSync(commandFile), []);
});

test('A18 native WSL refuses a redirected browser helper directory', { skip: process.platform !== 'linux' }, async context => {
    wslRuntime(context);
    const setup = fixture(context);
    const profile = await setup.accounts.create({ provider: 'codex' });
    const prepared = await setup.accounts.prepare(profile.id);
    const outside = path.join(setup.homeDirectory, 'outside-helper-directory');
    fs.mkdirSync(outside);
    fs.unlinkSync(path.join(prepared.browserDirectory, BrowserBridge.NAME));
    fs.rmdirSync(prepared.browserDirectory);
    fs.symlinkSync(outside, prepared.browserDirectory);
    await assert.rejects(setup.accounts.prepare(profile.id), /not a regular directory/);
    assert.deepEqual(fs.readdirSync(outside), []);
});

test('A19 Windows WSL preparation separates the browser directory from warnings and rejects a foreign helper path', { skip: process.platform !== 'win32' }, async context => {
    const setup = fixture(context);
    const profile = { id: '00000000-0000-4000-8000-000000000001', provider: 'claude', runtime: 'wsl', label: 'Main', wslDistribution: 'Ubuntu Test' };
    fs.mkdirSync(setup.configDirectory, { recursive: true });
    fs.writeFileSync(path.join(setup.configDirectory, AccountProfile.METADATA_FILE), JSON.stringify({ version: 1, profiles: [profile] }));
    const configDir = `/home/fixture/.paddock/agent-profiles/${profile.id}/claude`;
    let browserDirectory = `${configDir}/paddock-bin`;
    const calls = [];
    context.mock.method(require('node:child_process'), 'execFile', (command, argumentsList, options, callback) => {
        calls.push({ command, argumentsList });
        callback(null, `${configDir}\0${browserDirectory}\0An existing skill named writing was kept.\0`);
    });
    const modulePaths = [require.resolve('../account-profile-wsl'), require.resolve('../account-profiles')];
    const previousModules = modulePaths.map(modulePath => require.cache[modulePath]);
    for (const modulePath of modulePaths) delete require.cache[modulePath];
    context.after(() => modulePaths.forEach((modulePath, index) => { require.cache[modulePath] = previousModules[index]; }));
    const PreparedAccounts = require('../account-profiles').AccountProfiles;
    const accounts = new PreparedAccounts(setup);
    const prepared = await accounts.prepare(profile.id);
    assert.equal(prepared.configDir, configDir);
    assert.equal(prepared.browserDirectory, browserDirectory);
    assert.deepEqual(prepared.warnings, ['An existing skill named writing was kept.']);
    assert.deepEqual(calls[0].argumentsList.slice(-4), [profile.id, profile.provider, BrowserBridge.NAME, BrowserBridge.SCRIPT]);
    browserDirectory = '/tmp/another-account/paddock-bin';
    await assert.rejects(accounts.prepare(profile.id), /valid browser helper folder/);
});
