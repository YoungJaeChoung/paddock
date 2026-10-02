const test = require('node:test');
const assert = require('node:assert/strict');
const { isViewportStale } = require('../window-fit');

test('C-fit.1: 확대 배율을 곱한 화면 크기가 창 내용과 몇 픽셀 안이면 따라온 것으로 본다', () => {
    assert.equal(isViewportStale([1728, 3000], [1200, 2083], 1.44), false);
    assert.equal(isViewportStale([1000, 700], [1000, 700], 1), false);
});

test('C-fit.2: 최대화 전 크기에 머문 화면은 따라오지 못한 것이다', () => {
    assert.equal(isViewportStale([1728, 3000], [949, 569], 1.44), true);
    assert.equal(isViewportStale([1728, 3000], [1200, 1500], 1.44), true);
});
