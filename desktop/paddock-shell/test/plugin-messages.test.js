const test = require('node:test');
const assert = require('node:assert/strict');
const { coalesceMessage } = require('../message-coalesce');
const { loggedLines } = require('../plugin-output-log');
const { BackgroundGitRefresh } = require('../background-git-refresh');

/** 호출 횟수를 세고, 직접 닫을 수 있는 열기 함수. */
function fakeOpen() {
    const calls = [];
    const open = () => new Promise((resolve) => { calls.push(resolve); });
    return { open, calls };
}

test('A-messages.1: 같은 문구가 열려 있는 동안 다시 와도 한 번만 열고 같은 결과를 돌려준다', async () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    const first = coalesceMessage(pending, 'Git: fatal', open);
    const second = coalesceMessage(pending, 'Git: fatal', open);
    assert.equal(calls.length, 1);
    assert.equal(first, second);
    calls[0]('Close');
    assert.equal(await first, 'Close');
});

test('A-messages.2: 다른 문구는 각각 연다', () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    coalesceMessage(pending, 'a', open);
    coalesceMessage(pending, 'b', open);
    assert.equal(calls.length, 2);
});

test('A-messages.3: 닫힌 뒤 같은 문구가 다시 오면 다시 연다', async () => {
    const pending = new Map();
    const { open, calls } = fakeOpen();
    const first = coalesceMessage(pending, 'a', open);
    calls[0](undefined);
    await first;
    assert.equal(pending.size, 0);
    coalesceMessage(pending, 'a', open);
    assert.equal(calls.length, 2);
});

test('A-messages.4: Git 채널의 오류·경고 줄만 로그로 남긴다', () => {
    const text = [
        '2026-10-14T00:00:00.000Z [Info] > git status [3ms]',
        '2026-10-14T00:00:01.000Z [Error] fatal: mmap: could not determine filesize',
        '2026-10-14T00:00:02.000Z [Warning] git fetch failed',
        '',
    ].join('\n');
    assert.deepEqual(loggedLines('Git', text), [
        '2026-10-14T00:00:01.000Z [Error] fatal: mmap: could not determine filesize',
        '2026-10-14T00:00:02.000Z [Warning] git fetch failed',
    ]);
    assert.deepEqual(loggedLines('Tasks', text), []);
    assert.deepEqual(loggedLines('Git', undefined), []);
});

test('A-messages.5: 문구가 같아도 버튼 집합이 다르면 합치지 않는다', () => {
    const { messageKey } = require('../message-coalesce');
    assert.equal(messageKey('a', ['Open', 'Close']), messageKey('a', ['Open', 'Close']));
    assert.notEqual(messageKey('a', ['Open']), messageKey('a', ['Open', 'Close']));
    const pending = new Map();
    const { open, calls } = fakeOpen();
    coalesceMessage(pending, messageKey('a', ['Open']), open);
    coalesceMessage(pending, messageKey('a', ['Retry']), open);
    assert.equal(calls.length, 2);
});

/**
 * 화면(document)이 필요한 Theia 모달 구현 대신, 같은 응답 번호 변환을 하는 화면 없는 기본 구현을 부모로 끼워 읽는다.
 * 모달 여부로 갈리는 `doShowMessage`만 Paddock 코드가 바꾸고, 응답 번호 변환(`$showMessage`)은 두 구현이 같다.
 */
const Module = require('node:module');
const loadModule = Module._load;
Module._load = (request, ...rest) => (request === '@theia/plugin-ext/lib/main/browser/message-registry-main'
    ? { MessageRegistryMainImpl: loadModule('@theia/plugin-ext/lib/main/common/basic-message-registry-main', ...rest).BasicMessageRegistryMainImpl }
    : loadModule(request, ...rest));

