const test = require('node:test');
const assert = require('node:assert/strict');
const { WorkPresenceRegistry } = require('../work-presence');

test('창은 다른 창의 터미널 목록만 받고 화면 데이터는 받지 않는다', () => {
    const registry = new WorkPresenceRegistry();
    const snapshot = { folders: [{ key: 'file:///a', expanded: true }], terminals: [{ id: 't0', folder: 'file:///a', name: 'claude', program: 'claude', screen: 'secret output' }] };
    registry.update('first', snapshot, 1000);
    const others = registry.update('second', { folders: [], terminals: [] }, 1001);
    assert.deepEqual(others, [{ windowId: 'first', folders: [{ key: 'file:///a', expanded: true, label: '' }], terminals: [{ id: 't0', folder: 'file:///a', name: 'claude', program: 'claude' }] }]);
    registry.retire('first');
    assert.deepEqual(registry.list('second', 1002), []);
});

test('닫힌 창의 목록은 종료 신호가 없어도 만료된다', () => {
    const registry = new WorkPresenceRegistry();
    registry.update('closed', { folders: [], terminals: [] }, 1000);
    assert.equal(registry.list('current', 11000).length, 1);
    assert.deepEqual(registry.list('current', 16001), []);
});

test('다른 창에서 연결할 프로세스 ID만 전달하고 잘못된 ID는 제외한다', () => {
    const registry = new WorkPresenceRegistry();
    registry.update('first', { terminals: [
        { id: 'live', folder: 'file:///tmp', name: 'shell', terminalId: 7, env: { TOKEN: 'secret' } },
        { id: 'starting', folder: 'file:///tmp', name: 'shell', terminalId: -1 },
    ] }, 1000);
    const terminals = registry.list('second', 1001)[0].terminals;
    assert.equal(terminals[0].terminalId, 7);
    assert.equal('env' in terminals[0], false);
    assert.equal('terminalId' in terminals[1], false);
});

const closedWindow = registry => registry.update('closed', { folders: [{ key: 'file:///a' }], terminals: [
    { id: 't0', folder: 'file:///a', name: 'one', terminalId: 1 },
    { id: 't1', folder: '', name: 'two', terminalId: 2 },
    { id: 't2', folder: '', name: 'starting', terminalId: -1 },
] }, 1000);

test('창이 닫히면 살아 있는 터미널만 퇴역 표시로 남는다', () => {
    const registry = new WorkPresenceRegistry();
    closedWindow(registry);
    registry.retire('closed', id => id === 2);
    const [snapshot] = registry.list('other', 2000, id => id === 2);
    assert.equal(snapshot.retired, true);
    assert.deepEqual(snapshot.terminals.map(terminal => [terminal.id, terminal.folder]), [['t1', '']]);
});

test('퇴역 기록은 시간으로 만료되지 않고 프로세스가 끝나면 사라진다', () => {
    const registry = new WorkPresenceRegistry();
    closedWindow(registry);
    registry.retire('closed', () => true);
    assert.equal(registry.list('other', 1000 + 3600000, () => true)[0].terminals.length, 2);
    assert.deepEqual(registry.list('other', 1000 + 3600000, id => id === 1)[0].terminals.map(terminal => terminal.terminalId), [1]);
    assert.deepEqual(registry.list('other', 1000 + 3600000, () => false), []);
});

test('살아 있는 창이 같은 터미널을 보고하면 퇴역 기록에서 빠진다', () => {
    const registry = new WorkPresenceRegistry();
    closedWindow(registry);
    registry.retire('closed', () => true);
    registry.update('adopter', { folders: [], terminals: [{ id: 'n0', folder: '', name: 'two', terminalId: 2 }] }, 2000, () => true);
    const retired = registry.list('adopter', 2001, () => true).filter(snapshot => snapshot.retired);
    assert.deepEqual(retired[0].terminals.map(terminal => terminal.terminalId), [1]);
    registry.update('adopter', { folders: [], terminals: [{ id: 'n1', folder: '', name: 'one', terminalId: 1 }, { id: 'n0', folder: '', name: 'two', terminalId: 2 }] }, 2002, () => true);
    assert.deepEqual(registry.list('adopter', 2003, () => true), []);
});

test('소식이 끊긴 창은 지우지 않고 퇴역시킨다', () => {
    const registry = new WorkPresenceRegistry();
    closedWindow(registry);
    const snapshots = registry.list('other', 16001, id => id === 1);
    assert.equal(snapshots[0].retired, true);
    assert.deepEqual(snapshots[0].terminals.map(terminal => terminal.terminalId), [1]);
});

test('작업 폴더가 없는 터미널(folder 빈 문자열)도 저장된다', () => {
    const registry = new WorkPresenceRegistry();
    registry.update('first', { terminals: [{ id: 't0', folder: '', name: 'terminal 1', terminalId: 3 }] }, 1000);
    assert.equal(registry.list('second', 1001)[0].terminals[0].folder, '');
});
