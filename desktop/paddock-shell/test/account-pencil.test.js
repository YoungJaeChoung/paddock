const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { AccountProfiles } = require('../account-profiles');
const { WslScripts } = require('../account-profile-wsl');
const { BrowserBridge } = require('../account-browser');
const { PencilServer, PencilDoctor, toWslPath, pencilEntry, registerPencilServer, findWindowsPencil } = require('../account-pencil');

const hasPython = spawnSync('python3', ['--version']).status === 0;

function temporaryDirectory(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'paddock-pencil-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

function fakePencil(
    directory,
) {
    const command = path.join(directory, ...PencilServer.EXECUTABLE);
    fs.mkdirSync(path.dirname(command), { recursive: true });
    fs.writeFileSync(command, '#!/bin/sh\n');
    fs.chmodSync(command, 0o755);
    return command;
}

test('P01 toWslPath maps a drive path to its /mnt form and rejects other shapes', () => {
    assert.equal(toWslPath('C:\\Users\\young\\.pencil\\a.exe'), '/mnt/c/Users/young/.pencil/a.exe');
    assert.equal(toWslPath('D:/x/y'), '/mnt/d/x/y');
    assert.equal(toWslPath('/home/young'), null);
});

test('P02 registerPencilServer merges only mcpServers.pencil, keeps other keys and formatting, and is idempotent', context => {
    const file = path.join(temporaryDirectory(context), '.claude.json');
    fs.writeFileSync(file, '{\n    "oauthAccount": {"email": "kept"},\n    "mcpServers": {"other": {"command": "x"}}\n}\n', { mode: 0o600 });
    assert.equal(registerPencilServer(file, '/mnt/c/p.exe'), 'updated');
    const written = fs.readFileSync(file, 'utf8');
    assert.deepEqual(JSON.parse(written), { oauthAccount: { email: 'kept' }, mcpServers: { other: { command: 'x' }, pencil: pencilEntry('/mnt/c/p.exe') } });
    assert.match(written, /^\{\n    "oauthAccount"/);
    assert.ok(written.endsWith('}\n'));
    assert.equal(fs.statSync(file).mode & 0o777, 0o600);
    assert.equal(registerPencilServer(file, '/mnt/c/p.exe'), 'unchanged');
    assert.equal(fs.readFileSync(file, 'utf8'), written);
    assert.equal(registerPencilServer(file, '/mnt/c/moved.exe'), 'updated');
    assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).mcpServers.pencil.command, '/mnt/c/moved.exe');
    assert.deepEqual(fs.readdirSync(path.dirname(file)), ['.claude.json']);
});

test('P03 registerPencilServer adds mcpServers when absent and never creates or rewrites unusable files', context => {
    const directory = temporaryDirectory(context);
    const file = path.join(directory, '.claude.json');
    assert.equal(registerPencilServer(file, '/p.exe'), 'skipped');
    assert.equal(fs.existsSync(file), false);
    fs.writeFileSync(file, '{"a":1}');
    assert.equal(registerPencilServer(file, '/p.exe'), 'updated');
    assert.equal(fs.readFileSync(file, 'utf8'), JSON.stringify({ a: 1, mcpServers: { pencil: pencilEntry('/p.exe') } }, null, 2));
    for (const broken of ['{not json', '[]', '{"mcpServers": []}']) {
        fs.writeFileSync(file, broken);
        assert.equal(registerPencilServer(file, '/p.exe'), 'skipped');
        assert.equal(fs.readFileSync(file, 'utf8'), broken);
    }
});

test('P04 findWindowsPencil returns the first Windows user folder holding the server', context => {
    const users = temporaryDirectory(context);
    assert.equal(findWindowsPencil(path.join(users, 'missing')), null);
    fs.mkdirSync(path.join(users, 'Public'));
    const command = fakePencil(path.join(users, 'young'));
    assert.equal(findWindowsPencil(users), command);
});

test('P05 a native WSL Claude profile registers Pencil on every prepare and gets the diagnostic; Codex gets neither', { skip: process.platform !== 'linux' }, async context => {
    const previous = process.env.WSL_DISTRO_NAME;
    process.env.WSL_DISTRO_NAME = 'PaddockTest';
    context.after(() => {
        if (previous === undefined) delete process.env.WSL_DISTRO_NAME;
        else process.env.WSL_DISTRO_NAME = previous;
    });
    const home = temporaryDirectory(context);
    const users = path.join(home, 'Users');
    const command = fakePencil(path.join(users, 'young'));
    const accounts = new AccountProfiles({ homeDirectory: home, configDirectory: path.join(home, '.paddock', 'config'), claudeDirectory: path.join(home, '.claude'), codexDirectory: path.join(home, '.codex'), windowsUsersDirectory: users });
    const claude = await accounts.create({ provider: 'claude' });
    const first = await accounts.prepare(claude.id);
    const claudeFile = path.join(first.configDir, '.claude.json');
    assert.equal(fs.existsSync(claudeFile), false);
    assert.deepEqual(fs.readdirSync(first.browserDirectory).sort(), [BrowserBridge.NAME, PencilDoctor.NAME].sort());
    assert.equal(fs.readFileSync(path.join(first.browserDirectory, PencilDoctor.NAME), 'utf8'), PencilDoctor.SCRIPT);
    fs.writeFileSync(claudeFile, '{"userID":"kept"}\n');
    await accounts.prepare(claude.id);
    assert.deepEqual(JSON.parse(fs.readFileSync(claudeFile, 'utf8')), { userID: 'kept', mcpServers: { pencil: pencilEntry(command) } });
    const codex = await accounts.create({ provider: 'codex' });
    const prepared = await accounts.prepare(codex.id);
    assert.deepEqual(fs.readdirSync(prepared.browserDirectory), [BrowserBridge.NAME]);
});

