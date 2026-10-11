// 설정 화면의 "메모리 안내" 켜기·끄기를 Claude Code 설정에 맞춘다. 훅 파일을 고치는 일은 백엔드(/paddock/memory-notice)가 한다.

class MEMORY_NOTICE {
    static ENABLED = 'paddock.agents.memoryNotice';
    static MINIMUM_MB = 'paddock.agents.memoryNoticeMinimumMb';
    static DEFAULT_MINIMUM_MB = 4096;
}

/**
 * 가용 메모리가 부족할 때 에이전트에게 알리는 훅을 기본 환경과 등록 Claude 계정의 설정에 맞춘다.
 *
 * 기본은 꺼짐이라 사용자가 켜기 전에는 어떤 설정 파일도 건드리지 않는다.
 * 설정을 바꾸는 요청은 하나씩 차례로 보낸다. 같은 설정 파일을 여러 요청이 동시에 고쳐 쓰면 한쪽 변경이 사라진다.
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
     * 기본 환경과 모든 Claude 계정을 지금 설정에 맞춘다. 앱을 시작할 때와 설정을 바꿀 때 부른다.
     * 한 곳이 실패해도 나머지는 계속하고, 실패한 첫 문장을 알림으로 한 번 알린다.
     */
    syncAll() {
        const operation = this.queue.then(async () => {
            const failures = [];
            const apply = async (accountId) => {
                try {
                    await this.post(accountId);
                } catch (error) {
                    failures.push(error instanceof Error ? error.message : String(error));
                }
            };
            await apply(null);
            const profiles = await this.workspace.accounts.list().catch(() => []);
            for (const profile of profiles.filter(item => item.provider === 'claude')) await apply(profile.id);
            if (failures.length && failures[0] !== this.lastWarning) void this.workspace.messages.warn(`Memory notice: ${failures[0]}`);
            this.lastWarning = failures[0] || '';
        });
        this.queue = operation.catch(() => {});
        return operation;
    }

    /**
     * 등록 Claude 계정의 터미널을 띄우기 전에 그 계정 설정에 훅을 넣어 둔다.
     * Claude Code는 시작할 때 읽은 훅만 쓰므로, 주기적으로 맞추면 이미 시작한 대화에는 닿지 않는다.
     * 꺼져 있으면 아무것도 하지 않고, 실패해도 터미널은 그대로 띄우도록 오류를 던지지 않는다.
     */
    async prepareLaunch(
        profile,
    ) {
        if (profile.provider === 'claude' && profile.id && this.isEnabled()) {
            const operation = this.queue.then(() => this.post(profile.id));
            this.queue = operation.catch(() => {});
            await operation.catch(() => {});
        }
    }

    post(
        accountId,
    ) {
        const account = accountId ? `&accountId=${encodeURIComponent(accountId)}` : '';
        return this.workspace.fetchJson('/paddock/memory-notice', 'POST', `?enabled=${this.isEnabled()}&minimumMb=${this.minimumMb()}${account}`);
    }
}

module.exports = { MemoryNotice, MEMORY_NOTICE };
