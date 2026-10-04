const { TerminalWidgetImpl } = require('@theia/terminal/lib/browser/terminal-widget-impl');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { waitForClosed } = require('@theia/core/lib/browser/widgets');
const { DisposableCollection } = require('@theia/core/lib/common/disposable');
const { Deferred } = require('@theia/core/lib/common/promise-util');
const { OS } = require('@theia/core/lib/common/os');
const { IBaseTerminalServer } = require('@theia/terminal/lib/common/base-terminal-protocol');
const { terminalsPath } = require('@theia/terminal/lib/common/terminal-protocol');
const { guessShellTypeFromExecutable } = require('@theia/terminal/lib/common/shell-type');
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');

class TerminalReplacement {
    static TIMEOUT_MS = 20000;
    static ENVIRONMENT = new Set(['CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'WSLENV', 'PADDOCK_TERMINAL']);
    static PROFILE_FIELDS = ['id', 'provider', 'label', 'runtime', 'wslDistribution'];
    static RESUME_FIELDS = ['provider', 'sessionId', 'sourceAccountId', 'runtime', 'wslDistribution', 'cwd', 'transcriptPath', 'shellPid', 'shellStart'];
}

function metadataFields(
    value,
    fields,
) {
    const result = {};
    for (const field of fields) {
        if (typeof value?.[field] === 'string' || value?.[field] === null
            || (field === 'shellPid' && Number.isSafeInteger(value?.[field]) && value[field] > 0)) result[field] = value[field];
    }
    return result;
}

/** Saves only launch paths, account labels and transcript references; authentication values never enter widget state. */
function storedLauncher(
    options,
) {
    let result;
    if (typeof options?.shellPath === 'string' && (Array.isArray(options.shellArgs) || typeof options.shellArgs === 'string')) {
        result = {
            shellPath: options.shellPath,
            shellArgs: Array.isArray(options.shellArgs) ? options.shellArgs.filter(value => typeof value === 'string') : options.shellArgs,
            cwd: options.cwd?.toString(),
            title: typeof options.title === 'string' ? options.title : undefined,
            env: Object.fromEntries(Object.entries(options.env || {}).filter(([name, value]) => value === null
                || (TerminalReplacement.ENVIRONMENT.has(name) && typeof value === 'string'))),
        };
        if (options.paddockAccount) result.paddockAccount = metadataFields(options.paddockAccount, TerminalReplacement.PROFILE_FIELDS);
        if (options.paddockResume) result.paddockResume = metadataFields(options.paddockResume, TerminalReplacement.RESUME_FIELDS);
        if (typeof options.paddockBrowserDirectory === 'string') result.paddockBrowserDirectory = options.paddockBrowserDirectory;
    }
    return result;
}

async function waitForReplacementStep(
    promise,
    description,
) {
    let timer;
    let result;
    try {
        result = await Promise.race([promise, new Promise((_, reject) => {
            timer = setTimeout(() => reject(new Error(`${description} timed out. The terminal is kept; try the account switch again.`)), TerminalReplacement.TIMEOUT_MS);
        })]);
    } finally {
        clearTimeout(timer);
    }
    return result;
}

/**
 * 실행 중인 터미널의 닫기 요청을 확인한 뒤 종료한다.
 * 탭 버튼·메뉴·단축키는 같은 닫기 계약을 쓰며, 셸 프로세스가 스스로 끝날 때의 dispose는 확인 없이 유지한다.
 */
class PaddockTerminal extends TerminalWidgetImpl {
    /** Keeps the last account label when the launch entry was renamed and then removed. */
    storeState() {
        const state = super.storeState();
        const created = this.createdReplacement?.isClosed ? undefined : this.createdReplacement;
        const options = created?.options || this.replacementSavedState?.options || this.options;
        if (created) {
            // A renderer reload leaves backend processes running. Reattach to the known target
            // instead of starting the previous account beside a shell that already exists.
            state.terminalId = created.id;
            state.titleLabel = options.title || this.title.label;
        } else if (this.replacementSavedState) {
            state.terminalId = -1;
            state.titleLabel = this.replacementSavedState.title.label;
        }
        if (options.paddockAccount) state.paddockAccountLabel = options.paddockAccount.label;
        // WidgetManager retains the original construction options, so a replaced launcher must travel with widget state.
        if (this.hasReplacedProcess || created) state.paddockLauncher = storedLauncher(options);
        if (options.paddockResume) state.paddockResume = metadataFields(options.paddockResume, TerminalReplacement.RESUME_FIELDS);
        return state;
    }

    restoreState(
        oldState,
    ) {
        const launcher = storedLauncher(oldState.paddockLauncher);
        if (launcher) {
            this.options = { ...this.options, ...launcher };
            this.hasReplacedProcess = true;
        }
        if (oldState.paddockResume) this.options.paddockResume = metadataFields(oldState.paddockResume, TerminalReplacement.RESUME_FIELDS);
        const label = oldState.paddockAccountLabel;
        if (this.options.paddockAccount && typeof label === 'string' && label.trim() && label.length <= 64 && !/[\u0000-\u001f\u007f]/.test(label)) {
            this.options.paddockAccount.label = label;
        }
        super.restoreState(oldState);
    }

    /** Replaces the shell in this tab after its CLI has stopped, preserving the widget, output and layout. */
    async replaceProcess(
        options,
    ) {
        if (this.isDisposed) throw new Error('This terminal has already closed.');
        if (!this.pendingReplacement) {
            this.pendingReplacement = Promise.resolve().then(() => this.replaceShell(options)).finally(() => {
                this.pendingReplacement = undefined;
                this.replacementSavedState = undefined;
                if (!this.replacementFailed && this.createdReplacement) {
                    this.createdReplacement.exitListener?.dispose();
                    this.createdReplacement = undefined;
                }
            });
        }
        const terminalId = await this.pendingReplacement;
        return terminalId;
    }

    async replaceShell(
        options,
    ) {
        const previous = this.options;
        const previousTitle = { label: this.title.label, caption: this.title.caption };
        // Until a target process ID is known, a layout save keeps the last successful account.
        // Once creation succeeds, storeState saves that process and its launcher for reconnection.
        this.replacementSavedState = { options: previous, title: previousTitle };
        if (this.stdinBeforeReplacement === undefined) this.stdinBeforeReplacement = this.term.options.disableStdin;
        this.term.options.disableStdin = true;
        let terminalId;
        try {
            // A timed-out create can still finish later. Its cleanup must complete before another shell is started.
            if (this.pendingStartCleanup) await waitForReplacementStep(this.pendingStartCleanup, 'Stopping the previous account shell');
            await this.stopReplacementProcess();
            if (this.isDisposed) throw new Error('This terminal has already closed.');
            this.options = { ...options, id: this.id, created: previous.created, location: previous.location };
            this.exitStatus = undefined;
            const starting = this.start();
            try {
                terminalId = await waitForReplacementStep(starting, 'Starting the account shell');
                if (this.isDisposed) throw new Error('This terminal has already closed.');
                await waitForReplacementStep(this.waitForConnection.promise, 'Connecting the account shell');
                if (this.isDisposed) throw new Error('This terminal has already closed.');
            } catch (error) {
                const cleanup = starting.catch(() => {}).then(() => this.stopReplacementProcess()).catch(() => {
                    // The pending process is retained so Retry can finish closing it before creating another one.
                }).finally(() => {
                    if (this.pendingStartCleanup === cleanup) this.pendingStartCleanup = undefined;
                    if (this.replacementFailed) Object.assign(this.title, previousTitle);
                });
                this.pendingStartCleanup = cleanup;
                throw error;
            }
            this.replacementFailed = false;
            this.hasReplacedProcess = true;
            this.term.options.disableStdin = this.stdinBeforeReplacement;
            this.stdinBeforeReplacement = undefined;
        } catch (error) {
            this.options = previous;
            Object.assign(this.title, previousTitle);
            this.replacementFailed = true;
            throw error;
        }
        return terminalId;
    }

    /** Retains a replacement's created process ID even when reading its shell information fails. */
    async createTerminal() {
        let terminalId;
        if (!this.pendingReplacement) terminalId = await super.createTerminal();
        else {
            const launchingOptions = this.options;
            let rootURI = this.options.cwd?.toString();
            if (!rootURI) rootURI = (await this.workspaceService.roots)[0]?.resource?.toString();
            terminalId = await this.shellTerminalServer.create({
                shell: this.options.shellPath || this.shellPreferences.shell[OS.backend.type()],
                args: this.options.shellArgs || this.shellPreferences.shellArgs[OS.backend.type()],
                env: this.options.env,
                strictEnv: this.options.strictEnv,
                isPseudo: this.options.isPseudoTerminal,
                rootURI,
                cols: this.term.cols,
                rows: this.term.rows,
                enableShellIntegration: this.preferences['terminal.integrated.enableCommandHistory'] ?? false,
            });
            if (!IBaseTerminalServer.validateId(terminalId)) throw new Error('Error creating terminal widget, see the backend error log for more information.');
            // The base start method assigns terminalId only after this method returns. Retain
            // it separately until metadata succeeds so failed startup can still stop that shell.
            const process = { id: terminalId, isClosed: false };
            this.processToReplace = process;
            const created = { id: terminalId, options: launchingOptions, isClosed: false };
            created.exitListener = this.terminalWatcher.onTerminalExit(({ terminalId: exitedId }) => {
                if (exitedId === created.id) {
                    created.isClosed = true;
                    created.exitListener?.dispose();
                }
            });
            this.createdReplacement = created;
            const processInfo = await this.shellTerminalServer.getProcessInfo(terminalId);
            const shellType = guessShellTypeFromExecutable(processInfo.executable);
            if (shellType) {
                this._shellName = shellType;
                this.onShellTypeChangedEmiter.fire(shellType);
                if (!this.hasUserTitle && this.options.useServerTitle) {
                    this.title.label = shellType;
                    this.title.caption = shellType;
                }
            }
            if (this.processToReplace === process) this.processToReplace = undefined;
        }
        return terminalId;
    }

    /** Waits for the old process or its data channel to close before this widget starts another shell. */
    async stopReplacementProcess() {
        if (!this.processToReplace && this.terminalId >= 0) {
            this.processToReplace = {
                id: this.terminalId,
                connection: this.waitForConnection?.promise,
                state: this.terminalConnection,
                isClosed: this.connectionClosed,
            };
            // Theia's exit watcher disposes the current terminal. Detach its numeric process ID before requesting exit.
            this._terminalId = -1;
            this.waitingForConnectionCloseToDispose = false;
            this.waitForConnection = undefined;
        }
        const process = this.processToReplace;
        if (process) {
            if (!process.closed) {
                process.closed = new Promise(resolve => {
                    process.confirmClosed = () => {
                        process.isClosed = true;
                        if (this.createdReplacement?.id === process.id) {
                            this.createdReplacement.isClosed = true;
                            this.createdReplacement.exitListener?.dispose();
                        }
                        process.listener?.dispose();
                        process.exitListener?.dispose();
                        resolve();
                    };
                    process.exitListener = this.terminalWatcher.onTerminalExit(({ terminalId }) => {
                        if (terminalId === process.id) process.confirmClosed();
                    });
                    // Closing a shell must not depend on its data channel ever becoming available.
                    Promise.resolve(process.connection).then(connection => {
                        if (connection && !process.isClosed) process.listener = connection.onClose(process.confirmClosed);
                        if (process.isClosed || process.state?.closed) process.confirmClosed();
                    }).catch(() => {});
                    if (process.isClosed || process.state?.closed) process.confirmClosed();
                });
            }
            const closing = this.shellTerminalServer.close(process.id).then(async () => {
                // Exit may have happened before the listener was installed; attach reports an absent process explicitly.
                if (await this.shellTerminalServer.attach(process.id) < 0) process.confirmClosed();
            });
            await waitForReplacementStep(Promise.all([closing, process.closed]), 'Stopping the previous shell');
            if (this.processToReplace === process) this.processToReplace = undefined;
        }
    }

    /** Restricts terminal input, output and close callbacks to the process that owns their connection. */
    connectTerminalProcess() {
        if (typeof this.terminalId === 'number' && !this.options.isPseudoTerminal) {
            this.toDisposeOnConnect.dispose();
            this.toDispose.push(this.toDisposeOnConnect);
            this.connectionClosed = false;
            this.waitingForConnectionCloseToDispose = false;
            const state = { id: this.terminalId, closed: false };
            this.terminalConnection = state;
            const waitForConnection = this.waitForConnection = new Deferred();
            const ownsConnection = () => !this.isDisposed && this.terminalConnection === state && this.terminalId === state.id;
            this.connectionProvider.listen(`${terminalsPath}/${state.id}`, (_path, connection) => {
                if (!ownsConnection()) connection.close();
                else {
                    connection.onMessage(event => {
                        if (ownsConnection()) this.write(event().readString());
                    });
                    // Exclude device status responses emitted by the terminal itself, as the base widget does.
                    const sendData = data => {
                        if (ownsConnection() && data && !this.deviceStatusCodes.has(data) && !this.disableEnterWhenAttachCloseListener()) {
                            connection.getWriteBuffer().writeString(data).commit();
                        }
                    };
                    const disposable = new DisposableCollection();
                    disposable.push(this.term.onData(sendData));
                    disposable.push(this.term.onBinary(sendData));
                    this.toDisposeOnConnect.push(disposable);
                    connection.onClose(() => {
                        disposable.dispose();
                        state.closed = true;
                        if (ownsConnection()) {
                            this.connectionClosed = true;
                            if (this.waitingForConnectionCloseToDispose) this.finalizeAndDispose();
                        }
                    });
                    waitForConnection.resolve(connection);
                }
            }, false);
        }
    }

    async reconnectTerminalProcess() {
        if (!this.pendingReplacement && !this.pendingStartCleanup && !this.replacementFailed) {
            await super.reconnectTerminalProcess();
        }
    }

    /** An exit callback queued by the previous shell must not close the newly attached shell or a retryable tab. */
    finalizeAndDispose() {
        const terminalId = this.terminalId;
        this.term.write('', () => {
            if (!this.isDisposed && !this.pendingReplacement && !this.replacementFailed && this.terminalId === terminalId) {
                if (this.commandHistoryState?.currentCommand) this.finishCurrentCommand();
                this.dispose();
            }
        });
    }

    dispose() {
        // Process errors during startup keep the tab available for Retry. Explicit close is still allowed below.
        if (this.explicitClose || (!this.pendingReplacement && !this.pendingStartCleanup)) super.dispose();
    }

    init() {
        super.init();
        // 밝은 테마에서도 명령과 출력의 색을 읽을 수 있도록 배경과의 최소 명암비를 유지한다.
        this.term.options.minimumContrastRatio = 4.5;
        // 많은 터미널을 열면 브라우저가 오래된 그래픽 자원을 회수할 수 있다.
        // 그래픽 연결이 끊기면 바로 기본 그리기로 돌아가 복구 대기 중 출력이 비지 않게 한다.
        const showText = () => queueMicrotask(() => {
            if (!this.isDisposed) this.webglAddon.dispose();
        });
        this.node.addEventListener('webglcontextlost', showText, true);
        this.toDispose.push({ dispose: () => this.node.removeEventListener('webglcontextlost', showText, true) });
        // 화면 밖에서 그래픽 복구 실패가 통지돼도 기존 출력과 계속 들어오는 내용을 보존한다.
        this.toDispose.push(this.webglAddon.onContextLoss(() => this.webglAddon.dispose()));
    }

    close() {
        return this.closeWithSaving();
    }

    /** 같은 터미널에 닫기를 반복 요청해도 확인 창은 하나만 연다. 취소하면 터미널은 그대로 남는다. */
    async closeWithSaving() {
        if (!this.isDisposed) {
            if (!this.pendingClose) {
                this.pendingClose = Promise.resolve().then(async () => {
                    if (await this.applicationShell.confirmCloseTerminals([this])) await this.closeWithoutSaving();
                }).finally(() => {
                    this.pendingClose = undefined;
                });
            }
            await this.pendingClose;
        }
    }

    /** 일괄 종료 확인을 이미 받은 터미널을 닫는다. Theia의 공개 닫기 계약과 같은 이름을 쓴다. */
    async closeWithoutSaving() {
        this.explicitClose = true;
        if (this.processToReplace) void this.shellTerminalServer.close(this.processToReplace.id).catch(() => {});
        super.close();
        await waitForClosed(this);
    }
}
decorate(injectable(), PaddockTerminal);
decorate(inject(ApplicationShell), PaddockTerminal.prototype, 'applicationShell');

module.exports = { PaddockTerminal };
