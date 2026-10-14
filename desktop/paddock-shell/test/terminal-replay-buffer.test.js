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

test('재생을 멈춘 사이 쓰기가 쌓여도 읽지 않은 구간이 덮일 때만 독자를 밀고 중복·역행이 없다', async () => {
    // 링을 채우고 첫 조각을 받은 뒤 멈춘 상태에서 64 KiB를 세 번 쓴다. 독자가 읽은 구간만 덮이다가 읽지 않은 구간이 덮이기 시작한다.
    const { ring, input } = filled(30000);
    let next = 30000;
    let written = '';
    const text = await new Promise(resolve => {
        const stream = ring.getStream();
        const chunks = [];
        let timer;
        let isStalled = false;
        stream.on('data', chunk => {
            chunks.push(chunk);
            if (!isStalled) {
                isStalled = true;
                stream.pause();
                for (let n_bursts = 0; n_bursts < 3; n_bursts += 1) {
                    let burst = '';
                    while (Buffer.byteLength(burst) < 65536) { burst += line(next); next += 1; }
                    written += burst;
                    ring.enq(burst);
                }
                setImmediate(() => stream.resume());
            }
            clearTimeout(timer);
            timer = setTimeout(() => { stream.destroy(); resolve(chunks.join('')); }, 150);
        });
    });
    assertClean(text);
    const numbers = [...text.matchAll(/줄 (\d+) ✅/g)].map(match => Number(match[1]));
    for (let i = 1; i < numbers.length; i += 1) assert.ok(numbers[i] > numbers[i - 1], `줄 번호가 역행하거나 중복됨: ${numbers[i - 1]} → ${numbers[i]}`);
    assert.ok(text.endsWith(line(next - 1)));
    // 재생 길이는 링 크기에 쓰기로 덮인 만큼 더한 값을 넘지 않고, 읽던 구간이 이미 보내진 만큼은 유지된다.
    assert.ok(Buffer.byteLength(text) <= RING_SIZE + Buffer.byteLength(written));
    assert.ok(Buffer.byteLength(text) > RING_SIZE);
});

test('한 번의 쓰기가 링보다 커서 앞이 잘려도 읽던 독자는 글자 경계부터 읽는다', () => {
    const ring = new PaddockReplayBuffer({ size: 10 });
    ring.enq('ab');
    const reader = ring.getReader();
    assert.equal(ring.deq(reader), 'ab');
    // 12바이트 중 마지막 10바이트만 남는데, 시작이 '글'의 중간이다.
    ring.enq('한글다라');
    const text = ring.deq(reader);
    assert.equal(/�/.test(text), false);
    assert.equal(text, '글다라');
});

test('작은 링에 무작위로 쓰고 읽어도 깨진 글자·역행·중복이 없고 재생은 링에 남은 입력의 꼬리와 일치한다', () => {
    let seed = 20261014;
    const random = n => {
        seed = (seed * 1103515245 + 12345) & 0x7fffffff;
        return seed % n;
    };
    const decorations = ['', '한', '✅', '😀', 'é'];
    const n_trials = 3000;
    let n_fffd = 0;
    let n_disorder = 0;
    let n_mismatch = 0;
    for (let trial = 0; trial < n_trials; trial += 1) {
        const size = 24 + random(200);
        const ring = new PaddockReplayBuffer({ size });
        const reader = ring.getReader();
        let n_tokens = 0;
        let input = '';
        const chunks = [];
        for (let step = 0; step < 80; step += 1) {
            let write = '';
            for (let i = 0, n_in_write = 1 + random(6); i < n_in_write; i += 1) {
                write += `[${n_tokens}/${n_tokens}${decorations[random(decorations.length)]}|`;
                n_tokens += 1;
            }
            // 링보다 큰 쓰기는 앞이 잘려 토큰 검사가 어려우니 이 시험에서는 건너뛴다(별도 시험이 있다).
            if (Buffer.byteLength(write) <= size) {
                input += write;
                ring.enq(write);
                // 가끔 몇 조각만 읽고 멈춘다. 읽은 조각은 버리지 않고 모두 모은다.
                if (random(3) === 0) {
                    let chunk = ring.deq(reader, random(2) ? -1 : 1 + random(30));
                    while (chunk !== undefined) {
                        chunks.push(chunk);
                        chunk = random(4) === 0 ? undefined : ring.deq(reader, 1 + random(30));
                    }
                }
            }
        }
        for (let chunk = ring.deq(reader); chunk !== undefined; chunk = ring.deq(reader)) chunks.push(chunk);
        const text = chunks.join('');
        if (/�/.test(text)) n_fffd += 1;
        // 완전한 토큰("[번호/번호…|")만 본다. 읽던 독자가 밀려 생긴 틈 앞뒤의 잘린 토큰이 이어 붙어도 앞뒤 번호가 달라 걸러진다.
        const numbers = [...text.matchAll(/\[(\d+)\/(\d+)[^|\[]*\|/g)].filter(match => match[1] === match[2]).map(match => Number(match[1]));
        if (numbers.some((number, i) => i > 0 && number <= numbers[i - 1])) n_disorder += 1;
        if (!text.endsWith(windowOf(input, size))) n_mismatch += 1;
    }
    assert.deepEqual({ n_fffd, n_disorder, n_mismatch }, { n_fffd: 0, n_disorder: 0, n_mismatch: 0 });
});

test('쓰기 알림: 쓸 때마다 쓴 문자열을 받고, dispose하면 받지 않는다', () => {
    const ring = new PaddockReplayBuffer({ size: RING_SIZE });
    const received = [];
    const listener = ring.addWriteListener(text => received.push(text));
    ring.enq('a');
    ring.enq('한글');
    listener.dispose();
    ring.enq('c');
    assert.deepEqual(received, ['a', '한글']);
});
