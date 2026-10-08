const { URI } = require('@theia/core');
const { OS } = require('@theia/core/lib/common/os');
const model = require('../work-model');
const agent = require('../agent-model');
const wsl = require('../wsl-terminals');
const { AccountDialog } = require('../account-dialog');
const { retryAccountStorage } = require('../account-access-dialog');
const { AccountLaunch, accountTerminalOptions, refreshAccountResume } = require('../account-launch');
const { element, button } = require('./shared');

/**
 * 등록 계정(Claude·Codex)으로 새 터미널을 열거나, 쉬고 있는 터미널을 다른 계정으로 바꿔 시작한다.
 */
class AccountTerminals {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
    }

    /**
     * Shows labels chosen by the user; the official CLI owns login and account identity.
     * With `folderKey` (opened from a sidebar folder ＋), Add & Open starts the account in that folder.
     */
    async manageAccounts(
        { folderKey } = {},
    ) {
        const isWindows = OS.backend.type() === OS.Type.Windows;
        const runtimeOptions = [{ value: 'native', label: isWindows ? 'Windows' : 'This device' }];
        if (isWindows && this.workspace.isWslReady) runtimeOptions.push({ value: 'wsl', label: 'WSL' });
        const current = this.workspace.currentWidget();
        const prefersWsl = isWindows && this.workspace.isWslReady && (wsl.isWslShell(current?.options?.shellPath)
            || wsl.isWslShell(this.workspace.profiles.defaultProfile?.shellPath));
        let opened;
        await new AccountDialog({
            service: this.workspace.accounts,
            runtimeOptions,
            defaultRuntime: prefersWsl ? 'wsl' : 'native',
            onOpen: async profile => { opened = await this.openAccount(profile.id, { folderKey }); },
        }).open();
        await this.updateAccountLabels().catch(() => {});
        await this.workspace.usagePanel.data.updateProfiles();
        void this.workspace.statusBar.refreshUsage();
        // Dialog disposal restores the previous focus. Activate the new terminal only after that finishes.
        if (opened && !opened.isDisposed) await this.workspace.activate(opened.id);
        this.workspace.refreshSoon();
    }

    async updateAccountLabels() {
        const profiles = await this.workspace.accounts.list();
        // ＋가 메뉴 없이 바로 터미널을 열지 정할 때 쓴다. 아직 읽은 적 없으면 값이 없어 메뉴를 띄운다.
        this.workspace.n_knownAccounts = profiles.length;
        let changed = false;
        for (const terminal of this.workspace.terminals.all) {
            const previous = terminal.options?.paddockAccount;
            const current = previous && profiles.find(profile => profile.id === previous.id);
            if (current && current.label !== previous.label) {
                const oldLabel = this.accountLabel(terminal);
                Object.assign(previous, current);
                changed = true;
                if (this.workspace.state.terminals[terminal.id]?.name === oldLabel) {
                    this.workspace.state = model.renameTerminal(this.workspace.state, terminal.id, this.accountLabel(terminal));
                }
            }
        }
        if (changed) {
            await this.workspace.save();
            this.workspace.refreshSoon();
        }
        return profiles;
    }

    /** Opens the requested CLI in a new terminal; a default launch uses the normal shell environment. */
    async openUsageAccount(
        profile,
    ) {
        let terminal;
        if (profile.id) {
            terminal = await this.openAccount(profile.id);
        } else if (Object.hasOwn(AccountLaunch.PROVIDERS, profile.provider)) {
            terminal = await this.workspace.newTerminalHere();
            await terminal.sendText(`${profile.provider}\n`);
        } else {
            throw new Error('Choose Claude or Codex.');
        }
        return terminal;
    }

    accountLabel(
        terminal,
    ) {
        const profile = terminal?.options?.paddockAccount;
        return profile ? `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}` : '';
    }

    /**
     * Prepares the chosen account's shell options for `cwd` without touching any terminal.
     * Opening a new tab and starting in the current tab share it, so both refuse a missing folder, another WSL distribution,
     * or a missing shell before anything is stopped. `cwd` is a folder URI string; WSL accounts receive the path inside WSL.
     */
    async accountLaunchOptions(
        id,
        cwd,
    ) {
        const directory = cwd && await this.workspace.files.resolve(new URI(cwd)).catch(() => undefined);
        if (!directory?.isDirectory) throw new Error('The current folder is unavailable. Open an existing folder and try the account again.');
        const prepared = await retryAccountStorage(() => this.workspace.accounts.prepare(id), this.workspace.accounts, { accountId: id });
        await this.workspace.usagePanel.data.prepareLaunch(prepared.profile);
        const uri = new URI(cwd);
        const isWindows = OS.backend.type() === OS.Type.Windows;
        let launchCwd = cwd;
        if (prepared.profile.runtime === 'wsl') {
            const authority = uri.authority.toLowerCase();
            if (authority === 'wsl.localhost' || authority === 'wsl$') {
                const [distribution, ...segments] = uri.path.toString().replace(/^\//, '').split('/');
                if (distribution.toLowerCase() !== prepared.profile.wslDistribution.toLowerCase()) {
                    throw new Error('This folder belongs to a different WSL distribution. Open a terminal in the account’s distribution first.');
                }
                launchCwd = `/${segments.join('/')}`;
            } else if (uri.path.toString() === this.workspace.homePath) {
                launchCwd = '~';
            } else {
                launchCwd = uri.path.fsPath();
            }
        } else if (isWindows && ['wsl.localhost', 'wsl$'].includes(uri.authority.toLowerCase())) {
            throw new Error('Choose a WSL account for this Linux folder, or open a Windows folder first.');
        }
        const permissions = this.workspace.preferences.get('paddock.agents.permissions', 'ask');
        const options = accountTerminalOptions({ ...prepared, permissions }, { cwd: launchCwd, isWindows, wslEnv: this.workspace.windowsWslEnv });
        const shellFile = await this.workspace.files.resolve(URI.fromFilePath(options.shellPath)).catch(() => undefined);
        if (!shellFile?.isFile) throw new Error('The account shell is unavailable. Install Bash, PowerShell, or WSL for the selected environment and try again.');
        options.cwd = cwd;
        return { prepared, options };
    }

    /**
     * Opens the chosen environment beside the current terminal without changing its process or history.
     * With `folderKey` (the sidebar folder ＋), it opens at that work folder's root instead of the shown folder or current terminal.
     */
    async openAccount(
        id,
        { folderKey: targetFolder } = {},
    ) {
        const current = this.workspace.currentWidget();
        const folderKey = targetFolder || this.workspace.shownFolder();
        const root = targetFolder ? null : this.workspace.shownTopTerminal();
        const fileRoot = !targetFolder && this.workspace.isFileOnlyGroup(current) ? this.workspace.fileRoots.get(current.id) : null;
        const cwd = targetFolder
            || (this.workspace.isTerminal(current) ? await this.workspace.readCwd(current) : current?.getResourceUri?.()?.parent?.toString())
            || folderKey || `file://${this.workspace.homePath}`;
        const { prepared, options } = await this.accountLaunchOptions(id, cwd);
        if (prepared.profile.runtime === 'wsl') {
            options.id = `terminal-${crypto.randomUUID()}`;
            options.env[wsl.MARKER] = options.id;
            options.env.WSLENV = wsl.forwardedWslEnv(this.workspace.windowsWslEnv);
        }
        const terminal = await this.workspace.terminals.newTerminal(options);
        if (folderKey) {
            this.workspace.state = model.assignTerminal(this.workspace.state, terminal.id, folderKey, options.title);
            this.workspace.cwdCache.set(terminal.id, cwd);
            await this.workspace.save();
        } else if (root) {
            this.workspace.innerTerminalRoots.set(terminal.id, root);
            await this.workspace.saveInnerTabs();
        } else if (fileRoot) {
            this.workspace.moveFileGroup(fileRoot, terminal.id);
            await this.workspace.saveInnerTabs();
        }
        // 사이드바 폴더에서 열면 지금 보는 탭과 무관한 폴더일 수 있어, 그 폴더의 새 터미널과 같은 자리에 연다.
        const widgetOptions = !targetFolder && current && !current.isDisposed
            ? { area: 'main', mode: fileRoot ? 'split-left' : 'tab-after', ref: current }
            : { area: 'main' };
        await this.workspace.openTerminal(terminal, { widgetOptions, mode: 'activate' });
        for (const warning of prepared.warnings || []) void this.workspace.messages.warn(warning);
        if (targetFolder) {
            this.workspace.selectedFolder = model.folderOf(this.workspace.state, terminal.id);
            this.workspace.view = 'work';
        }
        await this.workspace.refresh();
        return terminal;
    }

    /**
     * Whether this tab is a shell waiting for a command, so replacing it loses no running program.
     * An unknown foreground program is not idle, and a tab shown in another window is never replaced from here.
     */
    isIdleShell(
        terminal,
    ) {
        return this.workspace.isTerminal(terminal) && !terminal.isDisposed && !terminal.options?.paddockShared
            && agent.isShell(this.workspace.programs.get(terminal.id));
    }

    /** Starts the chosen account as a new conversation in this idle shell tab, keeping the tab, its place and its folder. */
    async startAccountHere(
        id,
        terminal,
    ) {
        if (!this.workspace.isTerminal(terminal) || terminal.isDisposed) throw new Error('The original terminal is no longer open. Select a terminal and try again.');
        if (terminal.paddockAccountSwitching) throw new Error('This terminal is already switching accounts. Wait for it to finish.');
        terminal.paddockAccountSwitching = true;
        this.workspace.folderTabs.renderFolderTabs();
        try {
            // A command may have started after the menu opened. Read the foreground again and treat a failed read as busy,
            // so a stale "shell" answer never stops a running program.
            this.workspace.programs.delete(terminal.id);
            delete this.workspace.wslTerminals[terminal.id];
            await this.workspace.agentActivity.refreshWslTerminals();
            await this.workspace.agentActivity.refreshPrograms();
            if (!this.isIdleShell(terminal)) throw new Error('A program is running in this terminal. Choose New terminal… to open the account beside it.');
            const cwd = await this.workspace.readCwd(terminal);
            const { prepared, options } = await this.accountLaunchOptions(id, cwd);
            options.id = terminal.id;
            if (prepared.profile.runtime === 'wsl') {
                options.env[wsl.MARKER] = terminal.id;
                options.env.WSLENV = wsl.forwardedWslEnv(this.workspace.windowsWslEnv);
            }
            const previousLabel = this.accountLabel(terminal);
            await terminal.replaceProcess(options);
            // 지난 셸의 프로그램·계정·PID 판정이 남으면 화면이 이전 셸 기준으로 계정을 보인다. 새 프로세스 기준으로 비운다.
            this.workspace.programs.delete(terminal.id);
            this.workspace.agentAccounts.delete(terminal.id);
            this.workspace.shellPids.delete(terminal.id);
            this.workspace.reportedCwds.delete(terminal.id);
            delete this.workspace.wslTerminals[terminal.id];
            this.workspace.cwdCache.set(terminal.id, cwd);
            this.workspace.activity.set(terminal.id, agent.idle());
            this.workspace.doneIds.delete(terminal.id);
            if (this.workspace.state.terminals[terminal.id]?.name === previousLabel) {
                this.workspace.state = model.renameTerminal(this.workspace.state, terminal.id, this.accountLabel(terminal));
            }
            await this.workspace.save();
            void this.workspace.statusBar.refreshUsage();
            for (const warning of prepared.warnings || []) void this.workspace.messages.warn(warning);
            await this.workspace.activate(terminal.id);
        } finally {
            terminal.paddockAccountSwitching = false;
            await this.workspace.refresh();
        }
    }

    /** Restarts the chosen account in the existing tab only after its exact conversation is prepared. */
    async switchTerminalAccount(
        id,
        terminal,
    ) {
        if (!this.workspace.isTerminal(terminal) || terminal.isDisposed) throw new Error('The original terminal is no longer open. Select a terminal and try again.');
        if (terminal.paddockAccountSwitching) throw new Error('This terminal is already switching accounts. Wait for it to finish.');
        terminal.paddockAccountSwitching = true;
        this.workspace.folderTabs.renderFolderTabs();
        try {
            await this.workspace.observingAccountSessions;
            const request = this.workspace.agentActivity.accountSessionRequest(terminal);
            if (!request) throw new Error('No conversation has been identified in this terminal. Start Claude Code before switching accounts.');
            request.shellPid = await terminal.processId.catch(() => request.shellPid);
            const prepared = await retryAccountStorage(() => this.workspace.accounts.prepareResume(id, request), this.workspace.accounts, { accountId: id });
            // 새 탭으로 여는 경로(accountLaunchOptions)와 같이, 바뀐 계정의 Claude가 시작하기 전에 상태 줄을 넣어 둔다.
            await this.workspace.usagePanel.data.prepareLaunch(prepared.profile);
            const cwd = await this.workspace.readCwd(terminal);
            const directory = cwd && await this.workspace.files.resolve(new URI(cwd)).catch(() => undefined);
            if (!directory?.isDirectory) throw new Error('The current folder is unavailable. Restore it before switching accounts.');
            const isWindows = OS.backend.type() === OS.Type.Windows;
            const launchCwd = prepared.profile.runtime === 'wsl' ? prepared.resume.cwd : cwd;
            const permissions = this.workspace.preferences.get('paddock.agents.permissions', 'ask');
            const options = accountTerminalOptions({ ...prepared, permissions }, { cwd: launchCwd, isWindows, wslEnv: this.workspace.windowsWslEnv });
            const shellFile = await this.workspace.files.resolve(URI.fromFilePath(options.shellPath)).catch(() => undefined);
            if (!shellFile?.isFile) throw new Error('The account shell is unavailable. Restore it before switching accounts.');
            options.cwd = cwd;
            options.id = terminal.id;
            if (prepared.profile.runtime === 'wsl') {
                options.env[wsl.MARKER] = terminal.id;
                options.env.WSLENV = wsl.forwardedWslEnv(this.workspace.windowsWslEnv);
            }
            // Keep the verified source for retry even if the new shell fails after the old CLI exits.
            if (terminal.options.paddockAccount) {
                terminal.options = refreshAccountResume(terminal.options, prepared.resume);
                // A failed first switch must also restore this conversation in the original account.
                terminal.hasReplacedProcess = true;
            } else {
                terminal.options.paddockResume = prepared.resume;
            }
            const previousLabel = this.accountLabel(terminal);
            await this.workspace.accounts.stopSession({ ...request, resume: prepared.resume });
            await terminal.replaceProcess(options);
            // 지난 프로세스의 프로그램·계정 판정이 남아 있으면 아래 재개 확인 요청이 이전 계정을 가리킨다. 새 프로세스 기준으로 비운다.
            this.workspace.programs.delete(terminal.id);
            this.workspace.agentAccounts.delete(terminal.id);
            this.workspace.shellPids.delete(terminal.id);
            const shellPid = await terminal.processId.catch(() => null);
            if (shellPid) {
                this.workspace.shellPids.set(terminal.id, shellPid);
                // Bind the resumed process immediately; a quick Ctrl+C can precede the next periodic observation.
                let resumed;
                for (let n_attempts = 0; n_attempts < 20 && !resumed && !terminal.isDisposed; n_attempts += 1) {
                    if (n_attempts) await new Promise(resolve => setTimeout(resolve, 150));
                    resumed = await this.workspace.accounts.observeSession(this.workspace.agentActivity.accountSessionRequest(terminal)).catch(() => undefined);
                }
                if (resumed) terminal.options = refreshAccountResume(terminal.options, resumed);
                else void this.workspace.messages.warn('The account shell opened, but Claude has not confirmed the resumed conversation. Keep Claude open until it loads before switching again.');
            }
            this.workspace.programs.delete(terminal.id);
            this.workspace.agentAccounts.delete(terminal.id);
            this.workspace.reportedCwds.delete(terminal.id);
            delete this.workspace.wslTerminals[terminal.id];
            this.workspace.cwdCache.set(terminal.id, cwd);
            this.workspace.activity.set(terminal.id, agent.idle());
            this.workspace.doneIds.delete(terminal.id);
            if (this.workspace.state.terminals[terminal.id]?.name === previousLabel) {
                this.workspace.state = model.renameTerminal(this.workspace.state, terminal.id, this.accountLabel(terminal));
            }
            await this.workspace.save();
            void this.workspace.statusBar.refreshUsage();
            for (const warning of prepared.warnings || []) void this.workspace.messages.warn(warning);
            await this.workspace.activate(terminal.id);
        } finally {
            terminal.paddockAccountSwitching = false;
            await this.workspace.refresh();
        }
    }

    async renderAccountMenu(
        { openInNewTerminal = false } = {},
    ) {
        const menu = this.workspace.shell.folderBar.node.querySelector('#account-menu');
        const terminal = this.workspace.currentWidget();
        menu.replaceChildren(element('p', 'account-menu-hint', 'Loading accounts…'));
        try {
            const profiles = await this.updateAccountLabels();
            const source = this.workspace.agentActivity.accountSessionRequest(terminal);
            const continueConversation = !openInNewTerminal && source?.provider === 'claude';
            // 빈 셸 탭은 셸을 바꿔도 잃을 작업이 없어 그 탭에서 시작한다. 그 밖의 탭은 새 터미널로 연다.
            const startHere = !openInNewTerminal && !continueConversation && this.isIdleShell(terminal);
            const choices = continueConversation ? profiles.filter(profile => profile.provider === source.provider && profile.runtime === source.runtime
                && (source.runtime !== 'wsl' || !source.wslDistribution || profile.wslDistribution?.toLowerCase() === source.wslDistribution.toLowerCase())) : profiles;
            // 고를 계정이 없으면 "이어갈 계정"을 말하는 안내가 항목(새 대화·계정 관리)과 어긋나므로 없다는 사실을 적는다.
            const hint = choices.length === 0 ? 'No accounts added yet' : continueConversation ? 'Continue this conversation with' : startHere ? 'Start in this terminal' : 'Open a new terminal';
            menu.replaceChildren(element('p', 'account-menu-hint', hint));
            for (const profile of choices) {
                const isCurrent = continueConversation && profile.id === source.accountId;
                const item = button([
                    element('span', 'shell-option-name', `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`),
                    element('span', 'shell-option-meta', isCurrent ? 'Current' : profile.runtime === 'wsl' ? 'WSL' : ''),
                ], 'shell-option', () => {
                    menu.hidePopover();
                    this.workspace.run(() => continueConversation ? this.switchTerminalAccount(profile.id, terminal)
                        : startHere ? this.startAccountHere(profile.id, terminal) : this.openAccount(profile.id));
                });
                const action = continueConversation ? 'Continue with' : startHere ? 'Start in this terminal with' : 'New conversation with';
                item.title = `${action} ${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`;
                item.disabled = isCurrent || Boolean(terminal?.paddockAccountSwitching);
                item.setAttribute('role', 'menuitem');
                menu.append(item);
            }
            if (continueConversation || startHere) {
                const create = button(continueConversation ? 'New conversation…' : 'New terminal…', 'shell-option account-menu-new', () => {
                    this.workspace.run(() => this.renderAccountMenu({ openInNewTerminal: true }));
                });
                create.setAttribute('role', 'menuitem');
                menu.append(create);
            }
        } catch {
            menu.replaceChildren(element('p', 'account-menu-hint', 'Accounts could not be loaded. Open Manage accounts to retry.'));
        }
        const manage = button('Manage accounts…', 'shell-option account-menu-manage', () => {
            menu.hidePopover();
            this.workspace.run(() => this.manageAccounts());
        });
        manage.setAttribute('role', 'menuitem');
        menu.append(manage);
        if (menu.matches(':popover-open')) menu.querySelector('button:not(:disabled)')?.focus({ preventScroll: true });
    }
}

module.exports = { AccountTerminals };