test('P06 the WSL preparation script merges the /mnt command into an existing Claude file only', { skip: process.platform === 'win32' || !hasPython }, context => {
    const home = temporaryDirectory(context);
    const id = '00000000-0000-4000-8000-000000000003';
    const configDir = path.join(home, '.paddock', 'agent-profiles', id, 'claude');
    const command = fakePencil(home);
    const run = (provider, pencilCommand) => spawnSync('sh', ['-c', WslScripts.PREPARE, 'paddock-accounts', id, provider, BrowserBridge.NAME, BrowserBridge.SCRIPT, PencilDoctor.NAME, PencilDoctor.SCRIPT, pencilCommand], { encoding: 'utf8', env: { ...process.env, HOME: home } });
    let result = run('claude', command);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fs.existsSync(path.join(configDir, '.claude.json')), false);
    assert.equal(fs.readFileSync(path.join(configDir, 'paddock-bin', PencilDoctor.NAME), 'utf8'), PencilDoctor.SCRIPT);
    fs.writeFileSync(path.join(configDir, '.claude.json'), '{\n    "userID": "kept"\n}\n');
    result = run('claude', command);
    assert.equal(result.status, 0, result.stderr);
    const written = fs.readFileSync(path.join(configDir, '.claude.json'), 'utf8');
    assert.deepEqual(JSON.parse(written), { userID: 'kept', mcpServers: { pencil: pencilEntry(command) } });
    assert.match(written, /^\{\n    "userID"/);
    assert.deepEqual(fs.readdirSync(configDir).filter(name => name.includes('tmp')), []);
    result = run('claude', '');
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(fs.readFileSync(path.join(configDir, '.claude.json'), 'utf8')).mcpServers.pencil.command, command);
});

function doctor(
    context,
    { server, register = true },
) {
    const home = temporaryDirectory(context);
    const configDir = path.join(home, 'claude');
    fs.mkdirSync(configDir);
    const script = path.join(home, 'doctor.sh');
    fs.writeFileSync(script, PencilDoctor.SCRIPT, { mode: 0o700 });
    if (server !== undefined) {
        const serverFile = path.join(home, 'server.js');
        fs.writeFileSync(serverFile, server, { mode: 0o755 });
        if (register) fs.writeFileSync(path.join(configDir, '.claude.json'), JSON.stringify({ mcpServers: { pencil: { type: 'stdio', command: serverFile, args: ['--app', 'paddock'], env: {} } } }));
    }
    const result = spawnSync('sh', [script], { encoding: 'utf8', env: { ...process.env, CLAUDE_CONFIG_DIR: configDir } });
    return { ...result, lines: result.stdout.trim().split('\n') };
}

const SERVER_HEADER = '#!/usr/bin/env node\nconst readline = require("node:readline");\n';
const ANSWERING_SERVER = `${SERVER_HEADER}process.stderr.write("[TransportClient] connected to paddock\\n");
readline.createInterface({ input: process.stdin }).on("line", line => {
    const message = JSON.parse(line);
    if (message.method === "tools/call" && message.params.name === "get_app_state") process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: { content: [] } }) + "\\n");
});
`;

test('P07 the diagnostic reports each layer and exits 0 when the round trip works', { skip: process.platform === 'win32' || !hasPython }, context => {
    const result = doctor(context, { server: ANSWERING_SERVER });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(result.lines.length, 3);
    assert.match(result.lines[0], /^\[OK\] 1\/3 registration/);
    assert.match(result.lines[1], /^\[OK\] 2\/3 connection: \[TransportClient\] connected to paddock/);
    assert.match(result.lines[2], /^\[OK\] 3\/3 round trip/);
});

test('P08 the diagnostic names the failing layer: unregistered, missing program, no pipe, silent editor', { skip: process.platform === 'win32' || !hasPython }, context => {
    let result = doctor(context, {});
    assert.equal(result.status, 1);
    assert.match(result.lines[0], /^\[FAIL\] 1\/3 registration/);
    const home = temporaryDirectory(context);
    const configDir = path.join(home, 'claude');
    fs.mkdirSync(configDir);
    fs.writeFileSync(path.join(configDir, '.claude.json'), JSON.stringify({ mcpServers: { pencil: { command: path.join(home, 'gone.exe') } } }));
    const script = path.join(home, 'doctor.sh');
    fs.writeFileSync(script, PencilDoctor.SCRIPT, { mode: 0o700 });
    result = spawnSync('sh', [script], { encoding: 'utf8', env: { ...process.env, CLAUDE_CONFIG_DIR: configDir } });
    assert.equal(result.status, 1);
    assert.match(result.stdout, /\[FAIL\] 1\/3 registration: .*gone\.exe does not exist/);
    result = doctor(context, { server: `${SERVER_HEADER}process.stderr.write("transport not connected to app: paddock\\n");\n` });
    assert.equal(result.status, 1);
    assert.equal(result.lines.length, 2);
    assert.match(result.lines[1], /^\[FAIL\] 2\/3 connection: .*transport not connected to app/);
});
