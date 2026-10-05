const test = require('node:test');
const assert = require('node:assert/strict');
const { distributionOf, withDistribution, environmentLabel } = require('../terminal-environment');

test('a WSL project selects its own distribution instead of the default launcher', () => {
    const options = { shellPath: 'wsl.exe', shellArgs: ['-d', 'Ubuntu'] };
    assert.equal(distributionOf(options, 'file://wsl.localhost/Debian/home/me/project'), 'Debian');
    assert.equal(distributionOf(options, 'file://wsl$/Ubuntu%2024/home/me'), 'Ubuntu 24');
    assert.deepEqual(withDistribution(['--cd', '~', '-d', 'Ubuntu', '--exec', 'bash'], 'Debian'), ['--distribution', 'Debian', '--cd', '~', '--exec', 'bash']);
});

test('environment labels keep Windows shells and WSL distributions distinguishable', () => {
    assert.equal(environmentLabel({ shellPath: 'C:\\Windows\\System32\\wsl.exe', shellArgs: ['-d', 'Debian'] }, '', 'Windows'), 'WSL · Debian');
    assert.equal(environmentLabel({ shellPath: 'powershell.exe' }, '', 'Windows'), 'PowerShell · Windows');
    assert.equal(environmentLabel({ shellPath: 'C:\\Program Files\\Git\\bin\\bash.exe' }, '', 'Windows'), 'Git Bash · Windows');
    assert.equal(environmentLabel({ shellPath: '/bin/bash' }, '', 'Linux'), 'bash · Linux');
});

test('adding a distribution preserves other options and removes duplicate distribution flags', () => {
    assert.deepEqual(withDistribution(['--distribution=Ubuntu', '--cd', '/home/me'], 'Debian'), ['--distribution', 'Debian', '--cd', '/home/me']);
    assert.equal(distributionOf({ shellArgs: ['--distribution', 'Debian'] }), 'Debian');
    assert.equal(distributionOf({ paddockAccount: { wslDistribution: 'Ubuntu' } }), 'Ubuntu');
});
