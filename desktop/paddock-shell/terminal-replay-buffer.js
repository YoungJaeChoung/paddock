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
 * 조각 경계에 걸친 3바이트 한글이 깨진 글자(U+FFFD)가 된다. 이 클래스는 `deq`만 바꾸고 쓰기(`enq`)·위치 계산은 그대로 둔다.
 *
 * 독자 위치가 우리가 마지막으로 읽은 곳 다음이 아니면(처음 읽을 때, 또는 쓰기가 독자를 앞질러 위치를 밀었을 때)
 * 글자 중간에서 시작했을 수 있으므로, 그 조각 앞쪽의 이어지는 바이트를 버리고 시작해 글자 경계에 맞춘다.
 * 이때 그 위치가 새 글자의 시작이면 버릴 것이 없어 손실도 없다.
 *
 * Examples
 * --------
 * | 상황                                           | 읽는 결과                                  |
 * |------------------------------------------------|--------------------------------------------|
 * | 링 1 MiB에 1.5 MB를 쓰고 처음부터 읽음         | 마지막 1 MiB 이하 한 번씩, 중복 없음       |
 * | 한글 3바이트가 두 조각에 나뉨                  | 첫 조각엔 앞 글자까지, 다음 조각에 그 글자 |
 * | 쓰기가 독자를 앞질러 글자 중간에서 시작        | 이어지는 바이트를 건너뛰고 다음 글자부터   |
 */
class PaddockReplayBuffer extends MultiRingBuffer {
    constructor(
        options,
    ) {
        super(options);
        // 독자 번호별 읽기 상태: { decoder, expectedPosition, isAligned }.
        // expectedPosition은 마지막으로 읽은 바로 다음 위치(모르면 null), isAligned는 글자 경계를 찾았는지다.
        this.readerStates = new Map();
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
