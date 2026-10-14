// 터미널 지난 출력을 담는 링 버퍼. 화면이 다시 붙을 때 이 버퍼의 내용을 처음부터 다시 보내 지난 화면을 복원한다.
// Theia 기본 링 버퍼(MultiRingBuffer)는 링이 한 바퀴 돈 뒤의 재생이 중복되고 한글이 깨져 xterm이 "Parsing error"를 내므로, 읽기만 고쳐 대신 쓴다.
const { StringDecoder } = require('node:string_decoder');
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');
const { MultiRingBuffer, MultiRingBufferOptions } = require('@theia/process/lib/node/multi-ring-buffer');

/** UTF-8에서 글자의 앞 바이트가 아니라 이어지는 바이트(10xxxxxx)인지. */
function isContinuationByte(
    byte,
) {
    return (byte & 0xc0) === 0x80;
}

/**
 * 독자마다 이어서 읽고 있는지 추적하며, 글자가 조각 경계에 걸려도 온전히 잇는 링 버퍼.
 *
 * 기본 구현의 `deq`는 두 가지가 틀린다. 첫째, 읽는 구간이 링 끝을 넘으면 요청한 크기를 무시하고 링 전체를 읽는데
 * 독자 위치는 요청한 크기만큼만 나아가므로 같은 구간이 다시 읽혀 재생이 중복된다. 둘째, 조각마다 따로 문자열로 바꿔
 * 조각 경계에 걸친 3바이트 한글이 깨진 글자(U+FFFD)가 된다. 셋째, 쓰기가 독자의 읽지 않은 구간을 덮었는지를
 * 쓰기가 끝난 뒤의 앞 위치로 판정해 한 쓰기만큼 어긋나므로, 독자가 새 데이터 중간에서 이어 읽거나 이미 보낸 구간을 다시 읽는다.
 * 이 클래스는 `deq`와 쓰기가 독자를 미는 규칙(`incTails`)을 바꾸고, 쓰기 자체(`enq`의 복사)는 그대로 둔다.
 *
 * 독자가 처음 읽거나 쓰기가 독자의 읽지 않은 구간을 덮어 독자를 새 시작 위치로 밀었을 때(또는 한 번의 쓰기가 링보다 커서
 * 앞이 잘렸을 때)는 이어 읽는 것이 아니고 글자 중간에서 시작했을 수 있다. 그 조각 앞쪽의 이어지는 바이트를 버리고
 * 읽던 글자 상태도 버려 글자 경계부터 읽는다. 그 위치가 새 글자의 시작이면 버릴 것이 없어 손실도 없다.
 *
 * Examples
 * --------
 * | 상황                                           | 읽는 결과                                  |
 * |------------------------------------------------|--------------------------------------------|
 * | 링 1 MiB에 1.5 MB를 쓰고 처음부터 읽음         | 마지막 1 MiB 이하 한 번씩, 중복 없음       |
 * | 한글 3바이트가 두 조각에 나뉨                  | 첫 조각엔 앞 글자까지, 다음 조각에 그 글자 |
 * | 쓰기가 독자의 읽지 않은 구간을 덮음            | 새 시작 위치의 다음 글자 경계부터          |
 * | 쓰기가 독자가 이미 읽은 구간만 덮음            | 독자 위치 그대로 이어 읽음                 |
 */
class PaddockReplayBuffer extends MultiRingBuffer {
    constructor(
        options,
    ) {
        super(options);
        // 독자 번호별 읽기 상태: { decoder, expectedPosition, isAligned }.
        // expectedPosition은 마지막으로 읽은 바로 다음 위치(모르면 null), isAligned는 글자 경계를 찾았는지다.
        this.readerStates = new Map();
        // 진행 중인 쓰기가 링보다 커서 앞이 잘렸는지. 잘린 쓰기는 글자 중간에서 시작할 수 있다.
        this.isTruncatingWrite = false;
    }

    enq(
        str,
        encoding = 'utf8',
    ) {
        this.isTruncatingWrite = Buffer.byteLength(str, encoding) > this.maxSize;
        super.enq(str, encoding);
        this.isTruncatingWrite = false;
    }

