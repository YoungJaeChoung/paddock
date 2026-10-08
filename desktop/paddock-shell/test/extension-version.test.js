const test = require('node:test');
const assert = require('node:assert/strict');
const { compareVersions } = require('../extension-version');

test('EO01: a newer release outranks the installed version, including double-digit parts and prereleases', () => {
    assert.ok(compareVersions('0.6.80', '0.6.74') > 0);
    assert.ok(compareVersions('0.10.0', '0.9.9') > 0);
    assert.ok(compareVersions('1.0.0', '1.0.0-beta') > 0);
    assert.equal(compareVersions('0.6.74', '0.6.74'), 0);
    assert.ok(compareVersions('0.6.74', '0.6.80') < 0);
    assert.ok(compareVersions('1.0.0-beta', '1.0.0') < 0);
});
