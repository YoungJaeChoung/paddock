// 설정의 "에이전트 상태 훅" 켜기·끄기를 Paddock 전용 Claude 설정 파일에 맞춘다. 파일을 고치는 일은 백엔드(/paddock/agent-status-hook)가 한다.

class AGENT_STATUS_HOOK {
    static ENABLED = 'paddock.agents.statusHook';
}

/**
 * 에이전트(Claude Code)의 상태를 터미널 신호로 알리는 훅을 Paddock 전용 Claude 설정 파일에 맞춘다.
 *
 * 메모리 안내(memory-notice.js)와 같은 틀이다. 그 파일은 이 Paddock에 하나뿐이고 기본 환경·등록 Claude 계정의 터미널이 모두 `--settings`로 읽으므로
 * 계정마다 따로 맞출 필요가 없다. 기본은 켬이며 사용자의 Claude 설정 파일은 어느 쪽이든 건드리지 않는다.
 * 설정을 바꾸는 요청은 하나씩 차례로 보낸다. 같은 파일을 여러 요청이 동시에 고쳐 쓰면 한쪽 변경이 사라진다.
 */
class AgentStatusHookSync {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.queue = Promise.resolve();
        // 같은 오류를 설정을 바꿀 때마다 반복해 띄우지 않으려고 마지막으로 알린 문장을 기억한다.
        this.lastWarning = '';
    }

    isEnabled() {
        return this.workspace.preferences.get(AGENT_STATUS_HOOK.ENABLED, true) !== false;
    }

    /** 설정이 바뀐 이름이 에이전트 상태 훅 설정인지. */
    isPreference(
        preferenceName,
    ) {
        return preferenceName === AGENT_STATUS_HOOK.ENABLED;
    }

    /**
     * 지금 설정을 Paddock 전용 설정 파일에 맞춘다. 앱을 시작할 때와 설정을 바꿀 때 부른다.
     * 실패하면 첫 문장을 알림으로 한 번 알린다.
     */
    syncAll() {
        const operation = this.queue.then(async () => {
            let failure = '';
            try {
                await this.workspace.fetchJson('/paddock/agent-status-hook', 'POST', `?enabled=${this.isEnabled()}`);
            } catch (error) {
                failure = error instanceof Error ? error.message : String(error);
            }
            if (failure && failure !== this.lastWarning) void this.workspace.messages.warn(`Agent status hook: ${failure}`);
            this.lastWarning = failure;
        });
        this.queue = operation.catch(() => {});
        return operation;
    }
}

module.exports = { AgentStatusHookSync, AGENT_STATUS_HOOK };
