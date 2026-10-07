const test = require('node:test');
const assert = require('node:assert/strict');
const { createCopyQueue } = require('../copy-queue');

/** 쓰기가 끝나는 시점을 시험이 정하는 가짜 클립보드. */
function manualClipboard() {
    const written = [];
    const pending = [];
    const write = text => new Promise(resolve => pending.push(() => { written.push(text); resolve(); }));
    return { written, write, finishNext: () => pending.shift()?.(), pendingCount: () => pending.length };
}

test('CQ01: an earlier slow write cannot overwrite the latest selection', async () => {
    const clipboard = manualClipboard();
    const enqueue = createCopyQueue(clipboard.write);
    enqueue('ab');
    // 첫 쓰기가 시작된 뒤(아직 안 끝남) 더 새 선택이 두 번 들어온다.
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(clipboard.pendingCount(), 1);
    enqueue('abc');
    let isSettled = false;
    enqueue('abcd').then(() => { isSettled = true; });
    // 쓰기가 끝나는 차례를 시험이 정한다: 대기 중인 쓰기를 하나씩 끝내며 마지막 요청이 끝날 때까지 돈다.
    while (!isSettled) {
        await new Promise(resolve => setImmediate(resolve));
        clipboard.finishNext();
    }
    assert.deepEqual(clipboard.written, ['ab', 'abcd']);
});

test('CQ02: a rejected write does not block later copies', async () => {
    const written = [];
    let shouldFail = true;
    const enqueue = createCopyQueue(async text => {
        if (shouldFail) throw new Error('clipboard busy');
        written.push(text);
    });
    await enqueue('x');
    shouldFail = false;
    await enqueue('y');
    assert.deepEqual(written, ['y']);
});

test('CQ03: repeated identical text is written once per request in order', async () => {
    const written = [];
    const enqueue = createCopyQueue(async text => { written.push(text); });
    enqueue('same');
    await enqueue('same');
    assert.deepEqual(written, ['same', 'same']);
});
