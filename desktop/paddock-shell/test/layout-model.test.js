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

test('C-layout.9: 여러 묶음의 배치를 나란히 이으면 묶음별로 다시 나눌 때 각 묶음의 칸 나눔이 돌아온다', () => {
    const shown = split([tabs([w('a')]), split([tabs([w('b')]), tabs([w('c')])], undefined, 'vertical')]);
    const other = split([tabs([w('x')]), tabs([w('y')])]);
    const merged = layout.besides([shown, null, other]);
    assert.deepEqual(ids(merged), ['a', 'b', 'c', 'x', 'y']);
    assert.deepEqual(layout.prune(merged, widget => ['a', 'b', 'c'].includes(widget.id)), shown);
    assert.deepEqual(layout.prune(merged, widget => ['x', 'y'].includes(widget.id)), other);
    assert.equal(layout.besides([null, other]), other);
    assert.equal(layout.besides([null]), null);
});

test('새 가로 칸은 세 번째 칸도 같은 폭을 얻고 기존 탭 순서와 선택을 유지한다', () => {
    const added = w('new');
    const area = split([tabs([w('a'), w('b')], 1), tabs([w('c')]), tabs([added])], [0.42, 0.42, 0.16]);
    const balanced = layout.balanceNewSplit(area, added, 'horizontal');
    assert.deepEqual(balanced.sizes, [1 / 3, 1 / 3, 1 / 3]);
    assert.deepEqual(balanced.children, area.children);
    assert.deepEqual(area.sizes, [0.42, 0.42, 0.16]);
});

test('새 세로 칸의 높이만 나누고 바깥 가로 칸의 사용자 비율은 보존한다', () => {
    const added = w('new');
    const area = split([tabs([w('a')]), split([tabs([w('b')]), tabs([added])], [0.8, 0.2], 'vertical')], [0.7, 0.3]);
    const balanced = layout.balanceNewSplit(area, added, 'vertical');
    assert.deepEqual(balanced.sizes, [0.7, 0.3]);
    assert.deepEqual(balanced.children[1].sizes, [0.5, 0.5]);
});

test('없는 위젯이나 다른 방향을 요청하면 저장한 칸 비율을 바꾸지 않는다', () => {
    const original = w('original');
    const area = split([tabs([original]), tabs([w('other')])], [0.6, 0.4]);
    assert.deepEqual(layout.balanceNewSplit(area, w('missing'), 'horizontal'), area);
    assert.deepEqual(layout.balanceNewSplit(area, original, 'vertical'), area);
    assert.equal(layout.balanceNewSplit(null, original, 'horizontal'), null);
});

test('저장 참조는 실제 위젯의 순환 참조를 제외하고 자원 주소와 문자열 화면 종류만 담는다', () => {
    const editor = { ...w('editor'), getResourceUri: () => ({ toString: () => 'file:///work/note.md' }) };
    const preview = { ...w('preview'), getResourceUri: editor.getResourceUri, viewType: 'markdown-preview' };
    const terminal = { ...w('terminal'), viewType: 7 };
    editor.parent = editor;
    const saved = layout.serialize(tabs([editor, preview, terminal], 1));
    assert.deepEqual(JSON.parse(JSON.stringify(saved)), tabs([
        { id: 'editor', uri: 'file:///work/note.md' },
        { id: 'preview', uri: 'file:///work/note.md', viewType: 'markdown-preview' },
        { id: 'terminal' },
    ], 1));
    assert.equal(layout.serialize(null), null);
});

test('저장·복원은 중첩 분할의 사용자 비율·방향·선택 탭을 그대로 유지한다', () => {
    const widgets = [w('a'), w('b'), w('c'), w('d')];
    const area = split([
        tabs(widgets.slice(0, 2), 1),
        split([tabs([widgets[2]]), tabs([widgets[3]])], [0.19, 0.81], 'vertical'),
    ], [0.73, 0.27]);
    const saved = JSON.parse(JSON.stringify(layout.serialize(area)));
    const originalSaved = structuredClone(saved);
    const restored = layout.restore(saved, reference => widgets.find(widget => widget.id === reference.id));
    assert.deepEqual(restored, area);
    assert.equal(restored.children[0].widgets[1], widgets[1]);
    restored.sizes[0] = 0.5;
    restored.children[1].sizes[0] = 0.5;
    assert.deepEqual(saved, originalSaved);
    assert.deepEqual(area.sizes, [0.73, 0.27]);
});

