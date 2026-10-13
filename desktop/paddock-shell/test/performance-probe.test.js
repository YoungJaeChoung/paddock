const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PROBE, evaluateSample, trimLog } = require('../performance-probe');

const QUIET = { max: 5e6, p99: 2e6 };
const SMALL = [{ type: 'Browser', workingSetKb: 200 * 1024 }, { type: 'Tab', workingSetKb: 300 * 1024 }];

test('PP01: a stall at the limit is logged with its length', () => {
    const result = evaluateSample({ max: 350e6, p99: 20e6 }, SMALL, 1000, 900);
    assert.equal(result.reason, 'stall');
    assert.equal(result.entry.stallMs, 350);
});

test('PP02: memory over the limit is logged and split by process type', () => {
    const result = evaluateSample(QUIET, [{ type: 'Tab', workingSetKb: 4096 * 1024 }, { type: 'GPU', workingSetKb: 512 * 1024 }], 1000, 900);
    assert.equal(result.reason, 'memory');
    assert.deepEqual(result.entry.memoryByType, { Tab: 4096, GPU: 512 });
});

test('PP03: a quiet sample is skipped until the heartbeat interval passes', () => {
    assert.equal(evaluateSample(QUIET, SMALL, 60000, 0).isLogged, false);
    assert.equal(evaluateSample(QUIET, SMALL, PROBE.HEARTBEAT_MS, 0).reason, 'heartbeat');
    assert.equal(evaluateSample(QUIET, SMALL, 1000, null).reason, 'heartbeat');
});

test('PP04: an oversize log keeps only whole lines from its newer half', () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'probe-'));
    const file = path.join(directory, 'performance.log');
    const line = `${JSON.stringify({ filler: 'x'.repeat(1000) })}\n`;
    fs.writeFileSync(file, line.repeat(1200));
    trimLog(file);
    const kept = fs.readFileSync(file, 'utf8');
    assert.ok(kept.length < PROBE.MAX_FILE_BYTES);
    assert.ok(kept.split('\n').filter(Boolean).every(text => JSON.parse(text).filler.length === 1000));
});
