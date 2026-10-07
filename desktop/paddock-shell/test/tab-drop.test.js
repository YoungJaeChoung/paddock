const test = require('node:test');
const assert = require('node:assert/strict');
const { tabDropTarget, tabDockRef, tabRowInsert, isSameTabPlace } = require('../tab-drop');

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

test('WD05: dropping on a tab row inserts before the left half and after the right half of the pointed tab', () => {
    const tabs = [{ id: 'a', left: 0, right: 100 }, { id: 'b', left: 100, right: 200 }];
    assert.deepEqual(tabRowInsert(tabs, 30), { mode: 'tab-before', refId: 'a', edge: 0 });
    assert.deepEqual(tabRowInsert(tabs, 70), { mode: 'tab-after', refId: 'a', edge: 100 });
    assert.deepEqual(tabRowInsert(tabs, 160), { mode: 'tab-after', refId: 'b', edge: 200 });
    // 줄 끝의 빈 곳은 마지막 탭 뒤다.
    assert.deepEqual(tabRowInsert(tabs, 260), { mode: 'tab-after', refId: 'b', edge: 200 });
    assert.equal(tabRowInsert([], 50), null);
});

test('WD06: inserting where the tab already sits in its own pane changes nothing, moving to another pane always does', () => {
    assert.equal(isSameTabPlace(['a', 'b', 'c'], 'a', 'tab-before', 'b'), true);
    assert.equal(isSameTabPlace(['a', 'b', 'c'], 'a', 'tab-after', 'a'), true);
    assert.equal(isSameTabPlace(['a', 'b', 'c'], 'c', 'tab-after', 'b'), true);
    assert.equal(isSameTabPlace(['a', 'b', 'c'], 'a', 'tab-after', 'b'), false);
    assert.equal(isSameTabPlace(['a', 'b', 'c'], 'c', 'tab-before', 'a'), false);
    assert.equal(isSameTabPlace(['b', 'c'], 'a', 'tab-before', 'b'), false);
});
