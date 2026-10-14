const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { ROTATION, rotateIfLarge } = require('../log-rotation');

const MB = 1024 * 1024;
const directory = () => fs.mkdtempSync(path.join(os.tmpdir(), 'paddock-rotation-'));

test('C-rotation.1: 20 MB를 넘는 파일은 .1로 넘기고 원래 이름은 비운다', () => {
    const dir = directory();
    const file = path.join(dir, 'backend.log');
    fs.writeFileSync(file, Buffer.alloc(21 * MB, 'a'));
    assert.equal(rotateIfLarge(file), true);
    assert.equal(fs.existsSync(file), false);
    assert.equal(fs.statSync(`${file}.1`).size, 21 * MB);
});

test('C-rotation.2: 기존 보관본은 한 칸씩 밀고 가장 오래된 .3은 지운다', () => {
    const dir = directory();
    const file = path.join(dir, 'backend.log');
    fs.writeFileSync(file, 'new'.padEnd(ROTATION.MAX_BYTES + 1, 'n'));
    fs.writeFileSync(`${file}.1`, 'one');
    fs.writeFileSync(`${file}.2`, 'two');
    fs.writeFileSync(`${file}.3`, 'three');
    rotateIfLarge(file);
    assert.equal(fs.readFileSync(`${file}.1`, 'utf8').slice(0, 3), 'new');
    assert.equal(fs.readFileSync(`${file}.2`, 'utf8'), 'one');
    assert.equal(fs.readFileSync(`${file}.3`, 'utf8'), 'two');
    assert.equal(fs.existsSync(`${file}.4`), false);
});

test('C-rotation.3: 작거나 없는 파일은 그대로 둔다', () => {
    const dir = directory();
    const file = path.join(dir, 'backend.log');
    assert.equal(rotateIfLarge(file), false);
    fs.writeFileSync(file, 'small');
    assert.equal(rotateIfLarge(file), false);
    assert.equal(fs.readFileSync(file, 'utf8'), 'small');
    assert.equal(fs.existsSync(`${file}.1`), false);
});

test('C-rotation.4: 회전이 실패하면 던지지 않고 알린 뒤 원래 파일을 그대로 둔다', () => {
    const { tryRotate } = require('../log-rotation');
    const dir = directory();
    const file = path.join(dir, 'backend.log');
    fs.writeFileSync(file, Buffer.alloc(21 * MB, 'a'));
    // 가장 오래된 보관 칸을 비어 있지 않은 폴더로 막아 삭제가 실패하게 한다.
    fs.mkdirSync(`${file}.3`);
    fs.writeFileSync(path.join(`${file}.3`, 'blocker'), 'x');
    const failures = [];
    assert.equal(tryRotate(file, error => failures.push(error)), false);
    assert.equal(failures.length, 1);
    assert.equal(fs.statSync(file).size, 21 * MB);
    assert.equal(fs.existsSync(`${file}.1`), false);
});

test('C-rotation.5: 회전이 성공하면 tryRotate는 true를 돌려주고 실패 알림을 부르지 않는다', () => {
    const { tryRotate } = require('../log-rotation');
    const dir = directory();
    const file = path.join(dir, 'backend.log');
    fs.writeFileSync(file, Buffer.alloc(21 * MB, 'a'));
    assert.equal(tryRotate(file, () => assert.fail('실패 알림이 불리면 안 된다')), true);
});
