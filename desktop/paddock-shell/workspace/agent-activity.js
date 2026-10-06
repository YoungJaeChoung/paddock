const { URI } = require('@theia/core');
const { OS } = require('@theia/core/lib/common/os');
const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const model = require('../work-model');
const layoutModel = require('../layout-model');
const agent = require('../agent-model');
const wsl = require('../wsl-terminals');
const cwdReport = require('../cwd-report');
const { AccountLaunch, accountTerminalOptions, refreshAccountResume } = require('../account-launch');
const agentAccount = require('../agent-account');

/**
 * 터미널마다 실행 중인 프로그램과 계정을 주기적으로 읽어, 에이전트가 돌면 작업 폴더를 만들고 끝나면 알린다.
 */
class AgentActivity {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
        this.watched = new WeakSet();
    }

    /** 이벤트가 없는 값(터미널 현재 폴더·프로그램·메모리)을 주기적으로 다시 읽는다. */
    async tick() {
        await this.refreshWslTerminals();
        await Promise.all(this.workspace.terminals.all.map(terminal => this.workspace.readCwd(terminal)));
        await this.refreshPrograms();
        await this.promoteAgents();
        this.checkAgents();
        this.workspace.rootSwitcher.showRootOf(this.workspace.currentWidget());
        void this.workspace.refreshWorkPresence();
        await this.workspace.refresh();
        await this.workspace.statusBar.refreshMemory();
        await this.workspace.statusBar.refreshUsage({ periodic: true });
    }

    /** 터미널의 입력(Enter)과 출력을 에이전트 활동 기록에 넣는다. 터미널마다 한 번만 붙인다. */
    watchTerminal(
        terminal,
    ) {
        if (!this.watched.has(terminal)) {
            this.watched.add(terminal);
            this.workspace.activity.set(terminal.id, agent.idle());
            terminal.onData((data) => {
                this.workspace.activity.set(terminal.id, agent.noteInput(this.workspace.activity.get(terminal.id) ?? agent.idle(), data, Date.now()));
                if (data.includes('\r')) {
                    this.acknowledgeActivity(terminal);
                    this.workspace.refreshSoon();
                }
            });
            // 입력 직후 첫 출력은 동기로 처리되어 Theia의 출력 수집보다 먼저 알림이 온다.
            // 화면에 파싱된 출력을 직접 관찰하면 그 첫 출력과 시작할 때 모아 둔 출력도 빠뜨리지 않는다.
            terminal.term.onWriteParsed(() => {
                const now = Date.now();
                const before = this.workspace.activity.get(terminal.id) ?? agent.idle();
                const after = agent.noteOutput(before, now);
                this.workspace.activity.set(terminal.id, after);
                // 미확인은 출력이 재개돼도 남긴다. 해당 터미널을 실제로 확인하면 지운다.
                // 첫 출력과 잠시 멎었다가 재개된 출력은 즉시 표시한다. 계속되는 출력은 기존 주기 갱신에 맡긴다.
                const agentNow = agent.isAgent(this.workspace.programs.get(terminal.id));
                if (agent.activityState(before, now, agentNow) !== agent.activityState(after, now, agentNow)) this.workspace.refreshSoon();
            });
            if (OS.backend.type() === OS.Type.Windows && cwdReport.isBashShell(terminal.options?.shellPath)) this.workspace.absorbResizeKeyLoss(terminal);
            // 셸이 프롬프트마다 알리는 현재 폴더(OSC 7)를 기억한다. Windows 셸은 createTerminal이 이 알림을 켠다.
            terminal.term?.parser?.registerOscHandler(7, (data) => {
                const reported = cwdReport.parseCwdReport(data);
                if (reported) this.workspace.reportedCwds.set(terminal.id, URI.fromFilePath(reported).toString());
                return true;
            });
            terminal.onDidDispose?.(() => {
                this.workspace.reportedCwds.delete(terminal.id);
                this.workspace.activity.delete(terminal.id);
                this.workspace.programs.delete(terminal.id);
                this.workspace.agentAccounts.delete(terminal.id);
                this.workspace.doneIds.delete(terminal.id);
                // 다른 묶음에 가려져 본문에서 빠진 터미널은 패널의 제거 이벤트 없이 종료될 수 있다.
                this.workspace.forgetClosedWidget(terminal);
            });
        }
    }

    /** 터미널마다 셸의 앞쪽 프로그램과 그 프로그램이 쓰는 계정을 백엔드에 묻는다. 실패하면 마지막 값을 둔다. */
    async refreshPrograms() {
        const terminals = this.workspace.terminals.all;
        await Promise.all(terminals.filter(terminal => !terminal.paddockAccountSwitching && !this.workspace.shellPids.has(terminal.id)).map(async (terminal) => {
            const terminalId = terminal.terminalId;
            const pid = await terminal.processId.catch(() => null);
            if (pid && !terminal.paddockAccountSwitching && terminal.terminalId === terminalId) this.workspace.shellPids.set(terminal.id, pid);
        }));
        const pids = terminals.map(terminal => this.workspace.shellPids.get(terminal.id)).filter(Boolean);
        if (pids.length) {
            try {
                const foregrounds = await this.workspace.fetchJson('/paddock/foreground', 'GET', `?pids=${pids.join(',')}`);
                for (const terminal of terminals) {
                    const foreground = foregrounds[this.workspace.shellPids.get(terminal.id)];
                    if (foreground?.program) this.rememberForeground(terminal, foreground);
                }
            } catch {
                // 원격 연결이 끊겼을 때 등. 다음 주기에 다시 묻는다.
            }
        }
        // Windows 백엔드는 앞쪽 프로그램을 모르므로 WSL 터미널은 WSL 안에서 읽은 프로그램을 쓴다.
        for (const terminal of terminals) {
            const foreground = this.workspace.wslTerminals[terminal.id];
            if (foreground?.program) this.rememberForeground(terminal, foreground);
        }
        await this.observeAccountSessions();
    }

    /** 앞쪽 프로그램과 계정 판정을 함께 기억한다. 계정을 모르면(환경을 읽지 못함) 지난 판정을 지워 터미널을 연 계정 규칙으로 돌아간다. */
    rememberForeground(
        terminal,
        { program, account },
    ) {
        this.workspace.programs.set(terminal.id, program);
        if (account) this.workspace.agentAccounts.set(terminal.id, account);
        else this.workspace.agentAccounts.delete(terminal.id);
    }

    /**
     * 이 터미널에 대해 화면이 보일 계정 범위. 실행 중인 claude·codex는 자기 환경의 계정이고, 없으면 터미널을 연 계정이다.
     * 계정 터미널에서 다른 도구를 실행하면 그 도구의 범위(기본 CLI 또는 감지된 계정)로 바뀌고, 원래 도구로 돌아오면 원래 계정이다.
     */
    runningScope(
        terminal,
    ) {
        return this.workspace.isTerminal(terminal) ? agentAccount.runningScope({
            program: this.workspace.programs.get(terminal.id),
            launchProfile: terminal.options?.paddockAccount,
            detected: this.workspace.agentAccounts.get(terminal.id),
        }) : null;
    }

    /**
     * 상태 줄·계정 선택에 보일 `도구 · 계정` 이름. 등록 계정·미등록 폴더이거나 계정으로 연 터미널이면 보이고,
     * 일반 터미널의 기본 CLI는 빈 문자열이다(기존처럼 셸 이름을 보인다).
     * 기본 CLI라도 로그인이 등록 계정 하나와 같다고 확인되면(linked) 그 계정 이름으로 보인다.
     */
    runningAccountLabel(
        terminal,
    ) {
        const scope = this.runningScope(terminal);
        const isShown = scope && (scope.id || scope.custom || scope.linked || terminal?.options?.paddockAccount);
        return isShown ? `${AccountLaunch.PROVIDERS[scope.provider]} · ${agentAccount.scopeName(scope)}` : '';
    }

    /** Identifies the terminal's own conversation while its CLI still exposes its process metadata. */
    accountSessionRequest(
        terminal,
    ) {
        const launchProfile = terminal?.options?.paddockAccount;
        const resume = terminal?.options?.paddockResume;
        const program = this.workspace.programs.get(terminal?.id);
        const provider = Object.hasOwn(AccountLaunch.PROVIDERS, program) ? program : launchProfile?.provider || resume?.provider;
        // 터미널을 연 계정은 같은 도구일 때만 원본 계정이다. 다른 도구를 실행 중이면 그 도구의 환경에서 감지한 계정(없으면 기본 CLI)이다.
        // 같은 도구라도 프로세스 환경에서 감지한 계정이 있으면 그것이 먼저다. 상태 줄·계정 메뉴와 같은 판정(runningScope)을 써서
        // 화면에 보인 계정과 전환 요청의 계정이 어긋나지 않게 한다. 감지 값이 없을 때(환경을 읽지 못함·셸로 돌아옴)만 터미널을 연 계정이다.
        const scope = this.runningScope(terminal);
        const profile = scope?.id && scope.provider === provider ? scope : null;
        let request = null;
        // 미등록 폴더로 실행한 CLI는 대화 기록을 찾을 등록 범위가 없어 계정 전환 대상이 아니다.
        if (this.workspace.isTerminal(terminal) && Object.hasOwn(AccountLaunch.PROVIDERS, provider) && !scope?.custom) {
            const runtime = profile?.runtime || (wsl.isWslShell(terminal.options?.shellPath) ? 'wsl' : 'native');
            const args = Array.isArray(terminal.options?.shellArgs) ? terminal.options.shellArgs : [];
            const distributionIndex = args.findIndex(argument => argument === '-d' || argument === '--distribution');
            request = {
                terminalId: terminal.id,
                shellPid: this.workspace.shellPids.get(terminal.id),
                accountId: profile?.id || null,
                provider,
                runtime,
                ...(runtime === 'wsl' ? { wslDistribution: profile?.wslDistribution || (distributionIndex >= 0 ? args[distributionIndex + 1] : undefined) } : {}),
                ...(resume ? { resume } : {}),
            };
        }
        return request;
    }

    /** Retains verified IDs before Ctrl+C returns the terminal to its shell; no file-recency guess is used. */
    async observeAccountSessions() {
        if (!this.workspace.observingAccountSessions) {
            const terminals = this.workspace.terminals.all.filter(terminal => !terminal.paddockAccountSwitching);
            const requests = terminals.map(terminal => this.accountSessionRequest(terminal)).filter(request => request?.shellPid && request.provider === 'claude');
            if (requests.length) {
                this.workspace.observingAccountSessions = this.workspace.accounts.observeSessions(requests).then(sessions => {
                    for (const terminal of terminals) {
                        const resume = sessions[terminal.id];
                        if (resume !== undefined && !terminal.isDisposed && !terminal.paddockAccountSwitching) {
                            // /clear and /resume can change the conversation after an account switch.
                            // A restored shell must use that current conversation, including a deliberately cleared one.
                            if (terminal.hasReplacedProcess && terminal.options.paddockAccount) {
                                terminal.options = refreshAccountResume(terminal.options, resume);
                            } else if (resume) terminal.options.paddockResume = resume;
                            else delete terminal.options.paddockResume;
                        }
                    }
                }).catch(() => {
                    // Metadata may be unavailable during CLI startup or a disconnected remote session.
                    // The switch action checks again and reports a specific error before stopping anything.
                }).finally(() => { this.workspace.observingAccountSessions = undefined; });
            }
        }
        await this.workspace.observingAccountSessions;
    }

    /** WSL 터미널이 있으면 WSL 안의 현재 폴더·실행 프로그램을 한 번에 읽어 둔다. 실패하면 지난 값을 둔다. */
    async refreshWslTerminals() {
        if (this.workspace.terminals.all.some(terminal => wsl.isWslShell(terminal.options?.shellPath))) {
            try {
                this.workspace.wslTerminals = await this.workspace.fetchJson('/paddock/wsl-terminals');
            } catch {
                // WSL이 시작 중이거나 원격 연결이 끊겼을 때. 다음 주기에 다시 읽는다.
            }
        }
    }

    /**
     * 에이전트가 실행 중인 추가 터미널을 그 폴더의 작업 터미널로 올린다.
     * 에이전트를 띄운 곳이 곧 작업할 곳이라 작업 폴더는 이렇게만 생긴다. 한 번 올린 터미널은 에이전트가 끝나도 남는다.
     */
    async promoteAgents() {
        const { state, promoted, added } = model.promoteAgentTerminals(this.workspace.state, this.workspace.terminals.all.map((terminal) => {
            const program = this.workspace.programs.get(terminal.id);
            return { id: terminal.id, root: this.workspace.innerTerminalRoots.get(terminal.id) || terminal.id,
                name: this.workspace.unassignedName(terminal),
                cwd: this.workspace.cwdCache.get(terminal.id), program, isAgent: agent.isAgent(program) };
        }));
        if (promoted.length) {
            const groups = new Map();
            for (const id of promoted) {
                const terminal = this.workspace.shell.getWidgetById(id);
                const previous = terminal && this.workspace.rootSwitcher.rootOf(terminal);
                if (previous?.startsWith('top:') && !groups.has(previous)) {
                    groups.set(previous, this.workspace.displayedRoot === previous ? this.workspace.shell.mainPanel.saveLayout().main : this.workspace.rootLayouts.get(previous));
                }
            }
            this.workspace.state = state;
            let innerChanged = false;
            for (const id of promoted) {
                const terminal = this.workspace.terminals.all.find(item => item.id === id);
                if (this.workspace.accountTerminals.accountLabel(terminal)) this.workspace.state = model.renameTerminal(this.workspace.state, id, this.workspace.accountTerminals.accountLabel(terminal));
                innerChanged = this.workspace.innerTerminalRoots.delete(id) || innerChanged;
            }
            for (const [previous, area] of groups) {
                const rootId = previous.slice(4);
                const folder = model.folderOf(this.workspace.state, rootId);
                const next = `folder:${folder}`;
                for (const [id, root] of this.workspace.fileRoots) {
                    if (root === rootId && this.workspace.shell.getWidgetById(id) instanceof WebviewWidget) this.workspace.webviewFolders.set(id, folder);
                }
                const destination = this.workspace.displayedRoot === next ? this.workspace.shell.mainPanel.saveLayout().main : this.workspace.rootLayouts.get(next);
                const merged = layoutModel.besides([layoutModel.withoutDisposed(destination), layoutModel.withoutDisposed(area)]);
                this.workspace.rootLayouts.set(next, merged);
                this.workspace.rootLayouts.delete(previous);
                if (this.workspace.preferencesReturnRoot === previous) this.workspace.preferencesReturnRoot = next;
                if (this.workspace.displayedRoot === previous || this.workspace.displayedRoot === next) {
                    this.workspace.displayedRoot = next;
                    this.workspace.isRestoringRootLayouts = true;
                    this.workspace.rootSwitcher.showRoot(next, this.workspace.currentWidget());
                }
            }
            if (innerChanged) await this.workspace.saveInnerTabs();
            await this.workspace.save();
            for (const key of added) {
                this.workspace.openRepository(key);
            }
            const current = this.workspace.currentWidget();
            if (current && promoted.includes(current.id)) this.workspace.selectedFolder = model.folderOf(this.workspace.state, current.id);
        }
    }

    /**
     * 긴 에이전트 출력이 멎으면 알린다. 지금 보고 있는 터미널이면 소리를 내지 않고,
     * 다른 터미널이면 소리와 함께 미확인 알림을 붙인다. 해당 터미널을 실제로 확인해야 지워진다.
     */
    checkAgents() {
        const now = Date.now();
        const current = this.workspace.currentWidget();
        for (const terminal of this.workspace.terminals.all) {
            const { activity, finished } = agent.settle(this.workspace.activity.get(terminal.id) ?? agent.idle(), now, agent.isAgent(this.workspace.programs.get(terminal.id)));
            this.workspace.activity.set(terminal.id, activity);
            const watching = terminal === current && terminal.isVisible && document.hasFocus();
            if (watching) this.acknowledgeActivity(terminal);
            if (finished && !watching) {
                this.workspace.doneIds.add(terminal.id);
                if (this.workspace.preferences.get('paddock.agentDoneSound', true)) this.playDoneSound();
            }
        }
    }

    /** 배경 창의 자동 선택은 읽음으로 세지 않는다. 현재 창에서 보이는 선택 터미널만 확인 처리한다. */
    acknowledgeActivity(widget) {
        if (this.workspace.isTerminal(widget) && widget === this.workspace.currentWidget() && widget.isVisible && document.hasFocus()
            && this.workspace.doneIds.delete(widget.id)) this.workspace.refreshSoon();
    }

    /** 짧은 두 음 알림. 소리 파일 없이 만든다. */
    playDoneSound() {
        try {
            this.audio = this.audio || new AudioContext();
            const start = this.audio.currentTime;
            for (const [offset, frequency] of [[0, 880], [0.12, 1320]]) {
                const tone = this.audio.createOscillator();
                const volume = this.audio.createGain();
                tone.frequency.value = frequency;
                volume.gain.setValueAtTime(0.0001, start + offset);
                volume.gain.exponentialRampToValueAtTime(0.18, start + offset + 0.01);
                volume.gain.exponentialRampToValueAtTime(0.0001, start + offset + 0.25);
                tone.connect(volume).connect(this.audio.destination);
                tone.start(start + offset);
                tone.stop(start + offset + 0.3);
            }
        } catch {
            // 소리를 낼 수 없는 환경이면 완료 표시만 남긴다.
        }
    }
}

module.exports = { AgentActivity };
