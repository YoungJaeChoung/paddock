const test = require('node:test');
const assert = require('node:assert/strict');
const { gitShimPath } = require('../git-shim-location');

test('Windows with the shim built gives its path', () => {
    const seen = [];
    const location = gitShimPath('win32', 'C:\\Users\\me\\AppData\\Local\\Programs\\Paddock\\resources\\app', file => { seen.push(file); return true; });
    assert.equal(location, 'C:\\Users\\me\\AppData\\Local\\Programs\\Paddock\\resources\\app\\paddock-shell\\bin\\git.exe');
    assert.deepEqual(seen, [location]);
});

test('Windows without the shim file gives null so the Windows git stays in use', () => {
    assert.equal(gitShimPath('win32', 'C:\\app', () => false), null);
});

test('other platforms never use the shim', () => {
    assert.equal(gitShimPath('linux', '/app', () => true), null);
    assert.equal(gitShimPath('darwin', '/app', () => true), null);
});
