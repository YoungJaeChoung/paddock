const test = require('node:test');
const assert = require('node:assert/strict');
const { dirtyWidgets, reconnectAction, lostConnectionMessage } = require('../reconnect-decision');
const { closeDisconnectedFrontends } = require('../frontend-routes');

test('C-reconnect.1: 여러 목록의 미저장 위젯을 합치고 같은 위젯은 한 번만 센다', () => {
    const [a, b, c] = [{ id: 'a' }, { id: 'b', dirty: true }, { id: 'c', dirty: true }];
    const isDirty = widget => widget.dirty === true;
    assert.deepEqual(dirtyWidgets([[a, b], [b, c]], isDirty), [b, c]);
    assert.deepEqual(dirtyWidgets([[], []], isDirty), []);
});

test('C-reconnect.2: 미저장이 없으면 새로 고치고 있으면 알린다', () => {
    assert.equal(reconnectAction(0), 'reload');
    assert.equal(reconnectAction(1), 'notify');
    assert.equal(reconnectAction(3), 'notify');
});

test('C-reconnect.3: 알림 문장은 편집기 수에 맞게 단·복수를 쓴다', () => {
    assert.match(lostConnectionMessage(1), /1 editor has unsaved/);
    assert.match(lostConnectionMessage(2), /2 editors have unsaved/);
});

test('C-reconnect.4: 죽은 화면의 옛 연결은 기다리는 것을 모두 닫고 닫은 수를 돌려준다', () => {
    const closed = [];
    const service = { closeTimeouts: new Map([['1', 0], ['2', 0]]), closeConnection: (id, reason) => closed.push([id, reason]) };
    assert.equal(closeDisconnectedFrontends(service), 2);
    assert.deepEqual(closed, [['1', 'renderer-gone'], ['2', 'renderer-gone']]);
    assert.equal(closeDisconnectedFrontends({ closeTimeouts: new Map(), closeConnection: () => assert.fail('닫을 연결이 없다') }), 0);
});

const { mock } = require('node:test');
const Module = require('node:module');

/**
 * 화면(document)이 필요한 Theia 모듈을 같은 이름표(식별 객체)로 바꿔 끼워 `reconnect-guard`를 읽는다.
 * 가짜 컨테이너가 이 이름표로 가짜 서비스를 내준다.
 */
function loadGuard() {
    const tokens = {
        Saveable: { Saveable: { isDirty: widget => widget.dirty === true } },
        ApplicationShell: { ApplicationShell: { name: 'shell' } },
        WindowService: { WindowService: { name: 'window' } },
        WebSocketConnectionSource: { WebSocketConnectionSource: { name: 'source' } },
        MessageService: { MessageService: { name: 'message' } },
        EditorManager: { EditorManager: { name: 'editors' } },
    };
    const stubs = {
        '@theia/core/lib/browser/saveable': tokens.Saveable,
        '@theia/core/lib/browser/shell/application-shell': tokens.ApplicationShell,
        '@theia/core/lib/browser/window/window-service': tokens.WindowService,
        '@theia/core/lib/browser/messaging/ws-connection-source': tokens.WebSocketConnectionSource,
        '@theia/core/lib/common/message-service': tokens.MessageService,
        '@theia/editor/lib/browser/editor-manager': tokens.EditorManager,
    };
    const loadModule = Module._load;
    Module._load = (request, ...rest) => stubs[request] ?? loadModule(request, ...rest);
    delete require.cache[require.resolve('../reconnect-guard')];
    const { PaddockReconnectGuard } = require('../reconnect-guard');
    Module._load = loadModule;
    return { PaddockReconnectGuard, tokens };
}

/** 편집기·알림 응답을 시험이 정하는 가짜 컨테이너. */
function fakeContainer(tokens, { editors, answers }) {
    const log = { reloads: 0, warnings: [] };
    const services = new Map([
        [tokens.ApplicationShell.ApplicationShell, { widgets: [], secondaryWindowHandler: undefined }],
        [tokens.EditorManager.EditorManager, { get all() { return editors(); } }],
        [tokens.WindowService.WindowService, { reload: () => { log.reloads += 1; } }],
        [tokens.MessageService.MessageService, { warn: async (text) => { log.warnings.push(text); return answers.shift(); } }],
    ]);
    return { container: { get: key => services.get(key) }, log };
}

test('C-reconnect.5: 미저장 편집기가 없으면 알림 없이 바로 새로 고친다', async () => {
    const { PaddockReconnectGuard, tokens } = loadGuard();
    const { container, log } = fakeContainer(tokens, { editors: () => [{ dirty: false }], answers: [] });
    await new PaddockReconnectGuard(container).handleReconnectFailed();
    assert.deepEqual(log, { reloads: 1, warnings: [] });
});

test('C-reconnect.6: 알림을 닫아도(응답 없음) 30초 뒤 다시 띄우고, "Reload window"를 누르면 새로 고친다', async (context) => {
    context.mock.timers.enable({ apis: ['setTimeout'] });
    const { PaddockReconnectGuard, tokens } = loadGuard();
    const { container, log } = fakeContainer(tokens, { editors: () => [{ dirty: true }], answers: [undefined, 'Reload window'] });
    const done = new PaddockReconnectGuard(container).handleReconnectFailed();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(log.warnings.length, 1);
    assert.equal(log.reloads, 0);
    context.mock.timers.tick(30 * 1000);
    await done;
    assert.equal(log.warnings.length, 2);
    assert.equal(log.reloads, 1);
});

test('C-reconnect.7: 다시 띄울 때 미저장 편집기가 사라졌으면 바로 새로 고친다', async (context) => {
    context.mock.timers.enable({ apis: ['setTimeout'] });
    const { PaddockReconnectGuard, tokens } = loadGuard();
    let dirty = true;
    const { container, log } = fakeContainer(tokens, { editors: () => [{ dirty }], answers: [undefined] });
    const done = new PaddockReconnectGuard(container).handleReconnectFailed();
    await new Promise(resolve => setImmediate(resolve));
    dirty = false;
    context.mock.timers.tick(30 * 1000);
    await done;
    assert.equal(log.warnings.length, 1);
    assert.equal(log.reloads, 1);
});
