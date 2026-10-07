const test = require('node:test');
const assert = require('node:assert/strict');
const { osc52ClipboardText } = require('../terminal-copy');

test('TC01: a clipboard write request yields its UTF-8 text', () => {
    assert.equal(osc52ClipboardText('c;aGVsbG8='), 'hello');
    assert.equal(osc52ClipboardText(';7ZWc6riA'), '한글');
    // 끝 채움(=)이 빠진 본문도 받는다.
    assert.equal(osc52ClipboardText('c;aGVsbG8'), 'hello');
});

test('TC02: read requests and malformed bodies copy nothing', () => {
    for (const data of ['c;?', 'c', 'c;', 'c;!!!', 'c;/w==']) assert.equal(osc52ClipboardText(data), null, data);
});
