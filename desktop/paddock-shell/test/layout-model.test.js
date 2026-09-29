const test = require('node:test');
const assert = require('node:assert/strict');
const layout = require('../layout-model');

const w = (id, isDisposed = false) => ({ id, isDisposed });
const tabs = (widgets, currentIndex = 0) => ({ type: 'tab-area', widgets, currentIndex });
const split = (children, sizes = children.map(() => 1), orientation = 'horizontal') => ({ type: 'split-area', orientation, children, sizes });
const ids = area => layout.widgetsOf(area).map(widget => widget.id);

test('C-layout.1: 배치의 위젯을 왼쪽·위부터 순서대로 돌려준다', () => {
    const area = split([tabs([w('a'), w('b')]), split([tabs([w('c')]), tabs([w('d')])], undefined, 'vertical')]);
    assert.deepEqual(ids(area), ['a', 'b', 'c', 'd']);
    assert.deepEqual(ids(null), []);
});

test('C-layout.2: 다른 묶음의 위젯을 빼면 빈 칸은 사라지고 칸 크기 비율은 남은 칸끼리 유지한다', () => {
    const area = split([tabs([w('a')]), tabs([w('x')]), tabs([w('b')])], [1, 2, 3]);
    const pruned = layout.prune(area, widget => widget.id !== 'x');
    assert.deepEqual(ids(pruned), ['a', 'b']);
    assert.deepEqual(pruned.sizes, [1, 3]);
});

test('C-layout.3: 자식이 하나만 남은 분할은 그 자식이 된다', () => {
    const area = split([tabs([w('a')]), tabs([w('x')])]);
    assert.deepEqual(layout.prune(area, widget => widget.id === 'a'), tabs([w('a')]));
});

test('C-layout.4: 남은 위젯이 없으면 null이다', () => {
    assert.equal(layout.prune(split([tabs([w('x')])]), () => false), null);
});

test('C-layout.5: 선택된 탭이 사라지면 가장 가까운 탭으로 옮기고 남아 있으면 유지한다', () => {
    const area = tabs([w('a'), w('x'), w('b')], 1);
    assert.equal(layout.prune(area, widget => widget.id !== 'x').currentIndex, 1);
    const kept = tabs([w('x'), w('a'), w('b')], 2);
    const pruned = layout.prune(kept, widget => widget.id !== 'x');
    assert.equal(pruned.widgets[pruned.currentIndex].id, 'b');
});

test('C-layout.6: 닫힌 위젯은 복원 전에 뺀다', () => {
    const area = split([tabs([w('a')]), tabs([w('d', true)])]);
    assert.deepEqual(ids(layout.withoutDisposed(area)), ['a']);
});

test('C-layout.7: 위젯을 첫 칸 끝에 탭으로 더하고 선택한다', () => {
    const area = split([tabs([w('a')]), tabs([w('b')])]);
    const next = layout.withWidget(area, w('n'));
    assert.deepEqual(ids(next), ['a', 'n', 'b']);
    assert.equal(next.children[0].currentIndex, 1);
    assert.deepEqual(ids(layout.withWidget(null, w('n'))), ['n']);
});

test('C-layout.8: 배치의 위젯을 선택된 탭으로 만든다', () => {
    const b = w('b');
    const area = split([tabs([w('a')]), tabs([w('c'), b])]);
    assert.equal(layout.select(area, b).children[1].currentIndex, 1);
    assert.equal(layout.includes(area, b), true);
});
