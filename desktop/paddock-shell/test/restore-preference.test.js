const test = require('node:test');
const assert = require('node:assert/strict');
const { readRestoreOnRestart, RESTORE } = require('../restore-preference');

test('되살리기 설정: never만 끄고, 없거나 알 수 없는 값·주석 처리된 값은 always다', () => {
    assert.equal(readRestoreOnRestart('{ "paddock.terminal.restoreOnRestart": "never" }'), RESTORE.NEVER);
    assert.equal(readRestoreOnRestart('{ "paddock.terminal.restoreOnRestart": "always" }'), RESTORE.ALWAYS);
    assert.equal(readRestoreOnRestart('{ "paddock.terminal.restoreOnRestart": "x" }'), RESTORE.ALWAYS);
    assert.equal(readRestoreOnRestart('{}'), RESTORE.ALWAYS);
    assert.equal(readRestoreOnRestart(''), RESTORE.ALWAYS);
    assert.equal(readRestoreOnRestart(undefined), RESTORE.ALWAYS);
    assert.equal(readRestoreOnRestart('{\n // "paddock.terminal.restoreOnRestart": "never"\n}'), RESTORE.ALWAYS);
});
