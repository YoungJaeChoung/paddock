const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { WslClaudeLauncher, parseLauncherOutput } = require('../wsl-claude-launcher');
const { claudeWrapperScript } = require('../claude-wrapper');

function home(
    context,
) {
    const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'wsl-launcher-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    return directory;
}

/** Runs the fixed script the way runWsl does (sh -c <script> <name> <args…>) with a temporary HOME. */
function run(
    homeDirectory,
    ...argumentsList
) {
    const result = spawnSync('sh', ['-c', WslClaudeLauncher.SCRIPT, 'paddock-launcher', ...argumentsList], { encoding: 'utf8', env: { PATH: process.env.PATH, HOME: homeDirectory } });
    return { status: result.status, stderr: result.stderr, items: result.stdout.split('\0').filter(Boolean) };
}

const LINE = 'export PATH="$HOME/.paddock/config/bin:$PATH" # paddock-claude-wrapper';

test('WL01 the script passes shell syntax checking and its marker matches the constant', { skip: process.platform === 'win32' }, () => {
    const result = spawnSync('sh', ['-n', '-c', WslClaudeLauncher.SCRIPT], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.ok(WslClaudeLauncher.SCRIPT.includes(`marker='${WslClaudeLauncher.MARKER}'`));
});

test('WL02 adding writes one marked line, never twice, after a missing final newline too; removing deletes exactly that line', { skip: process.platform === 'win32' }, context => {
    const directory = home(context);
    const rc = path.join(directory, '.bashrc');
    assert.deepEqual(run(directory, 'add').items, ['added', rc, 'no']);
    assert.equal(fs.readFileSync(rc, 'utf8'), `${LINE}\n`);
    assert.deepEqual(run(directory, 'add').items, ['present', rc, 'no']);
    assert.equal(fs.readFileSync(rc, 'utf8'), `${LINE}\n`);
    assert.deepEqual(run(directory, 'remove').items, ['removed', rc, 'no']);
    assert.equal(fs.readFileSync(rc, 'utf8'), '');
    assert.deepEqual(run(directory, 'remove').items, ['absent', rc, 'no']);
    fs.writeFileSync(rc, 'alias ll="ls -l"\nexport EDITOR=vim');
    assert.equal(run(directory, 'add').items[0], 'added');
    assert.equal(fs.readFileSync(rc, 'utf8'), `alias ll="ls -l"\nexport EDITOR=vim\n${LINE}\n`);
    assert.equal(run(directory, 'remove').items[0], 'removed');
    assert.equal(fs.readFileSync(rc, 'utf8'), 'alias ll="ls -l"\nexport EDITOR=vim\n');
});

test('WL03 a linked .bashrc is edited at its real location, the link stays, and the output says the file is a link', { skip: process.platform === 'win32' }, context => {
    const directory = home(context);
    const repository = path.join(directory, 'dotfiles', 'bashrc');
    fs.mkdirSync(path.dirname(repository));
    fs.writeFileSync(repository, 'export A=1\n');
    fs.symlinkSync(repository, path.join(directory, '.bashrc'));
    assert.deepEqual(run(directory, 'add').items, ['added', repository, 'yes']);
    assert.equal(fs.readFileSync(repository, 'utf8'), `export A=1\n${LINE}\n`);
    assert.equal(fs.lstatSync(path.join(directory, '.bashrc')).isSymbolicLink(), true);
    assert.deepEqual(run(directory, 'remove').items, ['removed', repository, 'yes']);
    assert.equal(fs.readFileSync(repository, 'utf8'), 'export A=1\n');
    fs.rmSync(repository);
    assert.equal(run(directory, 'add').items[0], 'blocked', 'a dangling link is left alone');
    assert.equal(fs.existsSync(repository), false);
});

test('WL04 installing puts an executable launcher file in place, replaces an old one, and refuses a link', { skip: process.platform === 'win32' }, context => {
    const directory = home(context);
    const script = claudeWrapperScript({ kind: 'sh', flagFile: `${directory}/.paddock/config/claude/flag-settings.json` });
    assert.deepEqual(run(directory, 'install', script).items, ['installed']);
    const file = path.join(directory, '.paddock', 'config', 'bin', 'claude');
    assert.equal(fs.readFileSync(file, 'utf8'), script);
    assert.equal(fs.statSync(file).mode & 0o777, 0o700);
    fs.writeFileSync(file, '#!/bin/sh\n# old\n');
    assert.deepEqual(run(directory, 'install', script).items, ['installed']);
    assert.equal(fs.readFileSync(file, 'utf8'), script);
    assert.deepEqual(fs.readdirSync(path.dirname(file)), ['claude']);
    fs.rmSync(file);
    fs.symlinkSync('/bin/true', file);
    assert.equal(run(directory, 'install', script).status, 3);
    assert.equal(run(directory, 'unknown').status, 2);
});

test('WL05 the added PATH line puts the launcher first in a real interactive bash', { skip: process.platform === 'win32' }, context => {
    const directory = home(context);
    const real = path.join(directory, 'real');
    fs.mkdirSync(real);
    fs.writeFileSync(path.join(real, 'claude'), '#!/bin/sh\nprintf "REAL|%s\\n" "$*"\n', { mode: 0o755 });
    run(directory, 'install', claudeWrapperScript({ kind: 'sh', flagFile: path.join(directory, 'missing.json') }));
    fs.writeFileSync(path.join(directory, '.bashrc'), `export PATH="${real}:$PATH"\n`);
    run(directory, 'add');
    const result = spawnSync('bash', ['-i', '-c', 'command -v claude; claude --version'], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: directory }, timeout: 10000 });
    // The system-wide bash startup may print its own lines first, so look for the launcher path among the output lines.
    assert.ok(result.stdout.split('\n').includes(path.join(directory, '.paddock', 'config', 'bin', 'claude')), result.stdout);
    assert.match(result.stdout, /REAL\|--version/);
});

test('WL06 the output parser maps each shape and falls back to unsupported', () => {
    assert.deepEqual(parseLauncherOutput(['installed']), { state: 'installed', file: '', isLinked: false });
    assert.deepEqual(parseLauncherOutput(['added', '/home/me/.bashrc', 'no']), { state: 'added', file: '/home/me/.bashrc', isLinked: false });
    assert.deepEqual(parseLauncherOutput(['present', '/repo/bashrc', 'yes']), { state: 'present', file: '/repo/bashrc', isLinked: true });
    assert.deepEqual(parseLauncherOutput([]), { state: 'unsupported', file: '', isLinked: false });
});
