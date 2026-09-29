const test = require('node:test');
const assert = require('node:assert/strict');
const { WorkPresenceRegistry } = require('../work-presence');

test('창은 다른 창의 터미널 목록만 받고 화면 데이터는 받지 않는다', () => {
    const registry = new WorkPresenceRegistry();
    const snapshot = { folders: [{ key: 'file:///a', expanded: true }], terminals: [{ id: 't0', folder: 'file:///a', name: 'claude', program: 'claude', screen: 'secret output' }] };
    registry.update('first', snapshot, 1000);
    const others = registry.update('second', { folders: [], terminals: [] }, 1001);
    assert.deepEqual(others, [{ windowId: 'first', folders: [{ key: 'file:///a', expanded: true, label: '' }], terminals: [{ id: 't0', folder: 'file:///a', name: 'claude', program: 'claude' }] }]);
    registry.remove('first');
    assert.deepEqual(registry.list('second', 1002), []);
});

test('닫힌 창의 목록은 종료 신호가 없어도 만료된다', () => {
    const registry = new WorkPresenceRegistry();
    registry.update('closed', { folders: [], terminals: [] }, 1000);
    assert.equal(registry.list('current', 11000).length, 1);
    assert.deepEqual(registry.list('current', 16001), []);
});
