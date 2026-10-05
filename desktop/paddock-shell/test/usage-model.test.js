const test = require('node:test');
const assert = require('node:assert/strict');
const usage = require('../usage-model');

// Codex 세션 기록 한 줄(실제 형식에서 필요한 부분만 남김).
const CODEX_LINE = JSON.stringify({ timestamp: '2026-09-24T10:31:36Z', type: 'event_msg', payload: { type: 'token_count', rate_limits: { limit_id: 'codex', primary: { used_percent: 41.0, window_minutes: 10080, resets_at: 1790846316 }, secondary: { used_percent: 12.4, window_minutes: 300, resets_at: 1790800000 }, plan_type: 'pro' } } });

test('C-usage-U1.1: Codex 기록에서 마지막 사용량 한도를 창별로 읽는다', () => {
    const text = ['{"type":"other"}', CODEX_LINE.replace('41.0', '30.0'), 'not json', CODEX_LINE, ''].join('\n');
    assert.deepEqual(usage.codexWindows(text), [
        { label: '5h', used: 12, resetsAt: 1790800000 },
        { label: 'week', used: 41, resetsAt: 1790846316 },
    ]);
});

test('C-usage-U1.2: 한도 기록이 없거나 깨지면 빈 목록이다', () => {
    assert.deepEqual(usage.codexWindows('{"type":"session_meta"}\n{broken'), []);
    assert.deepEqual(usage.codexWindows(''), []);
});

test('C-usage-U2.1: Claude 상태 줄 입력에서 5시간·주간 창을 읽는다', () => {
    const input = { rate_limits: { five_hour: { used_percentage: 23.5, resets_at: 1738425600 }, seven_day: { used_percentage: 41.2, resets_at: 1738857600 } } };
    assert.deepEqual(usage.claudeWindows(input), [
        { label: '5h', used: 24, resetsAt: 1738425600 },
        { label: 'week', used: 41, resetsAt: 1738857600 },
    ]);
    assert.deepEqual(usage.claudeWindows({ model: {} }), []);
});

test('C-usage-U3.1: 초기화 시각이 지난 창은 숨기지 않고 0%로 보이며, 말풍선은 다음 사용 때 새 창이 시작된다고 쓴다', () => {
    const windows = [{ label: '5h', used: 90, resetsAt: 100 }, { label: 'week', used: 10, resetsAt: 300 }];
    const current = usage.currentWindows(windows, 200);
    assert.deepEqual(current, [{ label: '5h', used: 0, resetsAt: null }, { label: 'week', used: 10, resetsAt: 300 }]);
    assert.equal(usage.describe(current[0], 200), '0% used · 100% left · reset — a new window starts with your next request');
    assert.equal(usage.describe({ label: '5h', used: 5 }, 200), '5% used · 95% left');
});

test('C-usage-U3.2: 말풍선은 사용·남은 양과 초기화까지 남은 시간을 쓴다', () => {
    assert.equal(usage.describe({ label: '5h', used: 23, resetsAt: 10000 + 2 * 3600 + 600 }, 10000), '23% used · 77% left · resets in 2h 10m');
    assert.equal(usage.describe({ label: 'week', used: 41, resetsAt: 10000 + 3 * 86400 + 3600 }, 10000), '41% used · 59% left · resets in 3d 1h');
});

test('C-usage-U3.3: 창 길이를 5h·week처럼 짧게 적고, 말풍선에서는 풀어 쓴다', () => {
    assert.equal(usage.windowLabel(300), '5h');
    assert.equal(usage.windowLabel(10080), 'week');
    assert.equal(usage.windowLabel(1440), '1d');
    assert.equal(usage.windowLabel(45), '45m');
    assert.equal(usage.windowName('5h'), '5-hour');
    assert.equal(usage.windowName('week'), 'weekly');
    assert.equal(usage.windowName('45m'), '45m');
});

test('C-usage-U4.1: 상태 줄 등록은 기존 명령을 이어 부르도록 기억하고, 해제하면 되돌린다', () => {
    const settings = { theme: 'dark', statusLine: { type: 'command', command: 'my-line' } };
    const installed = usage.installStatusLine(settings, 'paddock-line');
    assert.deepEqual(installed.settings.statusLine, { type: 'command', command: 'paddock-line' });
    assert.deepEqual(installed.previous, { type: 'command', command: 'my-line' });
    assert.equal(installed.settings.theme, 'dark');
    const again = usage.installStatusLine(installed.settings, 'paddock-line');
    assert.deepEqual(again.previous, null, '이미 등록돼 있으면 자기 자신을 이전 명령으로 기억하지 않는다');
    assert.deepEqual(usage.restoreStatusLine(installed.settings, installed.previous).statusLine, settings.statusLine);
    assert.equal('statusLine' in usage.restoreStatusLine({ statusLine: { type: 'command', command: 'paddock-line' } }, null), false);
});

