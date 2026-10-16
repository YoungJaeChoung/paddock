const test = require('node:test');
const assert = require('node:assert/strict');
const { PaddockOutputStream } = require('../terminal-output-stream');

/** 버퍼를 만들고, 보낸 조각을 `[보낸 시각(ms), 내용]`으로 모은다. 시계는 node:test의 가짜 타이머다. */
function recorded(
    t,
    options = {},
) {
    t.mock.timers.enable({ apis: ['setTimeout', 'Date'] });
    const stream = new PaddockOutputStream(options);
    const sent = [];
    stream.onData(chunk => sent.push([Date.now(), chunk]));
    return { stream, sent };
}

test('조용하던 중의 첫 출력(키 에코)은 타이머를 기다리지 않고 바로 보낸다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    assert.deepEqual(sent, [[0, 'a']]);
});

test('묶음 간격 안에 몰려온 출력은 간격이 끝날 때 한 번에 보낸다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    t.mock.timers.tick(5);
    stream.push('b');
    t.mock.timers.tick(5);
    stream.push('c');
    t.mock.timers.tick(6);
    assert.deepEqual(sent, [[0, 'a'], [16, 'bc']]);
});

test('간격이 끝날 때 모인 출력이 없으면 조용한 상태로 돌아가 다음 출력을 바로 보낸다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    t.mock.timers.tick(40);
    stream.push('b');
    assert.deepEqual(sent, [[0, 'a'], [40, 'b']]);
});

test('출력이 이어지면 간격마다 이어서 묶는다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    t.mock.timers.tick(5);
    stream.push('b');
    // 가짜 시계는 한 번의 tick 안에서 불린 콜백에도 tick이 끝난 시각을 알려 주므로, 타이머가 끝나는 16ms에 맞춰 나눠 민다.
    t.mock.timers.tick(11);
    t.mock.timers.tick(4);
    stream.push('c');
    t.mock.timers.tick(12);
    assert.deepEqual(sent, [[0, 'a'], [16, 'b'], [32, 'c']]);
});

test('한 번에 보내는 크기는 maxChunkSize 이하로 자르고 나머지는 다음 간격에 보낸다', (t) => {
    const { stream, sent } = recorded(t, { maxChunkSize: 3 });
    stream.push('abcdefg');
    t.mock.timers.tick(16);
    t.mock.timers.tick(16);
    assert.deepEqual(sent, [[0, 'abc'], [16, 'def'], [32, 'g']]);
});

test('flush는 남은 출력을 모두 보내고 다음 출력을 바로 보낸다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    stream.push('b');
    stream.flush();
    stream.push('c');
    assert.deepEqual(sent, [[0, 'a'], [0, 'b'], [0, 'c']]);
});

test('dispose 뒤에는 모아 둔 출력을 보내지 않는다', (t) => {
    const { stream, sent } = recorded(t);
    stream.push('a');
    stream.push('b');
    stream.dispose();
    t.mock.timers.tick(32);
    assert.deepEqual(sent, [[0, 'a']]);
});
