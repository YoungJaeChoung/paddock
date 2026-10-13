const { AbstractDialog } = require('@theia/core/lib/browser/dialogs');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { PreferenceService } = require('@theia/core/lib/common/preferences/preference-service');
const { PreferenceScope } = require('@theia/core/lib/common/preferences/preference-scope');
const { EnvVariablesServer } = require('@theia/core/lib/common/env-variables');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');
const { URI } = require('@theia/core/lib/common/uri');
const { StopReason } = require('@theia/core/lib/common/frontend-application-state');
const { QUIT_CHOICE_FILE, QUIT_CHOICE, formatQuitChoice } = require('./backend-session');
const { PaddockWorkspace } = require('./workspace');

/** 종료 설정 키. 메인 프로세스(quit-preference.js)와 같은 키를 쓴다. */
const ON_QUIT_KEY = 'paddock.backend.onQuit';

/**
 * 남은 터미널을 어떻게 할지 고르는 창. 값은 `{ choice, remember }`이고, 취소하면 `undefined`다.
 * `choice`는 `QUIT_CHOICE.KEEP`·`QUIT_CHOICE.STOP`, `remember`는 "다시 묻지 않기" 선택이다.
 */
class QuitDialog extends AbstractDialog {
    constructor(
        n_terminals,
    ) {
        super({ title: 'Quit Paddock', maxWidth: 580 });
        this.chosen = undefined;
        const document = this.node.ownerDocument;
        const message = document.createElement('p');
        message.textContent = `${n_terminals === 1 ? '1 terminal is' : `${n_terminals} terminals are`} still running.`;
        const detail = document.createElement('p');
        detail.textContent = 'Keep them running to pick up where you left off the next time you open Paddock. You can change this later in Settings under "Backend: On Quit" (paddock.backend.onQuit).';
        const remember = document.createElement('label');
        this.rememberBox = document.createElement('input');
        this.rememberBox.type = 'checkbox';
        remember.append(this.rememberBox, " Don't ask again");
        this.contentNode.append(message, detail, remember);
        this.appendCloseButton('Cancel');
        const choose = (choice) => {
            this.chosen = { choice, remember: this.rememberBox.checked };
            void this.accept();
        };
        this.stopButton = this.appendButton('Stop all and quit', false);
        this.stopButton.addEventListener('click', () => choose(QUIT_CHOICE.STOP));
        this.keepButton = this.appendButton('Keep running in background', true);
        this.keepButton.addEventListener('click', () => choose(QUIT_CHOICE.KEEP));
    }

    get value() {
        return this.chosen;
    }

    handleEnter(
        event,
    ) {
        // Enter는 기본 선택(계속 실행)이다. 터미널을 끝내는 선택은 버튼을 직접 눌러야 한다.
        if (!this.chosen) this.chosen = { choice: QUIT_CHOICE.KEEP, remember: this.rememberBox.checked };
        return super.handleEnter(event);
    }

    onActivateRequest(
        message,
    ) {
        super.onActivateRequest(message);
        this.keepButton.focus();
    }
}

/**
 * 마지막 창을 닫을 때, 창이 아직 보이는 동안 창 안에서 남은 터미널을 계속 실행할지 묻는다.
 *
 * 메인 프로세스는 창이 모두 닫힌 뒤에야 종료를 확정하므로, 거기서 물으면 소속 창 없는 질문 창이 화면 구석·다른 창 뒤에 떠
 * 놓치기 쉽다. 놓치면 앱이 창 없이 남고, 그사이 다시 실행하면 그 앱에 새 창이 붙었다가 늦게 답할 때 함께 닫힌다.
 * 여기서 받은 답은 종료 선택 표지(backend-session.js)로 남겨 메인이 다시 묻지 않게 한다.
 *
 * 다른 창이 떠 있으면 묻지 않는다. 이 창의 터미널은 백엔드에 남아 다른 창의 Work 목록에 background로 보인다.
 * 새로고침, 설정이 ask가 아닐 때, 남은 터미널이 없을 때도 묻지 않는다. 터미널 수를 읽지 못하면 묻지 않고 메인에 맡긴다.
 */
class PaddockQuitPrompt {
    constructor(
        container,
    ) {
        this.container = container;
    }

    onWillStop() {
        // 다른 닫기 확인(저장 안 한 편집 등)이 모두 통과한 뒤 마지막에 묻는다. 여기서 답한 뒤 닫기가 취소되지 않게 하기 위해서다.
        return { reason: 'Running terminals', priority: 100, action: (prepared, reason) => this.confirm(reason) };
    }

    /** 닫아도 되면 true, 사용자가 취소하면 false다. */
    async confirm(
        reason,
    ) {
        let isAllowed = true;
        const preferences = this.container.get(PreferenceService);
        const workspace = this.container.get(PaddockWorkspace);
        const isLastWindow = !workspace.remoteWorkSnapshots.some(snapshot => !snapshot.retired);
        const isAsked = reason !== StopReason.Reload && isLastWindow && preferences.get(ON_QUIT_KEY, 'ask') === 'ask';
        const n_terminals = isAsked ? await this.countTerminals() : 0;
        if (n_terminals > 0) {
            const answer = await new QuitDialog(n_terminals).open();
            if (answer) {
                if (answer.remember) await preferences.set(ON_QUIT_KEY, answer.choice, PreferenceScope.User);
                const directory = new URI(await this.container.get(EnvVariablesServer).getConfigDirUri());
                await this.container.get(FileService).write(directory.resolve(QUIT_CHOICE_FILE), formatQuitChoice(answer.choice, Date.now()));
            } else {
                isAllowed = false;
            }
        }
        return isAllowed;
    }

    /** 백엔드에 열려 있는 터미널 수. 읽지 못하면 0이다(묻지 않고 메인 프로세스에 맡긴다). */
    async countTerminals() {
        let n_terminals = 0;
        try {
            const response = await fetch(new Endpoint({ path: '/paddock/session' }).getRestUrl().toString(), { signal: AbortSignal.timeout(3000) });
            if (response.ok) n_terminals = Number((await response.json()).terminals) || 0;
        } catch {
            n_terminals = 0;
        }
        return n_terminals;
    }
}

module.exports = { PaddockQuitPrompt };
