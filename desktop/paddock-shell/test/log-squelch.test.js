const test = require('node:test');
const assert = require('node:assert/strict');
const { SQUELCH, createSquelchState, squelchKey, flushExpired, squelch } = require('../log-squelch');

const line = (n, text = 'root ERROR No reply handler for') => `2026-10-14T05:24:${String(n % 60).padStart(2, '0')}.123Z ${text} ${n}\n`;

/** 줄들을 차례로 넣어 통과한 줄 수를 센다. */
function feed(state, lines, now) {
    let n_passed = 0;
    let prefixes = '';
    for (const text of lines) {
        const decision = squelch(state, text, now);
        prefixes += decision.prefix;
        if (decision.isPassed) n_passed += 1;
    }
    return { n_passed, prefixes };
}

test('C-squelch.1: 시각·번호만 다른 같은 틀은 같은 열쇠다', () => {
    assert.equal(squelchKey(line(17)), squelchKey(line(99)));
    assert.notEqual(squelchKey(line(1)), squelchKey(line(1, 'root WARN other message')));
    assert.equal(squelchKey('[4b0ba821-b495-4f57-b8ea-5411ad21f9b7] Starting\n'), '[ID] Starting');
});

test('C-squelch.2: 같은 틀 100줄은 5줄만 통과하고 창이 끝나면 버린 95줄을 요약 한 줄로 남긴다', () => {
    const state = createSquelchState();
    const lines = Array.from({ length: 100 }, (unused, index) => line(index));
    const { n_passed, prefixes } = feed(state, lines, 1000);
    assert.equal(n_passed, 5);
    assert.equal(prefixes, '');
    const summary = flushExpired(state, 1000 + SQUELCH.WINDOW_MS);
    assert.match(summary, /repeated 95 more times in 10 s: 2026-10-14T05:24:39\.123Z root ERROR No reply handler for 99/);
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
    assert.equal(first.isPassed, true);
    assert.equal(feed(state, Array.from({ length: 4 }, (unused, index) => line(60 + index)), later).n_passed, 4);
});

test('C-squelch.5: 세션 100 MB를 넘으면 안내 한 줄 뒤에는 아무것도 통과하지 않는다', () => {
    const state = createSquelchState();
    state.n_bytes = SQUELCH.MAX_SESSION_BYTES - 10;
    const decision = squelch(state, 'a fairly long line that does not fit anymore\n', 5);
    assert.equal(decision.isPassed, false);
    assert.match(decision.prefix, /backend\.log exceeded 100 MB; suppressing further output until restart/);
    const after = feed(state, ['one\n', 'two\n'], 6);
    assert.equal(after.n_passed, 0);
    assert.equal(after.prefixes, '');
});

test('C-squelch.6: 추적하는 틀이 상한을 넘으면 가장 오래된 틀을 요약하고 버린다', () => {
    const state = createSquelchState();
    feed(state, Array.from({ length: 8 }, (unused, index) => line(index)), 0);
    for (let n = 0; n < SQUELCH.MAX_KEYS; n += 1) squelch(state, `unique-${'x'.repeat(n)}\n`, 1);
    assert.ok(state.windows.size <= SQUELCH.MAX_KEYS);
});
