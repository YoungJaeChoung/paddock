const { AbstractDialog } = require('@theia/core/lib/browser/dialogs');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { OS } = require('@theia/core/lib/common/os');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { PreferenceService } = require('@theia/core/lib/common/preferences/preference-service');
const { PreferenceScope } = require('@theia/core/lib/common/preferences/preference-scope');
const { WSL_LAUNCHER_KEY, wslLauncherPlan } = require('./wsl-launcher-plan');

/** WSL ~/.bashrc에 줄을 넣어도 되는지 묻는 창. 값은 'add'·'decline'이고, 닫으면 `undefined`(다음에 다시 묻는다)다. */
class WslLauncherDialog extends AbstractDialog {
    constructor() {
        super({ title: 'Show Claude usage in WSL terminals too?', maxWidth: 520 });
        this.chosen = undefined;
        const document = this.node.ownerDocument;
        const message = document.createElement('p');
        // 묻는 것은 얻는 것(사용량 표시·메모리 경고)과 바뀌는 것(.bashrc 한 줄)뿐이다. 줄의 표지·설정 키 이름은 설정 화면 설명에 둔다.
        message.textContent = 'When you run claude in a regular WSL terminal, Paddock can show your usage and tell Claude when memory runs low. This adds one line to your ~/.bashrc. You can undo it in Settings.';
        this.contentNode.append(message);
        this.appendCloseButton('Ask me later');
        this.declineButton = this.appendButton('No thanks', false);
        this.declineButton.addEventListener('click', () => {
            this.chosen = 'decline';
            void this.accept();
        });
        this.addButton = this.appendButton('Turn on', true);
        this.addButton.addEventListener('click', () => {
            this.chosen = 'add';
            void this.accept();
        });
    }

    get value() {
        return this.chosen;
    }

    onActivateRequest(
        message,
    ) {
        super.onActivateRequest(message);
        // 사용자 파일을 고치는 선택이 Enter 한 번으로 되지 않게, 처음 초점은 "나중에" 버튼에 둔다.
        this.closeButton.focus();
    }
}

/**
 * 앱이 시작된 뒤 사용자에게 한 번 알리거나 물을 일들을 처리한다.
 *   - 예전 판이 사용자 Claude 설정 파일에 넣어 둔 Paddock 항목을 걷어 냈다면 무엇을 지웠는지 알린다(백엔드가 한 번 걷어 내고 결과를 적어 둔다).
 *   - Windows 앱에서 WSL이 있으면, WSL 일반 터미널의 claude에도 Paddock 전용 설정이 붙도록 ~/.bashrc에 PATH 한 줄을 넣어도 되는지 한 번 묻는다.
 *     답은 설정(paddock.wsl.claudeLauncher)에 남고, 설정을 바꾸면 줄을 넣거나 뺀다.
 */
class PaddockSetupNotices {
    constructor(
        container,
    ) {
        this.container = container;
    }

    onDidInitializeLayout() {
        void this.announceStartupWarnings();
        void this.announceLegacyCleanup();
        void this.setUpWslLauncher();
    }

    endpoint(
        path,
        query = '',
    ) {
        return `${new Endpoint({ path }).getRestUrl().toString()}${query}`;
    }

    /** 백엔드가 시작할 때 단계별로 실패한 일(설정 파일 쓰기·래퍼 설치·예전 항목 정리)을 경고로 보인다. 터미널은 그대로 열린다. */
    async announceStartupWarnings() {
        try {
            const response = await fetch(this.endpoint('/paddock/startup-warnings'));
            const { warnings } = response.ok ? await response.json() : { warnings: [] };
            if (Array.isArray(warnings)) for (const warning of warnings) void this.container.get(MessageService).warn(warning);
        } catch {
            // 경고를 못 가져와도 앱 사용에는 영향이 없다.
        }
    }

