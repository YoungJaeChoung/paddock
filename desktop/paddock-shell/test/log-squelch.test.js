const test = require('node:test');
const assert = require('node:assert/strict');
const { SQUELCH, createSquelchState, splitUnits, squelchKey, flushExpired, squelch, installLogSquelch } = require('../log-squelch');

const line = (n, text = 'root WARN No reply handler for') => `2026-10-14T05:24:${String(n % 60).padStart(2, '0')}.123Z ${text} ${n}\n`;

/** 줄들을 차례로 넣어 통과한 줄 수와 앞에 붙은 요약·안내를 모은다. */
function feed(state, lines, now) {
    let n_passed = 0;
    let prefixes = '';
    for (const text of lines) {
        const decision = squelch(state, text, now);
        prefixes += decision.prefix;
        if (decision.passed) n_passed += 1;
    }
    return { n_passed, prefixes };
}

test('C-squelch.1: 시각·번호만 다른 같은 틀은 같은 열쇠다', () => {
    assert.equal(squelchKey(line(17)), squelchKey(line(99)));
    assert.notEqual(squelchKey(line(1)), squelchKey(line(1, 'root WARN other message')));
    assert.equal(squelchKey('[4b0ba821-b495-4f57-b8ea-5411ad21f9b7] Starting\n'), '[ID] Starting');
});

test('C-squelch.2: 같은 틀 100줄은 5줄만 통과하고 창이 끝나면 버린 95줄을 처음·마지막 원문과 함께 요약 한 줄로 남긴다', () => {
    const state = createSquelchState();
    const lines = Array.from({ length: 100 }, (unused, index) => line(index));
    const { n_passed, prefixes } = feed(state, lines, 1000);
    assert.equal(n_passed, 5);
    assert.equal(prefixes, '');
    const summary = flushExpired(state, 1000 + SQUELCH.WINDOW_MS);
    assert.match(summary, /repeated 95 more times in 10 s; first: 2026-10-14T05:24:05\.123Z .* 5 \| last: 2026-10-14T05:24:39\.123Z .* 99/);
    assert.equal(summary.split('\n').length, 2);
    assert.equal(flushExpired(state, 1000 + SQUELCH.WINDOW_MS), '');
});

test('C-squelch.3: 다른 틀의 출력은 서로 영향 없이 통과한다', () => {
    const state = createSquelchState();
    const lines = Array.from({ length: 50 }, (unused, index) => `message-type-${String.fromCharCode(97 + (index % 26))}${'x'.repeat(index)} \n`);
    assert.equal(feed(state, lines, 0).n_passed, 50);
});

test('C-squelch.4: 창이 끝난 뒤 같은 틀이 다시 오면 요약이 먼저 나오고 새 창으로 다시 5줄이 통과한다', () => {
    const state = createSquelchState();
    feed(state, Array.from({ length: 10 }, (unused, index) => line(index)), 0);
    const later = SQUELCH.WINDOW_MS + 1;
    const first = squelch(state, line(50), later);
    assert.match(first.prefix, /repeated 5 more times/);
    assert.equal(first.passed, line(50));
    assert.equal(feed(state, Array.from({ length: 4 }, (unused, index) => line(60 + index)), later).n_passed, 4);
});

test('C-squelch.5: 세션 100 MB를 넘으면 일반 출력은 안내 한 줄 뒤 통과하지 않는다', () => {
    const state = createSquelchState();
    state.n_bytes = SQUELCH.MAX_SESSION_BYTES - 10;
    const decision = squelch(state, 'a fairly long line that does not fit anymore\n', 5);
    assert.equal(decision.passed, '');
    assert.match(decision.prefix, /log limit reached: backend\.log exceeded 100 MB/);
    const after = feed(state, ['one\n', 'two\n'], 6);
    assert.equal(after.n_passed, 0);
    assert.equal(after.prefixes, '');
});

