const test = require('node:test');
const assert = require('node:assert/strict');
const { WSL_LAUNCHER_KEY, wslLauncherPlan } = require('../wsl-launcher-plan');

test('WP01 the start-up plan asks once only on Windows with WSL, re-checks an allowed line, and leaves a declined setting alone', () => {
    const windows = { isWindows: true, isWslReady: true };
    assert.equal(wslLauncherPlan('ask', windows), 'prompt');
    assert.equal(wslLauncherPlan('on', windows), 'add');
    assert.equal(wslLauncherPlan('off', windows), 'none');
    assert.equal(wslLauncherPlan('ask', { isWindows: true, isWslReady: false }), 'none');
    assert.equal(wslLauncherPlan('on', { isWindows: true, isWslReady: false }), 'none');
    assert.equal(wslLauncherPlan('ask', { isWindows: false, isWslReady: false }), 'none');
    assert.equal(wslLauncherPlan('unexpected', windows), 'none');
    assert.equal(WSL_LAUNCHER_KEY, 'paddock.wsl.claudeLauncher');
});
