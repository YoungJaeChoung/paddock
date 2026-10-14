const test = require('node:test');
const assert = require('node:assert/strict');
const { TrailingDebounce } = require('../trailing-debounce');

/** 가짜 시계. `tick(ms)`로 시간을 보내면 시각이 된 예약을 실행한다. */
function fakeClock() {
    const clock = {
        time: 0,
        jobs: new Map(),
        n_next: 1,
        setTimeout: (callback, delay) => { const id = clock.n_next; clock.n_next += 1; clock.jobs.set(id, { at: clock.time + delay, callback }); return id; },
        clearTimeout: id => { clock.jobs.delete(id); },
        now: () => clock.time,
        tick(ms) {
            clock.time += ms;
            for (const [id, job] of [...clock.jobs]) {
                if (job.at <= clock.time) { clock.jobs.delete(id); job.callback(); }
            }
        },
    };
    return clock;
}

test('신호가 멎은 뒤 quiet이 지나면 한 번 실행한다', () => {
    const clock = fakeClock();
    const runs = [];
    const debounce = new TrailingDebounce({ quietMs: 2000, maxWaitMs: 10000, run: () => runs.push(clock.time), timers: clock, now: clock.now });
    debounce.touch();
    clock.tick(1999);
    assert.deepEqual(runs, []);
    clock.tick(1);
    assert.deepEqual(runs, [2000]);
    clock.tick(20000);
    assert.deepEqual(runs, [2000]);
});

test('신호가 계속 와도 첫 신호로부터 maxWait 안에 실행하고 다음 신호는 새로 센다', () => {
    const clock = fakeClock();
    const runs = [];
    const debounce = new TrailingDebounce({ quietMs: 2000, maxWaitMs: 10000, run: () => runs.push(clock.time), timers: clock, now: clock.now });
    for (let i = 0; i < 12; i += 1) { debounce.touch(); clock.tick(1000); }
    assert.equal(runs[0], 10000);
    assert.equal(runs.length, 1);
    clock.tick(2000);
    assert.equal(runs.length, 2);
});

test('cancel하면 기다리던 실행이 사라진다', () => {
    const clock = fakeClock();
    const runs = [];
    const debounce = new TrailingDebounce({ quietMs: 2000, maxWaitMs: 10000, run: () => runs.push(1), timers: clock, now: clock.now });
    debounce.touch();
    debounce.cancel();
    debounce.cancel();
    clock.tick(30000);
    assert.deepEqual(runs, []);
});
