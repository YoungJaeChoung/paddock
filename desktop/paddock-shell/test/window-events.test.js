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

const { EventEmitter } = require('node:events');
const Module = require('node:module');
const { isSecondaryContents } = require('../window-events');

test('B-window.6: 보조 창 주소만 보조 창으로 본다', () => {
    assert.equal(isSecondaryContents('file:///app/lib/frontend/secondary-window.html'), true);
    assert.equal(isSecondaryContents('file:///app/lib/frontend/index.html?port=1'), false);
    assert.equal(isSecondaryContents(''), false);
});

/** electron을 가짜로 끼워 `window-events`를 새로 읽는다. 대화상자 응답과 종료 이벤트를 시험이 정한다. */
function loadWithFakeElectron(dialogResponse) {
    const app = new EventEmitter();
    const dialog = { showMessageBox: async () => ({ response: dialogResponse }) };
    const loadModule = Module._load;
    Module._load = (request, ...rest) => (request === 'electron' ? { app, dialog } : loadModule(request, ...rest));
    delete require.cache[require.resolve('../window-events')];
    const events = require('../window-events');
    Module._load = loadModule;
    return { events, app };
}

/** 이벤트를 받는 가짜 창. `url`은 화면 주소, `calls`에 reload·destroy 호출이 쌓인다. */
function fakeWindow(url) {
    const window = new EventEmitter();
    const calls = [];
    window.webContents = new EventEmitter();
    window.webContents.getURL = () => url;
    window.webContents.reload = () => calls.push('reload');
    window.isDestroyed = () => false;
    window.destroy = () => calls.push('destroy');
    window.webContents.isCrashed = () => false;
    return { window, calls };
}

const gone = (window, reason) => window.webContents.emit('render-process-gone', {}, { reason, exitCode: 1 });
const settle = () => new Promise(resolve => setImmediate(resolve));

test('B-window.7: clean-exit·killed는 되살리지 않고 crashed는 옛 연결을 닫은 뒤 다시 불러온다', async () => {
    const { events } = loadWithFakeElectron(0);
    const { window, calls } = fakeWindow('file:///app/index.html');
    const notes = [];
    events.attachWindowEventLog(window, text => notes.push(text), () => '', async () => { calls.push('closeStale'); });
    gone(window, 'clean-exit');
    gone(window, 'killed');
    await settle();
    assert.deepEqual(calls, []);
    gone(window, 'crashed');
    await settle();
    assert.deepEqual(calls, ['closeStale', 'reload']);
});

test('B-window.8: 종료 중에는 되살리지 않는다', async () => {
    const { events, app } = loadWithFakeElectron(0);
    events.attachAppEventLog(() => {});
    app.emit('will-quit');
    const { window, calls } = fakeWindow('file:///app/index.html');
    events.attachWindowEventLog(window, () => {});
    gone(window, 'crashed');
    await settle();
    assert.deepEqual(calls, []);
});

test('B-window.9: 보조 창 화면이 죽으면 다시 불러오지 않고 그 창만 닫는다', async () => {
    const { events } = loadWithFakeElectron(0);
    const { window, calls } = fakeWindow('file:///app/lib/frontend/secondary-window.html');
    events.attachWindowEventLog(window, () => {}, () => '', async () => { calls.push('closeStale'); });
    gone(window, 'crashed');
    await settle();
    assert.deepEqual(calls, ['destroy']);
});

test('B-window.9b: 보조 창과 함께 죽은 메인 창은 사망 이벤트가 없어도 복구한다', async () => {
    const { events } = loadWithFakeElectron(0);
    const main = fakeWindow('file:///app/index.html');
    const secondary = fakeWindow('file:///app/lib/frontend/secondary-window.html');
    const closeStale = async () => { main.calls.push('closeStale'); };
    events.attachWindowEventLog(main.window, () => {}, () => '', closeStale);
    events.attachWindowEventLog(secondary.window, () => {}, () => '', async () => {});
    main.window.webContents.isCrashed = () => true;
    gone(secondary.window, 'crashed');
    await settle();
    assert.deepEqual(secondary.calls, ['destroy']);
    assert.deepEqual(main.calls, ['closeStale', 'reload']);
});

test('B-window.9c: 메인 창이 자기 사망을 받은 직후 보조 창 처리가 다시 불러오기를 겹쳐 하지 않는다', async () => {
    const { events } = loadWithFakeElectron(0);
    const main = fakeWindow('file:///app/index.html');
    const secondary = fakeWindow('file:///app/lib/frontend/secondary-window.html');
    events.attachWindowEventLog(main.window, () => {}, () => '', async () => { await settle(); });
    events.attachWindowEventLog(secondary.window, () => {}, () => '', async () => {});
    main.window.webContents.isCrashed = () => true;
    gone(main.window, 'crashed');
    gone(secondary.window, 'crashed');
    await settle();
    await settle();
    assert.equal(main.calls.filter(call => call === 'reload').length, 1);
});

test('B-window.10: 5분 안 네 번째 사망은 대화상자로 묻고 "Close window"를 고르면 옛 연결을 닫고 창을 닫는다', async () => {
    const { events } = loadWithFakeElectron(1);
    const { window, calls } = fakeWindow('file:///app/index.html');
    const notes = [];
    events.attachWindowEventLog(window, text => notes.push(text), () => '', async () => { calls.push('closeStale'); });
    for (let n = 0; n < 3; n += 1) {
        gone(window, 'crashed');
        await settle();
    }
    calls.length = 0;
    gone(window, 'crashed');
    await settle();
    assert.deepEqual(calls, ['closeStale', 'destroy']);
    assert.ok(notes.some(text => text.includes('asking the user')));
});
