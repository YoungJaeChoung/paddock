const test = require('node:test');
const assert = require('node:assert/strict');
const { PaddockReplayBuffer } = require('../terminal-replay-buffer');

const RING_SIZE = 1048576;
/** CSI 파라미터 자리에 ASCII가 아닌 글자가 들어간 것 — xterm 파서가 "Parsing error"를 내는 모양. */
const BROKEN_CSI = /\x1b\[[0-9;:?]*[^\x20-\x7e]/g;
const line = index => `\x1b[38;2;117;113;94m한글 출력 줄 ${index} ✅\x1b[0m\r\n`;

/** `lines`개를 쓴 입력 전체 문자열과 그 줄들을 쓴 버퍼를 만든다. */
function filled(
    n_lines,
) {
    const ring = new PaddockReplayBuffer({ size: RING_SIZE });
    let input = '';
    for (let i = 0; i < n_lines; i += 1) {
        input += line(i);
        ring.enq(line(i));
    }
    return { ring, input };
}

/** 스트림의 내용이 잠시 끊기면 그때까지 받은 문자열을 돌려준다. `duringReplay`는 첫 조각을 받은 직후 실행한다. */
function replay(
    ring,
    duringReplay = () => {},
) {
    return new Promise(resolve => {
        const stream = ring.getStream();
        const chunks = [];
        let timer;
        stream.on('data', chunk => {
            if (chunks.length === 0) duringReplay();
            chunks.push(chunk);
            clearTimeout(timer);
            timer = setTimeout(() => { stream.destroy(); resolve(chunks.join('')); }, 150);
        });
    });
}

/** 입력의 마지막 `size`바이트에서 앞쪽의 잘린 글자를 버린 문자열. 링이 한 바퀴 돈 뒤 재생이 가져야 할 내용이다. */
function windowOf(
    input,
    size,
) {
    let bytes = Buffer.from(input).subarray(Math.max(0, Buffer.byteLength(input) - size));
    let n_skipped = 0;
    while (n_skipped < bytes.length && (bytes[n_skipped] & 0xc0) === 0x80) n_skipped += 1;
    return bytes.subarray(n_skipped).toString('utf8');
}

function assertClean(
    text,
) {
    assert.equal((text.match(/�/g) || []).length, 0, '깨진 글자(U+FFFD)가 없어야 한다');
    assert.equal([...text.matchAll(BROKEN_CSI)].length, 0, '끊긴 CSI가 없어야 한다');
}

test('링이 한 바퀴 돌지 않았으면 조용한 재생은 입력 전체와 같다', async () => {
    const { ring, input } = filled(9000);
    assert.ok(Buffer.byteLength(input) < RING_SIZE);
    const text = await replay(ring);
    assertClean(text);
    assert.equal(text, input);
});

test('링이 한 바퀴 돈 뒤 조용한 재생은 마지막 1 MiB 이하 한 번뿐이고 중복이 없다', async () => {
    const { ring, input } = filled(30000);
    assert.ok(Buffer.byteLength(input) > RING_SIZE);
    const text = await replay(ring);
    assertClean(text);
    assert.equal(text, windowOf(input, RING_SIZE));
    assert.ok(Buffer.byteLength(text) <= RING_SIZE);
});

test('링이 한 바퀴 돌지 않았을 때 재생 중에 온 출력은 입력 뒤에 이어진다', async () => {
    const { ring, input } = filled(9000);
    const extra = line('extra-a') + line('extra-b');
    const text = await replay(ring, () => { ring.enq(extra); });
    assertClean(text);
    assert.equal(text, input + extra);
});

test('링이 한 바퀴 돈 뒤 재생 중에 온 출력은 재생 끝에 이어지고 중복이 없다', async () => {
    const { ring, input } = filled(30000);
    const extra = line('extra-a') + line('extra-b');
    const text = await replay(ring, () => { ring.enq(extra); });
    assertClean(text);
    assert.equal(text, windowOf(input, RING_SIZE) + extra);
});

test('조각 경계에 걸친 한글은 깨지지 않고 이어진다', () => {
    const ring = new PaddockReplayBuffer({ size: 64 });
    ring.enq('가나다라마');
    const reader = ring.getReader();
    // 15바이트를 4바이트씩 읽으면 모든 조각 경계가 글자 중간에 걸린다.
    let text = '';
    for (let chunk = ring.deq(reader, 4); chunk !== undefined; chunk = ring.deq(reader, 4)) text += chunk;
    assert.equal(text, '가나다라마');
});

test('쓰기가 독자 위치를 글자 중간으로 밀면 다음 글자부터 읽는다', () => {
    const ring = new PaddockReplayBuffer({ size: 10 });
    ring.enq('가나다');
    const reader = ring.getReader();
    // 9바이트 뒤 4바이트를 더 쓰면 가장 오래된 4바이트가 밀려나 독자 위치가 '나'의 중간(1바이트 뒤)이 된다.
    ring.enq('ab가');
    const text = ring.deq(reader);
    assert.equal(/�/.test(text), false);
    assert.ok('가나다ab가'.endsWith(text));
});

test('closeReader는 독자의 읽기 상태를 지운다', () => {
    const ring = new PaddockReplayBuffer({ size: 64 });
    ring.enq('abc');
    const reader = ring.getReader();
    ring.deq(reader);
    assert.equal(ring.readerStates.has(reader), true);
    ring.closeReader(reader);
    assert.equal(ring.readerStates.has(reader), false);
});
