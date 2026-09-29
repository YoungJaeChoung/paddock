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

test('다른 창의 같은 id 터미널은 목록에 함께 보이되 이 창의 터미널 소속을 바꾸지 않는다', () => {
    const local = withTerminals('file:///a', ['claude']);
    const snapshot = { windowId: 'other-window', folders: [{ key: 'file:///b', expanded: false }], terminals: [{ id: 't0', folder: 'file:///b', name: 'codex', program: 'codex' }] };
    const { state, remote } = model.mergeWorkPresence(local, [snapshot]);
    assert.deepEqual(state.folders.map(folder => folder.key), ['file:///a', 'file:///b']);
    assert.deepEqual(model.visibleRows(state, 't0', new Set()).filter(row => row.kind === 'terminal').map(row => row.name), ['claude', 'codex']);
    assert.equal(model.folderOf(state, 't0'), 'file:///a');
    assert.equal(model.folderOf(state, 'other-window:t0'), 'file:///b');
    assert.equal(remote.get('other-window:t0').program, 'codex');
    assert.equal(local.folders.length, 1);
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

test('C-work-F8.1: 에이전트가 실행 중인 추가 터미널은 현재 폴더의 작업 터미널이 되고 이름은 에이전트 이름이다', () => {
    const { state, promoted, added } = model.promoteAgentTerminals(model.empty(), [
        { id: 't1', cwd: 'file:///home/me/repo/', program: 'claude', isAgent: true },
    ]);
    assert.deepEqual(promoted, ['t1']);
    assert.deepEqual(added, ['file:///home/me/repo']);
    assert.deepEqual(state.terminals.t1, { folder: 'file:///home/me/repo', name: 'claude' });
});

test('C-work-F8.2: 그 폴더가 이미 목록에 있으면 새 폴더를 만들지 않고 붙인다', () => {
    const before = withTerminals('file:///repo', ['terminal']);
    const { state, promoted, added } = model.promoteAgentTerminals(before, [
        { id: 'x', cwd: 'file:///repo', program: 'codex', isAgent: true },
    ]);
    assert.deepEqual(promoted, ['x']);
    assert.deepEqual(added, []);
    assert.equal(state.folders.length, 1);
    assert.deepEqual(model.terminalsOf(state, 'file:///repo'), ['t0', 'x']);
});

test('C-work-F8.3: 다른 폴더로 이동해 에이전트를 실행하면 터미널도 현재 폴더로 옮긴다', () => {
    const before = withTerminals('file:///a', ['terminal']);
    const { state, promoted, added } = model.promoteAgentTerminals(before, [
        { id: 't0', cwd: 'file:///b', program: 'claude', isAgent: true },
    ]);
    assert.deepEqual(promoted, ['t0']);
    assert.deepEqual(added, ['file:///b']);
    assert.equal(model.folderOf(state, 't0'), 'file:///b');
    assert.equal(state.terminals.t0.name, 'claude');
    assert.equal(model.terminalsOf(state, 'file:///a').length, 0);
});

test('C-work-F8.4: 에이전트가 아니거나 현재 폴더를 모르는 터미널은 올리지 않는다', () => {
    const { state, promoted } = model.promoteAgentTerminals(model.empty(), [
        { id: 'shell', cwd: 'file:///a', program: 'bash', isAgent: false },
        { id: 'unknown', cwd: undefined, program: 'claude', isAgent: true },
    ]);
    assert.deepEqual(promoted, []);
    assert.equal(state.folders.length, 0);
});

test('C-work-F9.1: 탭 줄 행은 그 폴더의 터미널만, 같은 이름은 ·2·3을 붙여 사이드바와 같게 돌려준다', () => {
    let state = withTerminals('file:///a', ['claude', 'terminal', 'claude']);
    state = model.assignTerminal(state, 'other', 'file:///b', 'claude');
    const rows = model.terminalRows(state, 'file:///a/');
    assert.deepEqual(rows.map(row => [row.id, row.name, row.suffix]), [['t0', 'claude', ''], ['t1', 'terminal', ''], ['t2', 'claude', '·2']]);
});

test('C-work-F10.1: 탭 이동은 같은 줄 안에서 이전·다음은 끝에서 반대쪽으로 돌고, 처음·마지막은 양 끝으로 간다', () => {
    const ids = ['a', 'b', 'c'];
    assert.equal(model.tabTarget(ids, 'b', 'previous'), 'a');
    assert.equal(model.tabTarget(ids, 'a', 'previous'), 'c');
    assert.equal(model.tabTarget(ids, 'c', 'next'), 'a');
    assert.equal(model.tabTarget(ids, 'b', 'first'), 'a');
    assert.equal(model.tabTarget(ids, 'b', 'last'), 'c');
});

test('C-work-F10.2: 현재 터미널이 줄에 없거나 줄이 비면 이동하지 않는다', () => {
    assert.equal(model.tabTarget(['a', 'b'], 'x', 'next'), null);
    assert.equal(model.tabTarget([], 'x', 'first'), null);
});
