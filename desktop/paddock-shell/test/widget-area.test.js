const test = require('node:test');
const assert = require('node:assert/strict');
const { widgetArea } = require('../widget-area');

test('WA01: a screen sent to a panel the shell does not have opens in the body', () => {
    for (const area of ['left', 'right', 'bottom']) {
        assert.equal(widgetArea({ area, isTerminal: false, isSidebarView: false, redirectsPanels: true }), 'main');
    }
});

test('WA02: a view the sidebar holds keeps the requested area so the sidebar can take it', () => {
    assert.equal(widgetArea({ area: 'left', isTerminal: false, isSidebarView: true, redirectsPanels: true }), 'left');
});

test('WA03: a terminal opens in the body except in a secondary window', () => {
    assert.equal(widgetArea({ area: 'bottom', isTerminal: true, isSidebarView: false, redirectsPanels: true }), 'main');
    assert.equal(widgetArea({ area: undefined, isTerminal: true, isSidebarView: false, redirectsPanels: true }), 'main');
    assert.equal(widgetArea({ area: 'secondaryWindow', isTerminal: true, isSidebarView: false, redirectsPanels: true }), 'secondaryWindow');
});

test('WA04: the body, secondary windows and no area stay as requested', () => {
    assert.equal(widgetArea({ area: 'main', isTerminal: false, isSidebarView: false, redirectsPanels: true }), 'main');
    assert.equal(widgetArea({ area: 'secondaryWindow', isTerminal: false, isSidebarView: false, redirectsPanels: true }), 'secondaryWindow');
    assert.equal(widgetArea({ area: undefined, isTerminal: false, isSidebarView: false, redirectsPanels: true }), undefined);
});

test('WA05: while the startup layout is built a non-terminal screen keeps the requested panel', () => {
    assert.equal(widgetArea({ area: 'bottom', isTerminal: false, isSidebarView: false, redirectsPanels: false }), 'bottom');
    assert.equal(widgetArea({ area: 'bottom', isTerminal: true, isSidebarView: false, redirectsPanels: false }), 'main');
});
