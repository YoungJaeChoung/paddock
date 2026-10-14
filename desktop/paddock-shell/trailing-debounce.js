// 자주 오는 신호를 모아 한 번만 실행하는 지연 실행기. 신호가 멎은 뒤 일정 시간이 지나면 실행하되, 신호가 계속 와도 첫 신호로부터 최대 대기 시간 안에는 실행한다.
// 터미널 화면 저장과 배치 저장이 같은 규칙을 쓴다. 시간 함수를 받아 시험에서 시계를 바꿔 끼운다.

/**
 * 신호(`touch`)가 오면 실행을 미룬다. 마지막 신호로부터 `quietMs`가 지나거나 첫 신호로부터 `maxWaitMs`가 지나면 `run`을 한 번 실행한다.
 *
 * Examples
 * --------
 * | 신호 시각(s)               | quiet 2 · max 10 | `run` 시각(s) |
 * |----------------------------|------------------|---------------|
 * | 0                          |                  | 2             |
 * | 0, 1, 2, … 매초 계속       |                  | 10            |
 * | 0, 5                       |                  | 2, 7          |
 */
class TrailingDebounce {
    constructor(
        {
            quietMs,
            maxWaitMs,
            run,
            timers = { setTimeout: (callback, delay) => setTimeout(callback, delay), clearTimeout: id => clearTimeout(id) },
            now = Date.now,
        },
    ) {
        this.quietMs = quietMs;
        this.maxWaitMs = maxWaitMs;
        this.run = run;
        this.timers = timers;
        this.now = now;
        this.timer = null;
        // 실행을 기다리는 첫 신호의 시각. 기다리는 신호가 없으면 null.
        this.pendingSince = null;
    }

    touch() {
        const now = this.now();
        if (this.pendingSince === null) this.pendingSince = now;
        if (this.timer !== null) this.timers.clearTimeout(this.timer);
        const delay = Math.max(0, Math.min(this.quietMs, this.pendingSince + this.maxWaitMs - now));
        this.timer = this.timers.setTimeout(() => {
            this.timer = null;
            this.pendingSince = null;
            this.run();
        }, delay);
    }

    /** 기다리는 실행을 취소한다. 실행할 것이 없으면 아무 일도 하지 않는다. */
    cancel() {
        if (this.timer !== null) this.timers.clearTimeout(this.timer);
        this.timer = null;
        this.pendingSince = null;
    }
}

module.exports = { TrailingDebounce };
