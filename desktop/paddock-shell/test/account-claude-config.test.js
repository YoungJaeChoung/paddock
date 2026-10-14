const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { ClaudeConfig, mergeClaudeConfig, syncClaudeConfig } = require('../account-claude-config');
const { pencilEntry } = require('../account-pencil');

const hasPython = spawnSync('python3', ['--version']).status === 0;
const PENCIL = '/mnt/c/Users/me/.pencil/mcp/paddock/out/mcp-server-windows-x64.exe';

function workspace(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'paddock-claude-config-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const files = { defaultFile: path.join(directory, 'default.json'), profileFile: path.join(directory, 'profile', '.claude.json'), syncFile: path.join(directory, 'profile', 'paddock-sync.json') };
    fs.mkdirSync(path.dirname(files.profileFile));
    const write = (name, value) => fs.writeFileSync(files[name], typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`);
    const read = name => JSON.parse(fs.readFileSync(files[name], 'utf8'));
    const record = servers => ({ version: 1, claude: { mcpServers: servers } });
    return { directory, files, write, read, record };
}

const SERVER_A = { type: 'http', url: 'https://example.com/a' };
const SERVER_B = { type: 'stdio', command: 'b', args: ['--x'], env: {} };

test('CC01 servers only the default has are copied into the profile and recorded as copies', context => {
    const { files, write, read, record } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A, b: SERVER_B } });
    write('profileFile', { oauthAccount: { emailAddress: 'kept' } });
    const result = syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(result, { status: 'updated', warnings: [] });
    assert.deepEqual(read('profileFile'), { oauthAccount: { emailAddress: 'kept' }, mcpServers: { a: SERVER_A, b: SERVER_B } });
    assert.deepEqual(read('syncFile'), record({ a: SERVER_A, b: SERVER_B }));
    assert.equal(syncClaudeConfig({ ...files, pencilCommand: null }).status, 'unchanged');
});

test('CC02 a server with the same name in both places takes the default value', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A } });
    write('profileFile', { mcpServers: { a: { type: 'http', url: 'https://old.example.com' } } });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers.a, SERVER_A);
});

test('CC03 a server only the profile has stays and is never recorded as a copy', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A } });
    write('profileFile', { mcpServers: { mine: SERVER_B } });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { mine: SERVER_B, a: SERVER_A });
    assert.deepEqual(Object.keys(read('syncFile').claude.mcpServers), ['a']);
    write('defaultFile', { mcpServers: {} });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { mine: SERVER_B });
});

test('CC04 a copied server disappears from the profile when the default drops it and the profile never changed it', context => {
    const { files, write, read, record } = workspace(context);
    write('defaultFile', { mcpServers: {} });
    write('profileFile', { mcpServers: { a: SERVER_A, mine: SERVER_B } });
    write('syncFile', record({ a: SERVER_A }));
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { mine: SERVER_B });
    assert.deepEqual(read('syncFile'), record({}));
});

test('CC05 a copied server the profile changed stays after the default drops it, and is no longer treated as a copy', context => {
    const { files, write, read, record } = workspace(context);
    const edited = { ...SERVER_A, url: 'https://edited.example.com' };
    write('defaultFile', { mcpServers: {} });
    write('profileFile', { mcpServers: { a: edited } });
    write('syncFile', record({ a: SERVER_A }));
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { a: edited });
    assert.deepEqual(read('syncFile'), record({}));
});

test('CC06 pencil is set to the fixed command last, is never copied from the default, and is never recorded or removed', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { mcpServers: { pencil: { type: 'stdio', command: '/default/versioned/pencil' }, a: SERVER_A } });
    write('profileFile', { mcpServers: { pencil: { command: '/profile/old' } } });
    syncClaudeConfig({ ...files, pencilCommand: PENCIL });
    assert.deepEqual(read('profileFile').mcpServers.pencil, pencilEntry(PENCIL));
    assert.deepEqual(Object.keys(read('syncFile').claude.mcpServers), ['a']);
    // Without a Pencil program the profile's registration is left as it is; with the default dropping every server, pencil still stays.
    write('defaultFile', { mcpServers: {} });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers.pencil, pencilEntry(PENCIL));
});

test('CC07 folders the profile has never seen get only the whitelisted answers; known folders are never touched', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { projects: {
        '/work/new': { hasTrustDialogAccepted: true, allowedTools: ['Bash(ls:*)'], lastCost: 1.5, history: ['secret'], mcpServers: { x: SERVER_A } },
        '/work/known': { hasTrustDialogAccepted: true, allowedTools: ['Edit'] },
        '/work/only-history': { lastCost: 2 },
    } });
    write('profileFile', { projects: { '/work/known': { hasTrustDialogAccepted: false, allowedTools: [] } } });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').projects, {
        '/work/known': { hasTrustDialogAccepted: false, allowedTools: [] },
        '/work/new': { hasTrustDialogAccepted: true, allowedTools: ['Bash(ls:*)'], mcpServers: { x: SERVER_A } },
    });
    for (const key of ClaudeConfig.PROJECT_KEYS) assert.equal(typeof key, 'string');
});

test('CC08 every other key, its order, indentation, trailing newline, and file permissions stay as the CLI wrote them', { skip: process.platform === 'win32' }, context => {
    const { files, write } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A }, oauthAccount: { accountUuid: 'default-login' }, theme: 'light' });
    write('profileFile', '{\n    "theme": "dark",\n    "oauthAccount": {"accountUuid": "profile-login"},\n    "numStartups": 7,\n    "mcpServers": {}\n}');
    fs.chmodSync(files.profileFile, 0o640);
    syncClaudeConfig({ ...files, pencilCommand: null });
    const text = fs.readFileSync(files.profileFile, 'utf8');
    assert.match(text, /^\{\n    "theme": "dark",\n    "oauthAccount": \{\n        "accountUuid": "profile-login"\n    \},\n    "numStartups": 7,\n    "mcpServers": \{\n        "a"/);
    assert.equal(text.endsWith('}'), true, 'a file without a trailing newline keeps none');
    assert.equal(fs.statSync(files.profileFile).mode & 0o777, 0o640);
    assert.deepEqual(fs.readdirSync(path.dirname(files.profileFile)).filter(name => name.includes('tmp')), []);
});

test('CC09 a profile without a sign-in file gets one with mcpServers and projects only (0600); nothing is created when there is nothing to put in it', { skip: process.platform === 'win32' }, context => {
    const { files, write, read } = workspace(context);
    assert.deepEqual(syncClaudeConfig({ ...files, pencilCommand: null }), { status: 'unchanged', warnings: [] });
    assert.equal(fs.existsSync(files.profileFile), false);
    write('defaultFile', { mcpServers: { a: SERVER_A }, projects: { '/work': { hasTrustDialogAccepted: true } }, oauthAccount: { accountUuid: 'x' } });
    assert.deepEqual(syncClaudeConfig({ ...files, pencilCommand: PENCIL }), { status: 'created', warnings: [] });
    assert.deepEqual(read('profileFile'), { mcpServers: { a: SERVER_A, pencil: pencilEntry(PENCIL) }, projects: { '/work': { hasTrustDialogAccepted: true } } });
    assert.equal(fs.statSync(files.profileFile).mode & 0o777, 0o600);
    assert.equal(fs.statSync(files.syncFile).mode & 0o777, 0o600);
});

test('CC10 an unreadable default or profile file skips the whole sync with a warning and changes nothing', context => {
    const { files, write } = workspace(context);
    write('defaultFile', '{ broken');
    write('profileFile', { mcpServers: { a: SERVER_A } });
    const before = fs.readFileSync(files.profileFile, 'utf8');
    const first = syncClaudeConfig({ ...files, pencilCommand: PENCIL });
    assert.equal(first.status, 'skipped');
    assert.equal(first.warnings.length, 1);
    assert.equal(fs.readFileSync(files.profileFile, 'utf8'), before);
    write('defaultFile', { mcpServers: {} });
    for (const broken of ['{ broken', '[]', '{"mcpServers": []}']) {
        write('profileFile', broken);
        const result = syncClaudeConfig({ ...files, pencilCommand: PENCIL });
        assert.equal(result.status, 'skipped', broken);
        assert.equal(result.warnings.length, 1);
        assert.equal(fs.readFileSync(files.profileFile, 'utf8'), broken);
    }
    assert.equal(fs.existsSync(files.syncFile), false);
});

test('CC11 an absent default copies and removes nothing, but Pencil is still registered', context => {
    const { files, write, read, record } = workspace(context);
    write('profileFile', { mcpServers: { a: SERVER_A } });
    write('syncFile', record({ a: SERVER_A }));
    syncClaudeConfig({ ...files, pencilCommand: PENCIL });
    assert.deepEqual(read('profileFile').mcpServers, { a: SERVER_A, pencil: pencilEntry(PENCIL) });
    assert.deepEqual(read('syncFile'), record({ a: SERVER_A }));
});

test('CC12 an unreadable copy record never causes a removal', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { mcpServers: {} });
    write('profileFile', { mcpServers: { a: SERVER_A } });
    write('syncFile', '{ broken');
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { a: SERVER_A });
});

test('CC13 mergeClaudeConfig does not change its inputs', () => {
    const defaultData = { mcpServers: { a: SERVER_A }, projects: { '/w': { hasTrustDialogAccepted: true } } };
    const profileData = { mcpServers: { b: SERVER_B } };
    const before = JSON.stringify([defaultData, profileData]);
    mergeClaudeConfig({ defaultData, profileData, synced: {}, pencil: pencilEntry(PENCIL) });
    assert.equal(JSON.stringify([defaultData, profileData]), before);
});

// The WSL preparation script runs the same algorithm in Python. Each scenario runs through both implementations on identical files.
const SCENARIOS = {
    'copy and keep': { default: { mcpServers: { a: SERVER_A, pencil: { command: 'x' } }, projects: { '/w': { hasTrustDialogAccepted: true, junk: 1 } } }, profile: { oauthAccount: { id: 1 }, mcpServers: { mine: SERVER_B } }, synced: {} },
    'remove untouched copy': { default: { mcpServers: {} }, profile: { mcpServers: { a: SERVER_A, mine: SERVER_B } }, synced: { a: SERVER_A } },
    'keep edited copy': { default: { mcpServers: {} }, profile: { mcpServers: { a: { ...SERVER_A, url: 'edited' } } }, synced: { a: SERVER_A } },
    'default wins': { default: { mcpServers: { a: SERVER_A } }, profile: { mcpServers: { a: { type: 'http', url: 'old' } } }, synced: {} },
    'create profile': { default: { mcpServers: { a: SERVER_A }, projects: { '/w': { allowedTools: ['x'] } } }, profile: null, synced: {} },
    'nothing to do': { default: null, profile: null, synced: {}, pencil: '' },
    'absent default': { default: null, profile: { mcpServers: { a: SERVER_A } }, synced: { a: SERVER_A } },
    'known folder untouched': { default: { projects: { '/w': { hasTrustDialogAccepted: true } } }, profile: { projects: { '/w': { hasTrustDialogAccepted: false } } }, synced: {} },
    'unicode and indent': { default: { mcpServers: { '한글': SERVER_A } }, profile: '{\n\t"memo": "한글",\n\t"mcpServers": {}\n}\n', synced: {} },
    'broken profile': { default: { mcpServers: { a: SERVER_A } }, profile: '{ broken', synced: {} },
    'broken default': { default: '{ broken', profile: { mcpServers: {} }, synced: {} },
    'shadowed name restored': { default: { mcpServers: {} }, profile: { mcpServers: { a: SERVER_A } }, synced: { a: SERVER_A }, shadowed: { a: { type: 'http', url: 'mine' } } },
    'shadow recorded': { default: { mcpServers: { a: SERVER_A } }, profile: { mcpServers: { a: { type: 'http', url: 'mine' } } }, synced: {} },
    'bad shape': { default: { mcpServers: { a: SERVER_A } }, profile: { mcpServers: [] }, synced: {} },
};

for (const [name, scenario] of Object.entries(SCENARIOS)) {
    test(`CC14 the Python implementation matches the JavaScript one: ${name}`, { skip: process.platform === 'win32' || !hasPython }, context => {
        const outcomes = [];
        for (const runner of ['js', 'python']) {
            const { files, write } = workspace(context);
            if (scenario.default !== null) write('defaultFile', scenario.default);
            if (scenario.profile !== null) write('profileFile', scenario.profile);
            if (Object.keys(scenario.synced).length) write('syncFile', { version: 1, claude: { mcpServers: scenario.synced, ...(scenario.shadowed ? { shadowed: scenario.shadowed } : {}) } });
            const pencilCommand = scenario.pencil === undefined ? PENCIL : scenario.pencil;
            let warning = '';
            if (runner === 'js') {
                warning = syncClaudeConfig({ ...files, pencilCommand: pencilCommand || null }).warnings[0] || '';
            } else {
                const result = spawnSync('python3', ['-', files.defaultFile, files.profileFile, files.syncFile, pencilCommand], { input: ClaudeConfig.PYTHON, encoding: 'utf8' });
                assert.equal(result.status, 0, result.stderr);
                warning = result.stdout;
            }
            const text = file => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null);
            outcomes.push({ warning, profile: text(files.profileFile), sync: text(files.syncFile) });
        }
        assert.deepEqual(outcomes[1], outcomes[0]);
    });
}

test('CC15 a name the profile had first returns to the profile\'s own value when the default drops it, and is deleted only when Paddock created it', context => {
    const { files, write, read } = workspace(context);
    const mine = { type: 'http', url: 'https://mine.example.com' };
    write('profileFile', { mcpServers: { a: mine } });
    write('defaultFile', { mcpServers: { a: SERVER_A, b: SERVER_B } });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { a: SERVER_A, b: SERVER_B });
    assert.deepEqual(read('syncFile').claude.shadowed, { a: mine });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('syncFile').claude.shadowed, { a: mine }, 'the original value is remembered across runs');
    write('defaultFile', { mcpServers: {} });
    syncClaudeConfig({ ...files, pencilCommand: null });
    assert.deepEqual(read('profileFile').mcpServers, { a: mine }, 'a returns to the profile value; b, which Paddock created, is gone');
    assert.deepEqual(read('syncFile').claude, { mcpServers: {} });
});

test('CC16 a change made to the profile file by a running session while Paddock merges is not overwritten', context => {
    const { files, write, read } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A } });
    write('profileFile', { theme: 'dark' });
    const readFile = fs.readFileSync;
    let n_reads = 0;
    context.mock.method(fs, 'readFileSync', (file, ...rest) => {
        const text = readFile(file, ...rest);
        // The session rewrites the file right after the first read (a refreshed sign-in).
        if (file === files.profileFile && (n_reads += 1) === 1) fs.writeFileSync(files.profileFile, JSON.stringify({ theme: 'dark', oauthAccount: { accountUuid: 'refreshed' } }));
        return text;
    });
    assert.equal(syncClaudeConfig({ ...files, pencilCommand: null }).status, 'updated');
    assert.deepEqual(read('profileFile'), { theme: 'dark', oauthAccount: { accountUuid: 'refreshed' }, mcpServers: { a: SERVER_A } });
});

test('CC17 a linked profile file is skipped with a warning and left as the link', { skip: process.platform === 'win32' }, context => {
    const { directory, files, write } = workspace(context);
    write('defaultFile', { mcpServers: { a: SERVER_A } });
    const real = path.join(directory, 'elsewhere.json');
    fs.writeFileSync(real, '{"theme":"dark"}');
    fs.symlinkSync(real, files.profileFile);
    const result = syncClaudeConfig({ ...files, pencilCommand: PENCIL });
    assert.deepEqual(result, { status: 'skipped', warnings: ['The account sign-in file (.claude.json) is a link, so MCP servers were not synced.'] });
    assert.equal(fs.lstatSync(files.profileFile).isSymbolicLink(), true);
    assert.equal(fs.readFileSync(real, 'utf8'), '{"theme":"dark"}');
});
