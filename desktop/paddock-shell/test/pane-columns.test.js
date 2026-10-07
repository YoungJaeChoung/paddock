const test = require('node:test');
const assert = require('node:assert/strict');
const { paneColumns, groupTabsByColumn, columnSpans } = require('../pane-columns');

test('PC01: side-by-side panes form separate columns ordered from the left', () => {
    const columns = paneColumns([{ key: 'b', left: 500, top: 0 }, { key: 'a', left: 0, top: 0 }]);
    assert.deepEqual(columns, [{ left: 0, keys: ['a'] }, { left: 500, keys: ['b'] }]);
});

test('PC02: panes stacked top to bottom share one column in top order, within the tolerance', () => {
    const columns = paneColumns([{ key: 'bottom', left: 500.6, top: 300 }, { key: 'a', left: 0, top: 0 }, { key: 'top', left: 500, top: 0 }]);
    assert.deepEqual(columns, [{ left: 0, keys: ['a'] }, { left: 500, keys: ['top', 'bottom'] }]);
    assert.deepEqual(paneColumns([]), []);
});

test('PC03: a full-width bottom pane joins the column of the pane above its left edge', () => {
    const columns = paneColumns([{ key: 't1', left: 0, top: 0 }, { key: 't2', left: 400, top: 0 }, { key: 'b', left: 0, top: 300 }]);
    assert.deepEqual(columns, [{ left: 0, keys: ['t1', 'b'] }, { left: 400, keys: ['t2'] }]);
});

test('PC04: tabs follow their pane column, keep the strip order inside a pane, and stacked panes go top first', () => {
    const paneOf = id => ({ t1: 'C', t2: 'B', f: 'B' })[id] ?? null;
    const columns = [{ left: 0, keys: ['A'] }, { left: 500, keys: ['B', 'C'] }];
    assert.deepEqual(groupTabsByColumn(['t1', 't2', 'f'], paneOf, columns), [[], ['t2', 'f', 't1']]);
});

test('PC05: tabs without a pane trail the last column, and no columns means one group', () => {
    const paneOf = id => (id === 't1' ? 'A' : null);
    assert.deepEqual(groupTabsByColumn(['w', 't1'], paneOf, [{ left: 0, keys: ['A'] }]), [['t1', 'w']]);
    assert.deepEqual(groupTabsByColumn(['t1', 'w'], paneOf, []), [['t1', 'w']]);
});

test('PC06: each column spans to the next column and the last reaches the strip end; clipped lefts start at 0', () => {
    const columns = [{ left: 0, keys: ['a'] }, { left: 500, keys: ['b'] }];
    assert.deepEqual(columnSpans(columns, 30, 900), [{ left: 0, width: 470 }, { left: 470, width: 430 }]);
    assert.deepEqual(columnSpans([{ left: 0, keys: ['a'] }], 30, 900), [{ left: 0, width: 900 }]);
    // 줄보다 오른쪽에서 시작하는 열(오른쪽 버튼에 가린 열)은 폭 0으로 남고 음수가 되지 않는다.
    assert.deepEqual(columnSpans([{ left: 0, keys: ['a'] }, { left: 1000, keys: ['b'] }], 30, 900), [{ left: 0, width: 900 }, { left: 900, width: 0 }]);
});
