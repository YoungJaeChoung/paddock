const test = require('node:test');
const assert = require('node:assert/strict');
const { RECOVERY, describeGone, shouldReload } = require('../window-events');

test('B-window.1: 화면 사망은 이유와 종료 코드를 한 줄로 쓴다', () => {
    assert.equal(describeGone({ reason: 'crashed', exitCode: -1073741819 }), 'renderer gone: crashed (exitCode=-1073741819)');
});

test('B-window.2: 하위 프로세스 사망은 종류와 이름을 앞에 쓴다', () => {
    assert.equal(describeGone({ type: 'GPU', reason: 'launch-failed', exitCode: 1 }), 'GPU process gone: launch-failed (exitCode=1)');
    assert.equal(describeGone({ type: 'Utility', name: 'network', reason: 'killed', exitCode: 9 }), 'Utility process (network) gone: killed (exitCode=9)');
});

test('B-window.3: 5분 안 이전 사건이 3회 미만이면 다시 불러오고 이번 사건을 기록에 더한다', () => {
    const now = 1_000_000;
    assert.deepEqual(shouldReload([], now), { isReload: true, history: [now] });
    assert.equal(shouldReload([now - 1000, now - 2000], now).isReload, true);
});

test('B-window.4: 5분 안 이전 사건이 3회면 묻는다(네 번째 사망)', () => {
    const now = 1_000_000;
    const decision = shouldReload([now - 3000, now - 2000, now - 1000], now);
    assert.equal(decision.isReload, false);
    assert.equal(decision.history.length, 4);
});

test('B-window.5: 윈도 밖의 오래된 사건은 세지 않고 기록에서 버린다', () => {
    const now = 10 * RECOVERY.WINDOW_MS;
    const old = [now - RECOVERY.WINDOW_MS - 1, now - RECOVERY.WINDOW_MS - 2, now - RECOVERY.WINDOW_MS - 3];
    assert.deepEqual(shouldReload(old, now), { isReload: true, history: [now] });
});
