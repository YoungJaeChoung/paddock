// 설정 화면의 "메모리 안내" 켜기·끄기를 Paddock 전용 Claude 설정 파일에 맞춘다. 파일을 고치는 일은 백엔드(/paddock/memory-notice)가 한다.

class MEMORY_NOTICE {
    static ENABLED = 'paddock.agents.memoryNotice';
    static MINIMUM_MB = 'paddock.agents.memoryNoticeMinimumMb';
    static DEFAULT_MINIMUM_MB = 4096;
}

/**
 * 가용 메모리가 부족할 때 에이전트에게 알리는 훅을 Paddock 전용 Claude 설정 파일에 맞춘다.
 *
 * 그 파일은 이 Paddock에 하나뿐이고 기본 환경·등록 Claude 계정의 터미널이 모두 `--settings`로 읽으므로, 계정마다 따로 맞출 필요가 없다.
 * 기본은 꺼짐이고, 사용자의 Claude 설정 파일은 어느 쪽이든 건드리지 않는다.
 * 설정을 바꾸는 요청은 하나씩 차례로 보낸다. 같은 파일을 여러 요청이 동시에 고쳐 쓰면 한쪽 변경이 사라진다.
 */
class MemoryNotice {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.queue = Promise.resolve();
        // 같은 오류를 설정을 바꿀 때마다 반복해 띄우지 않으려고 마지막으로 알린 문장을 기억한다.
        this.lastWarning = '';
    }

    isEnabled() {
        return this.workspace.preferences.get(MEMORY_NOTICE.ENABLED, false) === true;
    }

    /** 기준값(MB). 숫자가 아니거나 음수면 기본값이다. */
    minimumMb() {
        const value = Number(this.workspace.preferences.get(MEMORY_NOTICE.MINIMUM_MB, MEMORY_NOTICE.DEFAULT_MINIMUM_MB));
        return Number.isFinite(value) && value >= 0 ? Math.floor(value) : MEMORY_NOTICE.DEFAULT_MINIMUM_MB;
    }

    /** 설정이 바뀐 이름이 메모리 안내 설정인지. */
    isPreference(
        preferenceName,
    ) {
        return preferenceName === MEMORY_NOTICE.ENABLED || preferenceName === MEMORY_NOTICE.MINIMUM_MB;
    }

    /**
     * 지금 설정을 Paddock 전용 설정 파일에 맞춘다. 앱을 시작할 때와 설정을 바꿀 때 부른다.
     * 실패하면 첫 문장을 알림으로 한 번 알린다.
     */
    syncAll() {
        const operation = this.queue.then(async () => {
            let failure = '';
            try {
                await this.post();
            } catch (error) {
                failure = error instanceof Error ? error.message : String(error);
            }
            if (failure && failure !== this.lastWarning) void this.workspace.messages.warn(`Memory notice: ${failure}`);
            this.lastWarning = failure;
        });
        this.queue = operation.catch(() => {});
        return operation;
    }

    post() {
        return this.workspace.fetchJson('/paddock/memory-notice', 'POST', `?enabled=${this.isEnabled()}&minimumMb=${this.minimumMb()}`);
    }
}

module.exports = { MemoryNotice, MEMORY_NOTICE };