test('C-usage-U4.2: 설정 폴더가 다른 Paddock의 상태 줄은 이어 부르지 않고 그 파일 경로를 알려 준다', () => {
    const other = '"/usr/bin/node" "/tmp/qa/usage/claude-statusline.cjs" "/tmp/qa/usage/claude.json" "/tmp/qa/usage/previous-statusline.json"';
    assert.deepEqual(usage.paddockStatusLineFiles(other), { script: '/tmp/qa/usage/claude-statusline.cjs', usageFile: '/tmp/qa/usage/claude.json', previousFile: '/tmp/qa/usage/previous-statusline.json' });
    assert.equal(usage.paddockStatusLineFiles('npx ccstatusline'), null);
    const installed = usage.installStatusLine({ statusLine: { type: 'command', command: other } }, 'paddock-line');
    assert.equal(installed.previous, null);
    assert.equal(installed.replaced.previousFile, '/tmp/qa/usage/previous-statusline.json');
    assert.equal(usage.installStatusLine({ statusLine: { type: 'command', command: 'my-line' } }, 'paddock-line').replaced, null);
});

test('C-usage-U5.1: 상태 줄 스크립트는 한도만 저장하고, 원래 상태 줄이 있으면 그 출력을 보여 준다', () => {
    const fs = require('node:fs');
    const os = require('node:os');
    const path = require('node:path');
    const { spawnSync } = require('node:child_process');
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-usage-'));
    const usagePath = path.join(directory, 'claude.json');
    const previousPath = path.join(directory, 'previous-statusline.json');
    const script = path.join(__dirname, '..', 'claude-statusline.cjs');
    const input = JSON.stringify({ model: { display_name: 'Opus' }, context_window: { used_percentage: 8.4 }, rate_limits: { five_hour: { used_percentage: 23.5, resets_at: 1 } }, session_id: 'secret-session' });
    const plain = spawnSync(process.execPath, [script, usagePath, previousPath], { input, encoding: 'utf8' });
    assert.equal(plain.stdout, 'Opus · 8% context');
    const saved = JSON.parse(fs.readFileSync(usagePath, 'utf8'));
    assert.deepEqual(Object.keys(saved).sort(), ['rate_limits', 'updated_at']);
    fs.writeFileSync(previousPath, JSON.stringify({ type: 'command', command: 'cat >/dev/null; echo mine' }));
    const chained = spawnSync(process.execPath, [script, usagePath, previousPath], { input, encoding: 'utf8' });
    assert.equal(chained.stdout.trim(), 'mine');
});

test('C-usage-U1.3: Codex 기록은 기본 묶음의 마지막 한도를 쓰고, 다른 묶음이나 창이 없는 줄이 뒤에 있어도 건너뛴다', () => {
    const premium = JSON.stringify({ payload: { type: 'token_count', rate_limits: { limit_id: 'premium', primary: null, secondary: null, plan_type: 'plus' } } });
    const otherBucket = JSON.stringify({ payload: { type: 'token_count', rate_limits: { limit_id: 'other', primary: { used_percent: 99, window_minutes: 300, resets_at: 1 } } } });
    const empty = JSON.stringify({ payload: { type: 'token_count', rate_limits: { limit_id: 'codex', primary: null, secondary: null } } });
    const text = [CODEX_LINE, otherBucket, empty, premium, ''].join('\n');
    assert.deepEqual(usage.codexWindows(text).map(window => [window.label, window.used]), [['5h', 12], ['week', 41]]);
    const legacy = CODEX_LINE.replace('"limit_id":"codex",', '');
    assert.equal(usage.codexWindows(legacy).length, 2, 'records written before limit_id existed are the default bucket');
});

test('C-usage-U3.4: 초기화 전 창이 하나라도 있어야 지금 의미 있는 기록이고, 경과 시간은 가장 큰 단위 하나로 줄인다', () => {
    assert.equal(usage.hasCurrentWindow([{ label: 'week', used: 29, resetsAt: 300 }], 200), true);
    assert.equal(usage.hasCurrentWindow([{ label: 'week', used: 29, resetsAt: 100 }], 200), false);
    assert.equal(usage.hasCurrentWindow([{ label: 'week', used: 29 }], 200), false, 'an unknown reset time does not prove the record is current');
    assert.equal(usage.hasCurrentWindow([], 200), false);
    assert.deepEqual([59, 600, 67680, 259200].map(usage.shortAge), ['0m', '10m', '18h', '3d']);
});
