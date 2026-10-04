const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { AccountLaunch, accountTerminalOptions, quotePosix } = require('../account-launch');

function preparedAccount(
    provider,
    runtime,
    configDir,
) {
    return {
        profile: { id: '00000000-0000-4000-8000-000000000001', provider, runtime, label: 'Main', ...(runtime === 'wsl' ? { wslDistribution: 'Ubuntu Test' } : {}) },
        configDir,
    };
}

test('L01 launcher persistence contains account metadata and clears credential selectors without copying values', () => {
    for (const provider of ['claude', 'codex']) {
        const prepared = preparedAccount(provider, 'native', '/tmp/account path');
        const options = accountTerminalOptions(prepared, { cwd: '/tmp/work', isWindows: false });
        assert.equal(options.title, `${AccountLaunch.PROVIDERS[provider]} · Main`);
        assert.deepEqual(options.paddockAccount, prepared.profile);
        assert.deepEqual(JSON.parse(JSON.stringify(options)), options);
        assert.equal(options.cwd, '/tmp/work');
        const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
        assert.equal(options.env[variable], prepared.configDir);
        for (const name of AccountLaunch.AUTH_ENV) assert.equal(options.env[name], null);
        assert.equal(options.env.BASH_ENV, null);
        assert.equal(options.env.ENV, null);
    }
});

test('L02 POSIX quoting preserves a path containing apostrophes and shell syntax as literal text', { skip: process.platform === 'win32' }, () => {
    const value = "/tmp/user's path; $(printf unexpected) `printf unexpected` 😀";
    const result = spawnSync('sh', ['-c', `printf '%s' ${quotePosix(value)}`], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, value);
});

test('L03 WSL launch keeps the chosen distribution and passes the working directory as a separate argument', () => {
    const cwd = '/home/test/work with spaces';
    const options = accountTerminalOptions(preparedAccount('codex', 'wsl', '/home/test/accounts/codex'), { cwd, isWindows: true, wslEnv: 'PADDOCK_TERMINAL_ID/u:EXISTING/p' });
    assert.deepEqual(options.shellArgs.slice(0, 6), ['-d', 'Ubuntu Test', '--cd', cwd, '-e', '/bin/bash']);
    assert.equal(options.env.WSLENV, 'PADDOCK_TERMINAL_ID/u:EXISTING/p');
    assert.throws(() => accountTerminalOptions(preparedAccount('claude', 'wsl', '/home/test/accounts/claude'), { cwd, isWindows: false }), /WSL account/);
});

test('L04 account path controls and unsupported providers fail before constructing executable input', () => {
    for (const configDir of ['', '/tmp/a\nb', '/tmp/a\0b']) {
        assert.throws(() => accountTerminalOptions(preparedAccount('claude', 'native', configDir), { cwd: '/tmp', isWindows: false }), /configuration path is invalid/);
    }
    assert.throws(() => accountTerminalOptions(preparedAccount('other', 'native', '/tmp/profile'), { cwd: '/tmp', isWindows: false }), /Claude or Codex/);
});

test('L05 Windows encoded startup preserves supplementary Unicode and quotes account paths', () => {
    const prepared = preparedAccount('codex', 'native', "C:\\Users\\O'Brien 😀\\account");
    const options = accountTerminalOptions(prepared, { cwd: 'C:\\work', isWindows: true });
    const decoded = Buffer.from(options.shellArgs.at(-1), 'base64').toString('utf16le');
    assert.ok(decoded.includes("$env:CODEX_HOME = 'C:\\Users\\O''Brien 😀\\account'"));
    assert.ok(options.shellArgs.includes('-NoExit'));
});

test('L06 Windows official-command selection launches a fixture CLI and retains the account after it exits', { skip: process.platform !== 'win32' }, context => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-account-launch-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const reportScript = [
        "const provider = process.argv[2];",
        "const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';",
        `const selectors = ${JSON.stringify(AccountLaunch.AUTH_ENV)};`,
        "process.stdout.write('ACCOUNT_FIXTURE|' + Buffer.from(JSON.stringify({ provider, configDir: process.env[variable], selectorsPresent: selectors.filter(name => process.env[name] !== undefined) })).toString('base64') + '\\n');",
    ].join('\n');
    fs.writeFileSync(path.join(directory, 'report.cjs'), reportScript);
    for (const provider of ['claude', 'codex']) {
        fs.writeFileSync(path.join(directory, `${provider}.cmd`), `@"${process.execPath}" "%~dp0report.cjs" ${provider}\r\n`);
        const configDir = path.join(directory, "account ' ; Write-Output UNEXPECTED ; 😀");
        const prepared = preparedAccount(provider, 'native', configDir);
        const options = accountTerminalOptions(prepared, { cwd: directory, isWindows: true });
        const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
        const env = { SystemRoot: process.env.SystemRoot, ComSpec: process.env.ComSpec, PATHEXT: process.env.PATHEXT, PATH: `${directory};${process.env.SystemRoot}\\System32`, ...Object.fromEntries(AccountLaunch.AUTH_ENV.map(name => [name, 'fixture-only'])) };
        // NoProfile isolates this command execution check from the user's PowerShell setup.
        const result = spawnSync(options.shellPath, ['-NoProfile', ...options.shellArgs], {
            cwd: directory, env, encoding: 'utf8', timeout: 15000,
            input: `[Console]::WriteLine('AFTER_ACCOUNT|' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($env:${variable}))); exit 0\r\n`,
        });
        assert.equal(result.status, 0, result.stderr);
        const marker = result.stdout.split(/\r?\n/).find(line => line.startsWith('ACCOUNT_FIXTURE|'));
        assert.ok(marker, result.stdout);
        assert.deepEqual(JSON.parse(Buffer.from(marker.slice('ACCOUNT_FIXTURE|'.length), 'base64').toString('utf8')), { provider, configDir, selectorsPresent: [] });
        assert.ok(result.stdout.includes(`AFTER_ACCOUNT|${Buffer.from(configDir).toString('base64')}`), result.stdout);
    }
});

test('L07 WSL browser selection survives shell startup and keeps helper paths containing spaces', { skip: process.platform === 'win32' }, context => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock browser path '));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const helper = path.join(directory, 'paddock-open-browser');
    fs.writeFileSync(helper, '#!/bin/sh\nexit 0\n', { mode: 0o700 });
    for (const provider of ['claude', 'codex']) {
        const prepared = { ...preparedAccount(provider, 'native', directory), browserDirectory: directory };
        const options = accountTerminalOptions(prepared, { cwd: directory, isWindows: false });
        // An inherited command function supplies a fixture CLI without changing the user's shell files.
        const command = '() { case "$1" in claude|codex) printf "BROWSER_FIXTURE|%s|%s\\n" "$BROWSER" "$(builtin command -v "$BROWSER")" ;; -v) if [ "$2" = claude ] || [ "$2" = codex ]; then printf "%s\\n" "$2"; else builtin command "$@"; fi ;; *) builtin command "$@" ;; esac; }';
        const env = { ...process.env, BROWSER: 'true', HISTFILE: path.join(directory, 'history'), 'BASH_FUNC_command%%': command };
        for (const [name, value] of Object.entries(options.env)) {
            if (value === null) delete env[name];
            else env[name] = value;
        }
        const result = spawnSync(options.shellPath, options.shellArgs, { env, cwd: directory, input: 'exit\n', encoding: 'utf8', timeout: 10000 });
        assert.equal(result.status, 0, result.stderr);
        assert.ok(result.stdout.includes(`BROWSER_FIXTURE|paddock-open-browser|${helper}`), result.stdout + result.stderr);
    }
});
