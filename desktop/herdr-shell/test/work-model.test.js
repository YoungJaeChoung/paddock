const test = require('node:test');
const assert = require('node:assert/strict');
const model = require('../work-model');

function withTerminals(
    key,
    names,
) {
    let state = model.ensureFolder(model.empty(), key);
    names.forEach((name, index) => {
        state = model.assignTerminal(state, `t${index}`, key, name);
    });
    return state;
}

test('C-work-F1.1: 처음 보는 폴더는 목록 끝에 생긴다', () => {
    let state = model.ensureFolder(model.empty(), 'file:///home/a');
    state = model.ensureFolder(state, 'file:///home/b');
    assert.deepEqual(state.folders.map(folder => folder.key), ['file:///home/a', 'file:///home/b']);
});

test('C-work-F1.2: 이미 있는 폴더는 끝 슬래시가 달라도 새로 만들지 않는다', () => {
    let state = model.ensureFolder(model.empty(), 'file:///home/a');
    state = model.ensureFolder(state, 'file:///home/a/');
    assert.equal(state.folders.length, 1);
});

test('C-work-F1.3: 터미널을 다른 폴더에 배정하면 이전 폴더에서 빠진다', () => {
    let state = withTerminals('file:///a', ['claude']);
    state = model.ensureFolder(state, 'file:///b');
    state = model.assignTerminal(state, 't0', 'file:///b', 'claude');
    assert.equal(model.folderOf(state, 't0'), 'file:///b');
    assert.deepEqual(model.terminalsOf(state, 'file:///a'), []);
});

test('C-work-F1.4: 파일이 겹친 작업 폴더(홈과 그 아래 레포)에 속하면 가장 가까운 폴더를 고른다', () => {
    let state = model.ensureFolder(model.empty(), 'file:///home/me');
    state = model.ensureFolder(state, 'file:///home/me/repo');
    assert.equal(model.folderContaining(state, 'file:///home/me/repo/README.md'), 'file:///home/me/repo');
    assert.equal(model.folderContaining(state, 'file:///home/me/notes.md'), 'file:///home/me');
    assert.equal(model.folderContaining(state, 'file:///home/me/repository/a.md'), 'file:///home/me');
    assert.equal(model.folderContaining(state, 'file:///tmp/x'), null);
});

test('C-work-F2.1: 폴더를 빼면 그 폴더 터미널만 닫을 목록으로 돌려준다', () => {
    let state = withTerminals('file:///a', ['x', 'y']);
    state = model.ensureFolder(state, 'file:///b');
    state = model.assignTerminal(state, 'b1', 'file:///b', 'z');
    const result = model.removeFolder(state, 'file:///a', 'b1');
    assert.deepEqual(result.closed.sort(), ['t0', 't1']);
    assert.deepEqual(result.state.folders.map(folder => folder.key), ['file:///b']);
    assert.equal(model.folderOf(result.state, 'b1'), 'file:///b');
});

test('C-work-F2.2: 현재 폴더를 빼면 위 폴더(없으면 아래)의 첫 터미널이 다음 선택이다', () => {
    let state = model.empty();
    for (const [key, id] of [['file:///a', 'a1'], ['file:///b', 'b1'], ['file:///c', 'c1']]) {
        state = model.ensureFolder(state, key);
        state = model.assignTerminal(state, id, key, id);
    }
    assert.equal(model.removeFolder(state, 'file:///b', 'b1').next, 'a1');
    assert.equal(model.removeFolder(state, 'file:///a', 'a1').next, 'b1');
    assert.equal(model.removeFolder(state, 'file:///c', 'b1').next, 'b1');
    const single = model.assignTerminal(model.empty(), 'only', 'file:///x', 'x');
    assert.equal(model.removeFolder(single, 'file:///x', 'only').next, null);
});

test('C-work-F2.3: 터미널이 닫혀도 폴더는 남는다', () => {
    const state = model.forgetTerminal(withTerminals('file:///a', ['x']), 't0');
    assert.equal(state.folders.length, 1);
    assert.deepEqual(model.terminalsOf(state, 'file:///a'), []);
});

test('C-work-F3.1: 한 폴더에 8개를 넘으면 8줄과 "N more" 줄을 돌려준다', () => {
    const state = withTerminals('file:///a', Array.from({ length: 20 }, (_, index) => `n${index}`));
    const rows = model.visibleRows(state, null, new Set());
    assert.equal(rows.filter(row => row.kind === 'terminal').length, 8);
    assert.deepEqual(rows.find(row => row.kind === 'more'), { kind: 'more', folder: 'file:///a', n_hidden: 12 });
});

test('C-work-F3.2: 현재 터미널이 9번째 이후여도 보인다', () => {
    const state = withTerminals('file:///a', Array.from({ length: 20 }, (_, index) => `n${index}`));
    const rows = model.visibleRows(state, 't15', new Set());
    assert(rows.some(row => row.kind === 'terminal' && row.id === 't15'));
    assert.equal(rows.find(row => row.kind === 'more').n_hidden, 11);
});

test('C-work-F3.3: 같은 이름은 두 번째부터 ·2, ·3을 붙인다', () => {
    const rows = model.visibleRows(withTerminals('file:///a', ['mind', 'git', 'mind', 'mind']), null, new Set());
    assert.deepEqual(rows.filter(row => row.kind === 'terminal').map(row => row.suffix), ['', '', '·2', '·3']);
});

test('C-work-F3.4: 현재 터미널이 속한 폴더는 접혀 있어도 펼친다', () => {
    let state = withTerminals('file:///a', ['x']);
    state = model.setExpanded(state, 'file:///a', false);
    assert(model.visibleRows(state, 't0', new Set()).some(row => row.id === 't0'));
    assert(!model.visibleRows(state, null, new Set()).some(row => row.id === 't0'));
});

test('C-work-F4.1: 복원 시 없는 터미널 id는 버리고 폴더는 남긴다', () => {
    const saved = model.serialize(withTerminals('file:///a', ['x', 'y']));
    const state = model.restore(saved, new Set(['t1']));
    assert.equal(state.folders.length, 1);
    assert.deepEqual(model.terminalsOf(state, 'file:///a'), ['t1']);
});

test('C-work-F4.2: 깨진 저장 값은 빈 목록으로 시작한다', () => {
    assert.deepEqual(model.restore('{not json', new Set()), model.empty());
    assert.deepEqual(model.restore(JSON.stringify({ folders: 3 }), new Set()), model.empty());
});

test('C-work-F5.1: 메모리 사용률은 (전체-가용)/전체를 0~100 정수로 반올림한다', () => {
    assert.equal(model.memoryPercent(16, 9.76), 39);
    assert.equal(model.memoryPercent(0, 0), null);
});

test('C-work-F3.5: 폴더 목록 이름을 바꾸면 그 이름을 쓰고, 비우면 폴더 이름으로 돌아간다', () => {
    let state = model.ensureFolder(model.empty(), 'file:///home/a');
    state = model.setFolderLabel(state, 'file:///home/a/', 'mind');
    assert.equal(model.folderLabel(state, 'file:///home/a'), 'mind');
    state = model.setFolderLabel(state, 'file:///home/a', '  ');
    assert.equal(model.folderLabel(state, 'file:///home/a'), null);
    assert.equal(model.folderLabel(model.restore(model.serialize(model.setFolderLabel(state, 'file:///home/a', 'x')), new Set()), 'file:///home/a'), 'x');
});