    async announceLegacyCleanup() {
        try {
            const response = await fetch(this.endpoint('/paddock/legacy-cleanup'));
            const { notice } = response.ok ? await response.json() : { notice: [] };
            if (Array.isArray(notice) && notice.length) {
                const messages = this.container.get(MessageService);
                // 확인할 때까지 닫히지 않는 알림으로 보이고, 확인하면 다시 알리지 않게 백엔드에 알린다.
                await messages.info(notice.join('\n'), 'OK');
                await fetch(this.endpoint('/paddock/legacy-cleanup/ack'), { method: 'POST' });
            }
        } catch {
            // 알림을 못 보여도 앱 사용에는 영향이 없다. 다음 시작에 다시 시도한다.
        }
    }

    async isWslReady() {
        let isReady = false;
        try {
            const response = await fetch(this.endpoint('/paddock/wsl-ready'), { signal: AbortSignal.timeout(30000) });
            isReady = response.ok && (await response.json()).ready === true;
        } catch {
            isReady = false;
        }
        return isReady;
    }

    /**
     * 백엔드에 WSL ~/.bashrc의 PATH 줄을 넣거나 빼라고 한다. 사용자가 방금 고른 일이라 성공은 알리지 않는다.
     * 알리는 것은 사용자가 할 일이 있을 때뿐이다 — 실패했을 때, 그리고 .bashrc가 저장소 파일로 가는 링크라 그 저장소에 올려야 할 때.
     */
    async applyWslLauncher(
        isEnabled,
    ) {
        const messages = this.container.get(MessageService);
        try {
            const response = await fetch(this.endpoint('/paddock/wsl-claude-path', `?enabled=${isEnabled}`), { method: 'POST' });
            const result = await response.json();
            if (!response.ok) throw new Error(result.error || 'The WSL ~/.bashrc could not be changed.');
            if (result.isLinked && (result.state === 'added' || result.state === 'removed')) void messages.info(`${result.file} changed. If you keep it in a repository, commit or sync it.`);
            else if (result.state === 'blocked') void messages.warn('Paddock could not change your WSL ~/.bashrc because it is a broken link or not a regular file.');
        } catch (error) {
            void messages.warn(`Paddock could not change your WSL ~/.bashrc: ${error instanceof Error ? error.message : String(error)}`);
        }
    }

    async setUpWslLauncher() {
        const preferences = this.container.get(PreferenceService);
        const isWindows = OS.backend.type() === OS.Type.Windows;
        // 설정을 바꾸면(설정 화면에서 직접 켜거나 끄면) 줄을 넣거나 뺀다. 'ask'로 돌려도 이미 넣은 줄은 그대로 둔다.
        preferences.onPreferenceChanged(({ preferenceName }) => {
            if (isWindows && preferenceName === WSL_LAUNCHER_KEY) {
                const value = preferences.get(WSL_LAUNCHER_KEY, 'ask');
                if (value === 'on') void this.applyWslLauncher(true);
                else if (value === 'off') void this.applyWslLauncher(false);
            }
        });
        const plan = wslLauncherPlan(preferences.get(WSL_LAUNCHER_KEY, 'ask'), { isWindows, isWslReady: isWindows && await this.isWslReady() });
        if (plan === 'prompt') {
            const answer = await new WslLauncherDialog().open();
            if (answer === 'add') await preferences.set(WSL_LAUNCHER_KEY, 'on', PreferenceScope.User);
            else if (answer === 'decline') await preferences.set(WSL_LAUNCHER_KEY, 'off', PreferenceScope.User);
        } else if (plan === 'add') {
            await this.applyWslLauncherQuietly();
        }
    }

    /** 이미 허락한 줄이 .bashrc에 남아 있는지 확인하고, 없으면 조용히 다시 넣는다. 사용자가 이미 켠 기능을 유지하는 일이라 알리지 않는다. */
    async applyWslLauncherQuietly() {
        try {
            await fetch(this.endpoint('/paddock/wsl-claude-path', '?enabled=true'), { method: 'POST' });
        } catch {
            // 다음 시작에 다시 확인한다.
        }
    }
}

module.exports = { PaddockSetupNotices, WslLauncherDialog };