    /**
     * 쓰기 `size`바이트가 링의 가장 오래된 바이트를 덮는 만큼 시작 위치(tail)를 옮기고, 읽지 않은 구간이 덮인 독자를 새 시작 위치로 민다.
     * 덮이지 않은 독자는 그대로 둔다. 이 호출은 쓰기가 반영되기 전(머리 위치 갱신 전)에 일어난다.
     *
     * Examples
     * --------
     * | 링 크기 | 쓰기 전 담긴 양 | 쓰기 | 덮인 바이트 | 독자(시작에서 떨어진 거리) | 결과                  |
     * |---------|-----------------|------|-------------|----------------------------|-----------------------|
     * | 10      | 6               | 3    | 0           | 2                          | 그대로                |
     * | 10      | 10              | 4    | 4           | 3                          | 새 시작 위치로 밀림   |
     * | 10      | 10              | 4    | 4           | 4                          | 그대로(읽은 구간만 덮임) |
     */
    incTails(
        size,
    ) {
        const used = this.sizeFrom(this.tail, this.head, this.isWrapped(this.tail, this.head));
        const n_covered = Math.max(0, used + size - this.maxSize);
        if (n_covered > 0) {
            const oldTail = this.tail;
            this.tail = (oldTail + n_covered) % this.maxSize;
            for (const [id, pos] of this.readers) {
                if (pos !== -1 && (pos - oldTail + this.maxSize) % this.maxSize < n_covered) {
                    this.readers.set(id, this.tail);
                    this.invalidate(id);
                }
            }
        }
    }

    /** 독자 `id`가 마지막으로 읽은 곳에서 이어 읽는 것이 아니게 됐음을 표시한다. 다음 읽기는 글자 경계부터 시작한다. */
    invalidate(
        id,
    ) {
        const state = this.readerStates.get(id);
        if (state !== undefined) state.expectedPosition = null;
    }

    onData(
        start,
    ) {
        // 앞이 잘린 쓰기는 시작이 글자 중간일 수 있어, 다 읽고 기다리던 독자도 이어 읽는 것으로 보지 않는다.
        if (this.isTruncatingWrite) {
            for (const id of this.readerStates.keys()) this.invalidate(id);
        }
        super.onData(start);
    }

    closeReader(
        id,
    ) {
        this.readerStates.delete(id);
        super.closeReader(id);
    }

    /**
     * 독자 `id`가 읽을 다음 내용을 문자열로 돌려준다. 읽을 것이 없으면 `undefined`다.
     * 조각 끝에 글자가 반쯤 걸리면 그 바이트는 다음 호출로 미루므로 빈 문자열이 돌아올 수 있다(독자 위치는 나아간다).
     *
     * Examples
     * --------
     * | size       | 독자 앞에 남은 바이트 | 결과                                   |
     * |------------|-----------------------|----------------------------------------|
     * | `-1`       | 100                   | 100바이트 분량의 문자열                |
     * | `16384`    | 100                   | 100바이트 분량의 문자열                |
     * | `10`       | 100                   | 10바이트 분량(끝 글자가 반이면 앞까지) |
     * | `0` · 다 읽음 · 닫힌 독자 | —          | `undefined`                            |
     */
    deq(
        id,
        size = -1,
        encoding = 'utf8',
    ) {
        const pos = this.readers.get(id);
        let result;
        if (pos !== undefined && pos !== -1 && size !== 0) {
            const maxDeqSize = this.sizeForReader(id);
            const deqSize = size === -1 ? maxDeqSize : Math.min(size, maxDeqSize);
            // 링 끝을 넘으면 앞쪽에서 이어 읽되, 요청한 크기까지만 읽는다.
            const firstEnd = Math.min(pos + deqSize, this.maxSize);
            const first = this.buffer.subarray(pos, firstEnd);
            const second = this.buffer.subarray(0, pos + deqSize - firstEnd);
            let bytes = second.length > 0 ? Buffer.concat([first, second]) : first;

            let state = this.readerStates.get(id);
            if (state === undefined) {
                state = { decoder: new StringDecoder(encoding), expectedPosition: null, isAligned: false };
                this.readerStates.set(id, state);
            }
            if (state.expectedPosition !== pos) {
                // 처음 읽거나 쓰기가 독자 위치를 밀었다. 반쯤 걸린 글자는 버리고 글자 경계부터 다시 읽는다.
                state.decoder = new StringDecoder(encoding);
                state.isAligned = false;
            }
            if (!state.isAligned && /^utf-?8$/i.test(encoding)) {
                let n_skipped = 0;
                while (n_skipped < bytes.length && isContinuationByte(bytes[n_skipped])) n_skipped += 1;
                bytes = bytes.subarray(n_skipped);
                state.isAligned = bytes.length > 0;
            } else {
                state.isAligned = true;
            }
            result = state.decoder.write(bytes);

            const lastIndex = this.inc(pos, deqSize - 1).newPos;
            if (lastIndex === this.head) {
                // 다 읽었다. 다음 쓰기가 시작하는 위치에서 이어지므로 기대 위치는 그 위치다.
                this.readers.set(id, -1);
                state.expectedPosition = this.inc(this.head, 1).newPos;
            } else {
                const next = this.inc(pos, deqSize).newPos;
                this.readers.set(id, next);
                state.expectedPosition = next;
            }
        }
        return result;
    }
}
decorate(injectable(), PaddockReplayBuffer);
decorate(inject(MultiRingBufferOptions), PaddockReplayBuffer, 0);

module.exports = { PaddockReplayBuffer };
