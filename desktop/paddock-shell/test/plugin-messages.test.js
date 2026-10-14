const test = require('node:test');
const assert = require('node:assert/strict');
const { coalesceMessage } = require('../message-coalesce');
const { loggedLines } = require('../plugin-output-log');

/** 호출 횟수를 세고, 직접 닫을 수 있는 열기 함수. */
function fakeOpen() {
    const calls = [];
    const open = () => new Promise((resolve) => { calls.push(resolve); });
    return { open, calls };
}

test('A-messages.1: 같은 문구가 열려 있는 동안 다시 와도 한 번만 열고 같은 결과를 돌려준다', async () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    const first = coalesceMessage(pending, 'Git: fatal', open);
    const second = coalesceMessage(pending, 'Git: fatal', open);
    assert.equal(calls.length, 1);
    assert.equal(first, second);
    calls[0]('Close');
    assert.equal(await first, 'Close');
});

test('A-messages.2: 다른 문구는 각각 연다', () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    coalesceMessage(pending, 'a', open);
    coalesceMessage(pending, 'b', open);
    assert.equal(calls.length, 2);
});

test('A-messages.3: 닫힌 뒤 같은 문구가 다시 오면 다시 연다', async () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    const first = coalesceMessage(pending, 'a', open);
    calls[0](undefined);
    await first;
    assert.equal(pending.size, 0);
    coalesceMessage(pending, 'a', open);
    assert.equal(calls.length, 2);
});

test('A-messages.4: Git 채널의 오류·경고 줄만 로그로 남긴다', () => {
    const text = [
        '2026-10-14T00:00:00.000Z [Info] > git status [3ms]',
        '2026-10-14T00:00:01.000Z [Error] fatal: mmap: could not determine filesize',
        '2026-10-14T00:00:02.000Z [Warning] git fetch failed',
        '',
    ].join('\n');
    assert.deepEqual(loggedLines('Git', text), [
        '2026-10-14T00:00:01.000Z [Error] fatal: mmap: could not determine filesize',
        '2026-10-14T00:00:02.000Z [Warning] git fetch failed',
    ]);
    assert.deepEqual(loggedLines('Tasks', text), []);
    assert.deepEqual(loggedLines('Git', undefined), []);
});
