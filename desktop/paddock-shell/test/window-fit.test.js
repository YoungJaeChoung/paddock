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

test('C-fit.3: 최대화 표시만 남고 작업 영역을 채우지 않는 창은 다시 최대화하지 않는다', () => {
    const { fillsWorkArea } = require('../window-fit');
    const workArea = { x: 0, y: 0, width: 2560, height: 1552 };
    assert.equal(fillsWorkArea({ x: -8, y: -8, width: 2576, height: 1568 }, workArea), true);
    assert.equal(fillsWorkArea({ x: 0, y: 0, width: 2560, height: 1552 }, workArea), true);
    assert.equal(fillsWorkArea({ x: 200, y: 200, width: 1800, height: 1100 }, workArea), false);
    // 다른 모니터(왼쪽 위 4K)의 최대화 크기는 이 작업 영역을 채우지 않는다.
    assert.equal(fillsWorkArea({ x: -1302, y: -2168, width: 3856, height: 2128 }, workArea), false);
});

test('C-fit.4: 최대화 전 자리를 지금 모니터 작업 영역 안으로 옮긴다', () => {
    const { boundsWithin } = require('../window-fit');
    const workArea = { x: 0, y: 0, width: 2560, height: 1552 };
    assert.deepEqual(boundsWithin({ x: -1000, y: -1900, width: 1600, height: 1000 }, workArea), { x: 0, y: 0, width: 1600, height: 1000 });
    assert.deepEqual(boundsWithin({ x: 100, y: 100, width: 800, height: 600 }, workArea), { x: 100, y: 100, width: 800, height: 600 });
    assert.deepEqual(boundsWithin({ x: 0, y: 0, width: 3000, height: 2000 }, workArea), workArea);
    assert.deepEqual(boundsWithin({ x: 2400, y: 1500, width: 800, height: 600 }, workArea), { x: 1760, y: 952, width: 800, height: 600 });
});
