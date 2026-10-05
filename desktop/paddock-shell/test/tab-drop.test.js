const test = require('node:test');
const assert = require('node:assert/strict');
const { tabDropTarget, tabDockRef } = require('../tab-drop');

test('WD01: pane edges split in the intended direction and center merges tabs', () => {
    const rect = { left: 200, top: 50, width: 800, height: 400 };
    for (const [x, y, mode] of [[210, 250, 'split-left'], [990, 250, 'split-right'], [600, 60, 'split-top'], [600, 440, 'split-bottom'], [600, 250, 'tab-after']]) {
        const target = tabDropTarget(rect, x, y);
        assert.equal(target.mode, mode);
        assert(target.rect.left >= rect.left && target.rect.top >= rect.top);
        assert(target.rect.left + target.rect.width <= rect.left + rect.width);
        assert(target.rect.top + target.rect.height <= rect.top + rect.height);
    }
});

test('WD02: outside the pane and zero-size panes have no drop destination', () => {
    const rect = { left: 200, top: 50, width: 800, height: 400 };
    for (const [x, y] of [[199, 200], [1000, 200], [500, 49], [500, 450]]) assert.equal(tabDropTarget(rect, x, y), null);
    assert.equal(tabDropTarget({ ...rect, width: 0 }, 200, 100), null);
});

test('WD03: dropping on the center of the pane that already holds the tab changes nothing', () => {
    assert.equal(tabDockRef('tab-after', ['a', 'b', 'c'], 'b', 'a'), null);
    assert.equal(tabDockRef('tab-after', ['a', 'b'], 'a', 'a'), null);
    assert.equal(tabDockRef('tab-after', ['c'], 'c', 'a'), 'c');
});

test('WD04: an edge split from the tab own pane keeps another tab as the reference', () => {
    assert.equal(tabDockRef('split-right', ['a', 'b'], 'a', 'a'), 'b');
    assert.equal(tabDockRef('split-bottom', ['a', 'b'], 'b', 'a'), 'b');
    assert.equal(tabDockRef('split-left', ['a'], 'a', 'a'), null);
    assert.equal(tabDockRef('split-top', ['c'], 'c', 'a'), 'c');
});