test('A-messages.6: 오류 모달은 알림으로 열고 누른 버튼을 응답 번호로 돌려주며, 닫으면 onCloseActionHandle을 돌려준다', async () => {
    const { MessageService } = require('@theia/core/lib/common/message-service');
    const { ILogger } = require('@theia/core/lib/common/logger');
    const { MainMessageType } = require('@theia/plugin-ext/lib/common/plugin-api-rpc');
    const { PaddockMessageRegistryMain } = require('../plugin-messages');
    const shown = [];
    let answer = 'Show Command Output';
    const services = new Map([
        [MessageService, { error: (text, ...titles) => { shown.push([text, titles]); return Promise.resolve(answer); } }],
        [ILogger, { warn: () => {} }],
        [BackgroundGitRefresh, new BackgroundGitRefresh()],
    ]);
    const registry = new PaddockMessageRegistryMain({ get: key => services.get(key) });
    const actions = [{ title: 'Open Git Log' }, { title: 'Show Command Output' }];
    assert.equal(await registry.$showMessage(MainMessageType.Error, 'Git: fatal', { modal: true }, actions), 1);
    assert.deepEqual(shown, [['Git: fatal', ['Open Git Log', 'Show Command Output']]]);
    answer = undefined;
    assert.equal(await registry.$showMessage(MainMessageType.Error, 'Git: fatal 2', { modal: true, onCloseActionHandle: 7 }, actions), 7);
});

test('A-messages.7: 백그라운드 다시 읽기 중 같은 실패가 이어지면 첫 알림만 띄우고, 성공하면 묶음을 닫아 반복 횟수를 돌려준다', async () => {
    const refresh = new BackgroundGitRefresh();
    assert.equal(refresh.classify('Git: x'), 'foreground');
    const kinds = [];
    const failing = () => { kinds.push(refresh.classify('Git: x')); return Promise.resolve(); };
    assert.equal(await refresh.run(failing), null);
    assert.equal(await refresh.run(failing), null);
    assert.equal(await refresh.run(failing), null);
    assert.deepEqual(kinds, ['first', 'repeat', 'repeat']);
    assert.deepEqual(await refresh.run(() => Promise.resolve()), { failure: 'Git: x', n_repeats: 2 });
    assert.equal(await refresh.run(() => Promise.resolve()), null);
    await refresh.run(failing);
    assert.equal(kinds.at(-1), 'first');
});

test('A-messages.8: 다른 문구는 새 실패로 띄우고, 다시 읽기가 거부해도 알림이 없었으면 성공으로 본다', async () => {
    const refresh = new BackgroundGitRefresh();
    await refresh.run(() => { refresh.classify('Git: x'); });
    let kind;
    await refresh.run(() => { kind = refresh.classify('Git: y'); });
    assert.equal(kind, 'first');
    assert.deepEqual(await refresh.run(() => Promise.reject(new Error('cancelled'))), { failure: 'Git: y', n_repeats: 0 });
});

test('A-messages.9: 등록기는 백그라운드 반복 실패를 띄우지도 로그에 남기지도 않고 닫힌 것으로 답한다', async () => {
    const { MessageService } = require('@theia/core/lib/common/message-service');
    const { ILogger } = require('@theia/core/lib/common/logger');
    const { MainMessageType } = require('@theia/plugin-ext/lib/common/plugin-api-rpc');
    const { PaddockMessageRegistryMain } = require('../plugin-messages');
    const shown = [];
    const logged = [];
    const refresh = new BackgroundGitRefresh();
    const services = new Map([
        [MessageService, { error: (text) => { shown.push(text); return Promise.resolve(undefined); } }],
        [ILogger, { warn: (line) => { logged.push(line); } }],
        [BackgroundGitRefresh, refresh],
    ]);
    const registry = new PaddockMessageRegistryMain({ get: key => services.get(key) });
    const options = { modal: true, onCloseActionHandle: 3 };
    const answers = [];
    const failing = () => registry.$showMessage(MainMessageType.Error, 'Git: Failed to execute git', options, []).then(answer => { answers.push(answer); });
    await refresh.run(failing);
    await refresh.run(failing);
    await refresh.run(failing);
    assert.deepEqual(shown, ['Git: Failed to execute git']);
    assert.equal(logged.length, 1);
    assert.match(logged[0], /background Git refresh/);
    assert.deepEqual(answers, [3, 3, 3]);
    // 백그라운드 다시 읽기 밖에서 온 같은 문구는 그대로 띄운다.
    await registry.$showMessage(MainMessageType.Error, 'Git: Failed to execute git', options, []);
    assert.equal(shown.length, 2);
});