test('누락·종료된 칸의 크기만 빼고 남은 비율을 유지하며 자식 하나인 분할은 줄인다', () => {
    const a = w('a');
    const b = w('b');
    const disposed = w('disposed', true);
    const saved = split([
        tabs([{ id: 'a' }]),
        tabs([{ id: 'missing' }]),
        split([tabs([{ id: 'disposed' }]), tabs([{ id: 'b' }])], [0.6, 0.4], 'vertical'),
    ], [0.25, 0.1, 0.65]);
    const originalSaved = structuredClone(saved);
    const restored = layout.restore(saved, reference => [a, b, disposed].find(widget => widget.id === reference.id));
    assert.deepEqual(restored, split([tabs([a]), tabs([b])], [0.25, 0.65]));
    assert.deepEqual(saved, originalSaved);
    assert.equal(layout.restore(tabs([{ id: 'disposed' }, { id: 'missing' }]), reference => reference.id === 'disposed' ? disposed : undefined), null);
});

test('선택 탭이 살아 있으면 위치를 찾아 유지하고 사라지면 기존 정리 규칙대로 대체한다', () => {
    const widgets = [w('a'), w('b'), w('c')];
    for (const currentIndex of [0, 1, 2, 3]) {
        const saved = tabs([{ id: 'a' }, { id: 'missing' }, { id: 'b' }, { id: 'c' }], currentIndex);
        const restored = layout.restore(saved, reference => widgets.find(widget => widget.id === reference.id));
        const expected = layout.prune(tabs([widgets[0], w('missing'), widgets[1], widgets[2]], currentIndex), widget => widget.id !== 'missing');
        assert.deepEqual(restored, expected);
    }
});

test('서로 다른 저장 참조가 같은 위젯을 찾으면 전체 배치에서 첫 칸에 한 번만 둔다', () => {
    const a = w('a');
    const b = w('b');
    const saved = split([
        tabs([{ id: 'a' }, { id: 'b' }, { id: 'a-old' }], 2),
        tabs([{ id: 'a' }]),
        tabs([{ id: 'b' }]),
    ], [0.2, 0.3, 0.5]);
    const restored = layout.restore(saved, reference => reference.id === 'b' ? b : a);
    assert.deepEqual(restored, tabs([a, b], 0));
});

test('저장 참조의 자원 주소와 화면 종류를 복원 콜백에 전달하고 콜백 수정은 원본과 분리한다', () => {
    const preview = w('new-preview-id');
    const reference = { id: 'old-preview-id', uri: 'file:///work/note.md', viewType: 'markdown-preview' };
    const saved = tabs([reference]);
    const restored = layout.restore(saved, candidate => {
        assert.deepEqual(candidate, reference);
        candidate.id = 'changed-by-resolver';
        return preview;
    });
    assert.deepEqual(restored, tabs([preview]));
    assert.equal(reference.id, 'old-preview-id');
});

test('잘못된 저장 칸·참조를 건너뛰고 잘못된 비율과 선택 위치만 안전한 값으로 바꾼다', () => {
    const a = w('a');
    const b = w('b');
    const c = w('c');
    const saved = split([
        null,
        { type: 'tab-area', widgets: 'invalid' },
        tabs([null, 4, {}, { id: 'a', uri: 4, viewType: [] }], -1),
        tabs([{ id: 'b' }], 'last'),
        tabs([{ id: 'c' }], 900),
    ], [0.1, 0.2, -1, 0, 0.7]);
    const originalSaved = structuredClone(saved);
    const restored = layout.restore(saved, reference => {
        assert.deepEqual(Object.keys(reference), ['id']);
        return [a, b, c].find(widget => widget.id === reference.id);
    });
    assert.deepEqual(restored, split([tabs([a]), tabs([b]), tabs([c])], [1, 0, 0.7]));
    assert.deepEqual(saved, originalSaved);
    for (const sizes of [undefined, 'bad', [NaN, Infinity]]) {
        const savedWithBadSizes = { ...split([tabs([{ id: 'a' }]), tabs([{ id: 'b' }])]), sizes };
        assert.deepEqual(layout.restore(savedWithBadSizes, reference => reference.id === 'a' ? a : b).sizes, [1, 1]);
    }
});

test('복원할 수 없는 저장값은 예외 없이 null이며 참조 해석 실패는 해당 탭만 제거한다', () => {
    for (const saved of [null, undefined, false, 7, 'bad', [], {}, { type: 'unknown' }, { type: 'tab-area' }, { type: 'split-area', children: [] }, split([], []), split([tabs([{ id: 'a' }])], [1], 'invalid')]) {
        assert.equal(layout.restore(saved, () => { throw new Error('unexpected reference'); }), null);
    }
    const alive = w('alive');
    const restored = layout.restore(tabs([{ id: 'broken' }, { id: 'alive' }]), reference => {
        if (reference.id === 'broken') throw new Error('unreadable resource');
        return alive;
    });
    assert.deepEqual(restored, tabs([alive]));
});
