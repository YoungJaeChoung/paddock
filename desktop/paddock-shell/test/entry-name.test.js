const test = require('node:test');
const assert = require('node:assert/strict');
const { validateEntryName, copyEntryName } = require('../entry-name');

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

test('같은 이름이 있으면 덮어쓰지 않고 copy·copy n을 확장자 앞에 붙인다', () => {
    assert.equal(copyEntryName('image.png', 0), 'image.png');
    assert.equal(copyEntryName('image.png', 1), 'image copy.png');
    assert.equal(copyEntryName('image.png', 2), 'image copy 2.png');
    assert.equal(copyEntryName('a.tar.gz', 1), 'a.tar copy.gz');
    assert.equal(copyEntryName('.env', 1), '.env copy');
    assert.equal(copyEntryName('Makefile', 3), 'Makefile copy 3');
});
