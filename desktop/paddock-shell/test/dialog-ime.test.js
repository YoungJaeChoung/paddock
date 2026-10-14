const test = require('node:test');
const assert = require('node:assert/strict');
const { holdEnterWhileComposing } = require('../dialog-ime');

// 브라우저 없이 시험하려고 이벤트 대상·이벤트를 최소한으로 흉내 낸다.
class FakeTarget {
    constructor() { this.listeners = {}; }
    addEventListener(type, listener) { (this.listeners[type] ??= []).push(listener); }
    removeEventListener(type, listener) { this.listeners[type] = (this.listeners[type] ?? []).filter(item => item !== listener); }
    fire(type, event) { for (const listener of this.listeners[type] ?? []) listener(event); }
}

function keyEvent(element, init) {
    return {
        target: element, code: 'Enter', key: 'Enter', keyCode: 13, isComposing: false, ...init,
        isStopped: false, stopImmediatePropagation() { this.isStopped = true; }, preventDefault() {},
    };
}

function setup() {
    const dispatched = [];
    const input = { isConnected: true, closest: selector => (selector === '.dialogOverlay' ? {} : null), dispatchEvent: event => dispatched.push(event) };
    const outside = { isConnected: true, closest: () => null, dispatchEvent: () => {} };
    const timers = [];
    const target = new FakeTarget();
    globalThis.KeyboardEvent ??= class { constructor(type, init) { Object.assign(this, { type }, init); } };
    holdEnterWhileComposing(target, callback => timers.push(callback));
    return { target, input, outside, dispatched, runTimers: () => timers.splice(0).forEach(callback => callback()) };
}

test('DI01: Enter pressed while composing in a dialog is held and re-sent after composition ends', () => {
    const { target, input, dispatched, runTimers } = setup();
    const composing = keyEvent(input, { isComposing: true, keyCode: 229 });
    target.fire('keydown', composing);
    assert.equal(composing.isStopped, true);
    target.fire('compositionend', { target: input });
    runTimers();
    assert.equal(dispatched.length, 1);
    assert.equal(dispatched[0].code, 'Enter');
});

test('DI02: a real Enter right after composition ends confirms by itself, so nothing is re-sent', () => {
    const { target, input, dispatched, runTimers } = setup();
    target.fire('keydown', keyEvent(input, { isComposing: true, keyCode: 229 }));
    target.fire('compositionend', { target: input });
    const real = keyEvent(input, {});
    target.fire('keydown', real);
    runTimers();
    assert.equal(real.isStopped, false);
    assert.equal(dispatched.length, 0);
});

test('DI03: Enter outside dialogs and plain Enter in dialogs pass through untouched', () => {
    const { target, input, outside, dispatched, runTimers } = setup();
    const terminalEnter = keyEvent(outside, { isComposing: true, keyCode: 229 });
    const dialogEnter = keyEvent(input, {});
    target.fire('keydown', terminalEnter);
    target.fire('keydown', dialogEnter);
    runTimers();
    assert.equal(terminalEnter.isStopped, false);
    assert.equal(dialogEnter.isStopped, false);
    assert.equal(dispatched.length, 0);
});
