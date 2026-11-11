// 확장이 띄우는 오류 모달을 화면을 막지 않는 알림으로 바꾼다.
// 확장(예: 내장 git)은 명령이 실패하면 `showErrorMessage(…, { modal: true })`로 화면 전체를 막는 대화상자를 띄운다.
// 백그라운드에서 도는 git 새로 고침이 실패해도 입력 중인 터미널을 가로막아, 닫기 버튼과 창 닫기 버튼을 혼동하기 쉽다.
const { MessageService } = require('@theia/core/lib/common/message-service');
const { ILogger } = require('@theia/core/lib/common/logger');
const { MainMessageType } = require('@theia/plugin-ext/lib/common/plugin-api-rpc');
const { coalesceMessage, messageKey } = require('./message-coalesce');
const { MessageRegistryMainImpl } = require('@theia/plugin-ext/lib/main/browser/message-registry-main');
const { BackgroundGitRefresh } = require('./background-git-refresh');

/**
 * 확장의 오류 모달만 알림으로 바꾸는 메시지 등록기.
 *
 * 경고·안내 모달은 "변경을 버릴까요?" 같은 확인이라 알림으로 바꾸면 놓치기 쉬워 기본 구현 그대로 둔다.
 * 바꾼 오류는 문구를 로그(backend.log)에 남긴다. 사용자가 알림을 놓쳐도 나중에 어떤 확장이 무엇을 알렸는지 볼 수 있다.
 * Paddock의 백그라운드 Git 다시 읽기가 같은 오류로 거듭 실패하면 첫 알림만 띄우고 반복은 띄우지도 로그에 남기지도 않는다
 * (background-git-refresh.js). 반복 횟수는 다시 읽기가 성공할 때 한 줄로 남는다.
 */
class PaddockMessageRegistryMain extends MessageRegistryMainImpl {
    constructor(
        container,
    ) {
        super(container);
        this.logger = container.get(ILogger);
        this.container = container;
        this.backgroundGitRefresh = container.get(BackgroundGitRefresh);
        this.pendingErrors = new Map();
    }

    async doShowMessage(
        type,
        message,
        options,
        actions,
    ) {
        let selected;
        const isErrorModal = options.modal && type === MainMessageType.Error;
        const text = options.detail ? `${message}\n${options.detail}` : message;
        const kind = isErrorModal ? this.backgroundGitRefresh.classify(text) : null;
        if (kind === 'repeat') {
            // 같은 백그라운드 실패의 반복: 띄우지 않고 닫힌 것으로 답한다.
            selected = undefined;
        } else if (isErrorModal) {
            const note = kind === 'first' ? ' (background Git refresh; repeats are hidden until a refresh succeeds)' : '';
            this.logger.warn(`[plugin] ${text}${note}`);
            const titles = actions.map(action => action.title);
            selected = await coalesceMessage(
                this.pendingErrors,
                messageKey(text, titles),
                () => this.container.get(MessageService).error(text, ...titles),
            );
        } else {
            selected = await super.doShowMessage(type, message, options, actions);
        }
        return selected;
    }
}

module.exports = { PaddockMessageRegistryMain };
