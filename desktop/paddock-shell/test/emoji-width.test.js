const test = require('node:test');
const assert = require('node:assert/strict');
const { createEmojiWidthFixer } = require('../emoji-width');

test('한 칸 기호 + 이모지 표시 문자 뒤에만 빈칸을 넣는다', () => {
    const fix = createEmojiWidthFixer();
    assert.equal(fix('⚠️전제'), '⚠️ 전제');
    assert.equal(fix('✔️ ❤️'), '✔️  ❤️ ');
    // 원래 두 칸인 이모지, 표시 문자 없는 기호, 일반 글자는 그대로 둔다.
    assert.equal(fix('✅️완료 \u{1f600} ⚠ 가'), '✅️완료 \u{1f600} ⚠ 가');
});

test('숫자·#·*는 키캡 표시까지 붙어야 빈칸을 넣는다', () => {
    const fix = createEmojiWidthFixer();
    assert.equal(fix('1️⃣번'), '1️⃣ 번');
    assert.equal(fix('1️번'), '1️번');
});

test('기호와 표시 문자가 다른 조각에 걸쳐도 빈칸을 넣는다', () => {
    const fix = createEmojiWidthFixer();
    assert.equal(fix('표 ⚠'), '표 ⚠');
    assert.equal(fix('️전제'), '️ 전제');
    assert.equal(fix('️다시'), '️다시');
});
