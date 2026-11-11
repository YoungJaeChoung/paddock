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

// The launcher is a shell function, not a PATH entry: a login shell runs ~/.profile after ~/.bashrc, and Ubuntu's ~/.profile (and Claude Code's installer line)
// put ~/.local/bin back in front of PATH, so a PATH entry added in ~/.bashrc loses to the real claude. A function is found before PATH.
const LINE = 'claude() { if [ -x "$HOME/.paddock/config/bin/claude" ]; then "$HOME/.paddock/config/bin/claude" "$@"; else command claude "$@"; fi; } # paddock-claude-wrapper';
const OLD_LINE = 'export PATH="$HOME/.paddock/config/bin:$PATH" # paddock-claude-wrapper';

// Ubuntu's default ~/.profile: reads ~/.bashrc first, then puts ~/.local/bin in front of PATH.
const UBUNTU_PROFILE = `if [ -n "$BASH_VERSION" ]; then
    if [ -f "$HOME/.bashrc" ]; then
        . "$HOME/.bashrc"
    fi
fi
if [ -d "$HOME/.local/bin" ] ; then
    PATH="$HOME/.local/bin:$PATH"
fi
`;

/** A temporary HOME like a Claude Code user's: the real claude in ~/.local/bin (found by the installer's PATH line), Ubuntu's ~/.profile, and Paddock's launcher installed. */
function userHome(
    context,
    { launcher = true } = {},
) {
    const directory = home(context);
    fs.mkdirSync(path.join(directory, '.local', 'bin'), { recursive: true });
    fs.writeFileSync(path.join(directory, '.local', 'bin', 'claude'), '#!/bin/sh\nprintf "REAL|%s\\n" "$*"\n', { mode: 0o755 });
    fs.mkdirSync(path.join(directory, '.paddock', 'config', 'claude'), { recursive: true });
    fs.writeFileSync(path.join(directory, '.paddock', 'config', 'claude', 'flag-settings.json'), '{}\n');
    if (launcher) run(directory, 'install', claudeWrapperScript({ kind: 'sh', flagFile: path.join(directory, '.paddock', 'config', 'claude', 'flag-settings.json') }));
    fs.writeFileSync(path.join(directory, '.profile'), UBUNTU_PROFILE);
    // The Claude Code installer's line and a typical interactive-shell guard, as in a stock Ubuntu ~/.bashrc.
    fs.writeFileSync(path.join(directory, '.bashrc'), 'case $- in\n    *i*) ;;\n      *) return;;\nesac\nalias ll="ls -l"\nexport PATH="$HOME/.local/bin:$PATH"\n');
    return directory;
}

/** Runs bash with a clean environment and returns stdout lines. */
function bash(
    directory,
    flags,
    command,
) {
    const result = spawnSync('bash', [flags, '-c', command], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: directory }, timeout: 15000 });
    return result.stdout.split('\n');
}

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
    const script = claudeWrapperScript({ kind: 'sh', flagFile: `${directory}/.paddock/config/claude/flag-settings.json`, mcpConfigFile: `${directory}/.paddock/config/claude/pen-mcp.json` });
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

test('WL05 the added function line puts the launcher first in a real interactive bash', { skip: process.platform === 'win32' }, context => {
    const directory = home(context);
    const real = path.join(directory, 'real');
    fs.mkdirSync(real);
    fs.writeFileSync(path.join(real, 'claude'), '#!/bin/sh\nprintf "REAL|%s\\n" "$*"\n', { mode: 0o755 });
    run(directory, 'install', claudeWrapperScript({ kind: 'sh', flagFile: path.join(directory, 'missing.json'), mcpConfigFile: path.join(directory, 'pen-mcp.json') }));
    fs.writeFileSync(path.join(directory, '.bashrc'), `export PATH="${real}:$PATH"\n`);
    run(directory, 'add');
    const result = spawnSync('bash', ['-i', '-c', 'command -v claude; claude --version'], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: directory }, timeout: 10000 });
    // The added line is a function that calls the launcher by its full path, so `claude` resolves to the function (not to the `real` folder) and still reaches the real claude.
    assert.match(result.stdout, /(^|\n)claude\n/, result.stdout);
    assert.match(result.stdout, /REAL\|--version/);
});

