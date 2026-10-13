const test = require('node:test');
const assert = require('node:assert/strict');
const { WINDOW_ROLE, windowRole } = require('../window-role');

test('창 주소에 두 번째 창 표시가 있을 때만 secondary다', () => {
    assert.equal(windowRole('?port=3000'), WINDOW_ROLE.PRIMARY);
    assert.equal(windowRole('?port=3000&paddockWindow=secondary'), WINDOW_ROLE.SECONDARY);
    assert.equal(windowRole('?paddockWindow=other'), WINDOW_ROLE.PRIMARY);
    assert.equal(windowRole(''), WINDOW_ROLE.PRIMARY);
    assert.equal(windowRole(undefined), WINDOW_ROLE.PRIMARY);
});
