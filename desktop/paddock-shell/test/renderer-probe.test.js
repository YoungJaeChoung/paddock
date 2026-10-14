const test = require('node:test');
const assert = require('node:assert/strict');
const { RENDERER_PROBE, evaluateRendererSample } = require('../renderer-probe');

test('RP01: a long task at the limit is logged as a stall', () => {
    const result = evaluateRendererSample([60, 250], [30], 1000, 900);
    assert.equal(result.reason, 'stall');
    assert.equal(result.entry.longestTaskMs, 250);
    assert.equal(result.entry.n_longTasks, 2);
    assert.equal(result.entry.longTaskTotalMs, 310);
});

test('RP02: a slow key without a long task is logged as input delay', () => {
    const result = evaluateRendererSample([80], [20, 60, 120], 1000, 900);
    assert.equal(result.reason, 'input');
    assert.equal(result.entry.keyDelayMaxMs, 120);
    assert.equal(result.entry.n_slowKeys, 2);
});

test('RP03: a quiet sample is skipped until the heartbeat interval passes', () => {
    assert.equal(evaluateRendererSample([], [], 60000, 0).isLogged, false);
    assert.equal(evaluateRendererSample([], [], RENDERER_PROBE.HEARTBEAT_MS, 0).reason, 'heartbeat');
    assert.deepEqual(evaluateRendererSample([], [], 1000, null).entry, {
        at: new Date(1000).toISOString(), source: 'renderer', reason: 'heartbeat',
        longestTaskMs: 0, n_longTasks: 0, longTaskTotalMs: 0, keyDelayMaxMs: 0, n_slowKeys: 0,
    });
});

test('RP04: key and composition events count as input, other events do not', () => {
    for (const name of ['keydown', 'keypress', 'keyup', 'compositionupdate', 'beforeinput', 'input']) assert.ok(RENDERER_PROBE.KEY_EVENT.test(name), name);
    for (const name of ['click', 'pointerdown', 'inputmode']) assert.ok(!RENDERER_PROBE.KEY_EVENT.test(name), name);
});