test('WL07 in a login shell (~/.profile after ~/.bashrc) and in a plain interactive shell the launcher comes before the real claude', { skip: process.platform === 'win32' }, context => {
    const directory = userHome(context);
    run(directory, 'add');
    const wrapper = path.join(directory, '.paddock', 'config', 'bin', 'claude');
    const settings = path.join(directory, '.paddock', 'config', 'claude', 'flag-settings.json');
    for (const flags of ['-lic', '-ic']) {
        const lines = bash(directory, flags, 'type -a claude; claude --version');
        const types = lines.filter(line => line.startsWith('claude is ') || line.startsWith('claude '));
        assert.match(lines.join('\n'), /claude is a function/, `${flags}: ${lines.join(' | ')}`);
        assert.ok(types.findIndex(line => line.includes('a function')) < types.findIndex(line => line.includes(path.join(directory, '.local', 'bin', 'claude'))), `${flags}: function first`);
        assert.match(lines.join('\n'), new RegExp(`REAL\\|--settings ${settings.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} --version`), `${flags}: launcher added --settings: ${lines.join(' | ')}`);
    }
    assert.ok(fs.existsSync(wrapper));
});

test('WL08 the old PATH line alone loses to ~/.profile in a login shell, and adding upgrades it to the function in place', { skip: process.platform === 'win32' }, context => {
    const directory = userHome(context);
    const rc = path.join(directory, '.bashrc');
    fs.appendFileSync(rc, `${OLD_LINE}\n`);
    const before = bash(directory, '-lic', 'claude --version').join('\n');
    assert.doesNotMatch(before, /--settings/, 'the old line leaves the real claude first in a login shell');
    assert.deepEqual(run(directory, 'add').items, ['updated', rc, 'no']);
    const text = fs.readFileSync(rc, 'utf8');
    assert.ok(text.includes(`${LINE}\n`));
    assert.ok(!text.includes(OLD_LINE));
    assert.equal(text.split('# paddock-claude-wrapper').length - 1, 1, 'one marked line');
    assert.ok(text.startsWith('case $-'), 'other lines stay in place');
    assert.match(bash(directory, '-lic', 'claude --version').join('\n'), /--settings/);
    assert.deepEqual(run(directory, 'add').items, ['present', rc, 'no']);
});

test('WL09 removing restores the original ~/.bashrc and the real claude runs again; a missing launcher file falls back to the real claude', { skip: process.platform === 'win32' }, context => {
    const directory = userHome(context);
    const rc = path.join(directory, '.bashrc');
    const original = fs.readFileSync(rc, 'utf8');
    run(directory, 'add');
    fs.rmSync(path.join(directory, '.paddock', 'config', 'bin', 'claude'));
    const fallback = bash(directory, '-lic', 'claude --version').join('\n');
    assert.match(fallback, /REAL\|--version/, 'no launcher file: the real claude still runs');
    assert.deepEqual(run(directory, 'remove').items, ['removed', rc, 'no']);
    assert.equal(fs.readFileSync(rc, 'utf8'), original);
    assert.doesNotMatch(bash(directory, '-lic', 'type -a claude').join('\n'), /function/);
    // A user who only has the old line gets it removed too.
    fs.appendFileSync(rc, `${OLD_LINE}\n`);
    assert.equal(run(directory, 'remove').items[0], 'removed');
    assert.equal(fs.readFileSync(rc, 'utf8'), original);
});

test('WL06 the output parser maps each shape and falls back to unsupported', () => {
    assert.deepEqual(parseLauncherOutput(['installed']), { state: 'installed', file: '', isLinked: false });
    assert.deepEqual(parseLauncherOutput(['added', '/home/me/.bashrc', 'no']), { state: 'added', file: '/home/me/.bashrc', isLinked: false });
    assert.deepEqual(parseLauncherOutput(['present', '/repo/bashrc', 'yes']), { state: 'present', file: '/repo/bashrc', isLinked: true });
    assert.deepEqual(parseLauncherOutput([]), { state: 'unsupported', file: '', isLinked: false });
});
