const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { BrowserBridge } = require('../account-browser');

test('B01 browser address stays on stdin and cannot become executable shell source', { skip: process.platform === 'win32' }, context => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock browser '));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const opener = path.join(directory, BrowserBridge.NAME);
    fs.writeFileSync(opener, BrowserBridge.SCRIPT, { mode: 0o700 });
    fs.writeFileSync(path.join(directory, 'powershell.exe'), '#!/bin/sh\ncat > "$BROWSER_FIXTURE_STDIN"\nprintf "%s\\n" "$@" > "$BROWSER_FIXTURE_ARGS"\n', { mode: 0o700 });
    const stdinFile = path.join(directory, 'stdin');
    const argsFile = path.join(directory, 'args');
    const address = "https://example.com/login?state=a&redirect=%2Fpath#'\";$(touch SHOULD_NOT_EXIST)`touch SHOULD_NOT_EXIST` 😀";
    const env = { ...process.env, PATH: `${directory}:/usr/bin:/bin`, BROWSER_FIXTURE_STDIN: stdinFile, BROWSER_FIXTURE_ARGS: argsFile };
    const result = spawnSync(opener, [address], { env, cwd: directory, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fs.readFileSync(stdinFile, 'utf8'), address);
    assert.equal(fs.readFileSync(argsFile, 'utf8').includes(address), false);
    assert.equal(fs.existsSync(path.join(directory, 'SHOULD_NOT_EXIST')), false);
    assert.equal(result.stdout + result.stderr, '');
    for (const invalid of ['file:///tmp/example', 'javascript:alert(1)', '-Command', '']) {
        const rejected = spawnSync(opener, [invalid], { env, encoding: 'utf8' });
        assert.notEqual(rejected.status, 0);
    }
});

test('B02 failed Windows opener reports recovery without echoing the address', { skip: process.platform === 'win32' }, context => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-browser-failure-'));
    context.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const opener = path.join(directory, BrowserBridge.NAME);
    fs.writeFileSync(opener, BrowserBridge.SCRIPT, { mode: 0o700 });
    fs.writeFileSync(path.join(directory, 'powershell.exe'), '#!/bin/sh\ncat >&2\nexit 1\n', { mode: 0o700 });
    const address = 'https://example.com/login?state=fixture-private-value';
    const result = spawnSync(opener, [address], { env: { ...process.env, PATH: `${directory}:/usr/bin:/bin` }, encoding: 'utf8' });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Copy the login URL into your Windows browser/);
    assert.equal((result.stdout + result.stderr).includes('fixture-private-value'), false);
});
