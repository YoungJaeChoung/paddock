// 터미널 프로그램의 출력을 화면 연결로 보내기 전에 묶는 버퍼. Theia 기본 버퍼(StringBufferingStream)를 대신 쓴다.
// 기본 버퍼는 모든 출력을 타이머(16ms)가 끝날 때까지 붙잡는다. Windows의 Node 타이머는 15.6ms 단위로 불려 16ms 타이머가 실제로는 약 31ms 뒤에 끝나므로,
// 키 하나의 에코마다 그만큼 늦게 화면에 나타났다. 메모장처럼 친 글자가 바로 보이도록, 조용하던 중의 첫 출력은 바로 보내고 그 뒤 몰려오는 출력만 묶는다.
const { StringBufferingStream } = require('@theia/terminal/lib/node/buffering-stream');

/**
 * 조용하던 중 처음 들어온 출력은 곧바로 보내고, 그 뒤 묶음 간격(`emitInterval`) 동안 들어온 출력은 모아 간격이 끝날 때 한 번에 보내는 버퍼.
 *
 * 키 에코처럼 드문드문 오는 작은 출력은 기다리지 않고, 빌드 로그처럼 쏟아지는 출력은 기본 버퍼처럼 간격마다 묶어 화면 연결의 메시지 수를 그대로 억제한다.
 * 묶음 간격 안에 출력이 더 오지 않으면 간격이 끝날 때 조용한 상태로 돌아간다. 한 번에 보내는 크기는 기본 버퍼와 같이 `maxChunkSize` 이하로 자른다.
 * `flush()`는 남은 출력을 모두 보내고 조용한 상태로 돌아가며, `dispose()` 뒤에는 아무것도 보내지 않는다.
 *
 * Examples
 * --------
 * | 출력 도착 시각(ms) | 간격 16 | 보내는 시각(ms)과 내용      |
 * |--------------------|---------|-----------------------------|
 * | a@0                |         | a@0                         |
 * | a@0, b@40          |         | a@0, b@40                   |
 * | a@0, b@5, c@10     |         | a@0, bc@16                  |
 * | a@0, b@5, c@20     |         | a@0, b@16, c@32             |
 */
class PaddockOutputStream extends StringBufferingStream {
    push(
        chunk,
    ) {
        if (this.timeout === undefined) {
            // 조용한 상태: 묶을 출력이 없으므로 바로 보내고, 뒤따르는 출력을 묶을 간격을 연다.
            this.buffer = chunk;
            this.emitPending();
        } else {
            this.buffer = this.buffer === undefined ? chunk : this.concat(this.buffer, chunk);
        }
    }

    flush() {
        super.flush();
        this.timeout = undefined;
    }

    dispose() {
        super.dispose();
        this.timeout = undefined;
    }

    /** 모아 둔 출력을 `maxChunkSize`만큼 보내고 다음 묶음 간격을 연다. 모아 둔 출력이 없으면 조용한 상태로 돌아간다. */
    emitPending() {
        if (this.buffer === undefined) {
            this.timeout = undefined;
        } else {
            const pending = this.buffer;
            this.buffer = this.length(pending) > this.maxChunkSize ? this.slice(pending, this.maxChunkSize) : undefined;
            this.onDataEmitter.fire(this.slice(pending, 0, this.maxChunkSize));
            this.timeout = setTimeout(() => this.emitPending(), this.emitInterval);
        }
    }
}

module.exports = { PaddockOutputStream };