test('C-squelch.6: 100 MB 상한 뒤에도 오류 수준 줄(예외 스택·확장 호스트 사망)은 통과한다', () => {
    const state = createSquelchState();
    state.n_bytes = SQUELCH.MAX_SESSION_BYTES;
    squelch(state, 'plain\n', 1);
    const stack = '2026-10-14T00:00:00.000Z root ERROR Uncaught Exception: boom\n    at foo (a.js:1:2)\n    at bar (b.js:3:4)\n';
    assert.equal(squelch(state, stack, 2).passed, stack);
    const died = '2026-10-14T00:00:00.000Z plugin-ext:HostedPluginProcess INFO plugin host terminated unexpectedly\n';
    assert.equal(squelch(state, died, 3).passed, died);
});

test('C-squelch.7: 오류 수준 줄은 같은 틀이 반복돼도 억제하지 않고 오류 상한(10 MB)에서만 멈춘다', () => {
    const state = createSquelchState();
    const error = n => `2026-10-14T05:24:00.000Z root ERROR No reply handler ${n}\n`;
    assert.equal(feed(state, Array.from({ length: 100 }, (unused, index) => error(index)), 0).n_passed, 100);
    state.n_error_bytes = SQUELCH.MAX_ERROR_BYTES;
    const decision = squelch(state, error(1), 1);
    assert.equal(decision.passed, '');
    assert.match(decision.prefix, /log limit reached: error output exceeded 10 MB/);
    assert.equal(squelch(state, error(2), 2).prefix, '');
});

test('C-squelch.8: 여러 줄이 한 덩이로 와도 줄마다 판정한다(스택 줄은 앞 줄에 붙는다)', () => {
    assert.deepEqual(splitUnits('a\nb\n'), ['a\n', 'b\n']);
    assert.deepEqual(splitUnits('ERROR x\n    at f (1:2)\nnext\n'), ['ERROR x\n    at f (1:2)\n', 'next\n']);
    const state = createSquelchState();
    const chunk = Array.from({ length: 8 }, (unused, index) => line(index)).join('') + 'ERROR distinct failure\n';
    const decision = squelch(state, chunk, 0);
    assert.equal(decision.passed.split('\n').length - 1, 6, '반복 틀 5줄 + 오류 1줄');
    assert.match(decision.passed, /ERROR distinct failure/);
});

test('C-squelch.9: 추적하는 틀이 상한을 넘으면 가장 오래된 틀을 요약하고 버린다', () => {
    const state = createSquelchState();
    feed(state, Array.from({ length: 8 }, (unused, index) => line(index)), 0);
    for (let n = 0; n < SQUELCH.MAX_KEYS; n += 1) squelch(state, `unique-${'x'.repeat(n)}\n`, 1);
    assert.ok(state.windows.size <= SQUELCH.MAX_KEYS);
});

/** write 호출을 기록하는 가짜 스트림. */
function fakeStream() {
    const written = [];
    return { written, write(chunk, encoding, callback) { written.push(String(chunk)); (typeof encoding === 'function' ? encoding : callback)?.(); return true; } };
}

test('C-squelch.10: 스트림 래퍼는 버린 덩이에도 콜백을 부르고, 콜백이 encoding 자리에 와도 한 번만 부른다', () => {
    const stream = fakeStream();
    installLogSquelch([stream]);
    for (let n = 0; n < 5; n += 1) stream.write(line(n), 'utf8', () => {});
    let n_callbacks = 0;
    assert.equal(stream.write(line(6), () => { n_callbacks += 1; }), true);
    assert.equal(n_callbacks, 1, '버린 덩이도 콜백은 부른다');
    assert.equal(stream.written.length, 5);
    stream.write(line(7), () => { n_callbacks += 1; });
    stream.write(Buffer.from('ERROR from buffer\n'), () => { n_callbacks += 1; });
    assert.equal(n_callbacks, 3);
    assert.equal(stream.written.at(-1), 'ERROR from buffer\n');
});

test('C-squelch.11: 일부만 통과하는 덩이는 통과한 줄만 쓰고 콜백은 그 쓰기에 넘긴다', () => {
    const stream = fakeStream();
    installLogSquelch([stream]);
    for (let n = 0; n < 5; n += 1) stream.write(line(n));
    let n_callbacks = 0;
    stream.write(line(8) + 'ERROR keep me\n', () => { n_callbacks += 1; });
    assert.equal(stream.written.at(-1), 'ERROR keep me\n');
    assert.equal(n_callbacks, 1);
});
