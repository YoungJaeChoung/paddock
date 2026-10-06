const test = require('node:test');
const assert = require('node:assert/strict');
const { validateEntryName } = require('../entry-name');

test('빈 이름은 경고 없이 확인만 막고, 규칙을 어긴 이름만 이유를 알린다', () => {
    assert.equal(validateEntryName('notes.md'), '');
    assert.equal(validateEntryName('한글 폴더'), '');
    assert.equal(validateEntryName(''), false);
    assert.equal(validateEntryName('   '), false);
    assert.match(validateEntryName('a/b'), /\/ or \\/);
    assert.match(validateEntryName('a\\b'), /\/ or \\/);
    assert.match(validateEntryName('..'), /reserved/);
    assert.match(validateEntryName('a\tb'), /control characters/);
});
