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
const { Unicode11Addon } = require('xterm-addon-unicode11');
const { createEmojiWidthFixer } = require('./emoji-width');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');
const { EnvVariablesServer } = require('@theia/core/lib/common/env-variables');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { BinaryBuffer } = require('@theia/core/lib/common/buffer');
const { FileUri } = require('@theia/core/lib/common/file-uri');
const URI = require('@theia/core/lib/common/uri').default;
const { isWslShell } = require('./wsl-terminals');
const { PASTE_PLACE, pastePathText, imageExtension } = require('./paste-paths');

class TerminalReplacement {
    static TIMEOUT_MS = 20000;
    // Interval for repeating a stop request that the previous shell did not act on.
    static REPEAT_CLOSE_MS = 1000;
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
    /** A detached or moving document can briefly have no measurable terminal cells. */
    doResizeTerminal() {
        if (!this.isDisposed) {
            const dimensions = this.fitAddon.proposeDimensions();
            if (dimensions && Number.isInteger(dimensions.cols) && Number.isInteger(dimensions.rows)
                && dimensions.cols > 0 && dimensions.rows > 1) {
                // Keep Theia's bottom margin and the last valid process size until
                // the destination window supplies usable font and layout metrics.
                this.term.resize(dimensions.cols, dimensions.rows - 1);
                this.resizeTerminalProcess();
            }
        }
    }

    /** Clearing a selection keeps the copied text available for the next paste. */
    get copyOnSelection() {
        return super.copyOnSelection && this.term.hasSelection();
    }

    /**
     * 마우스 버튼을 누른 채(끌어 고르는 중)에는 링크 상자("Follow link")를 띄우지 않는다.
     * Theia는 경로·주소 위에 포인터가 머물면 버튼 상태와 무관하게 상자를 띄워, 글자를 끌어 고르는 동안 선택 위에 상자가 뜬다.
     */
    showLinkHover(
        invokeAction,
        x,
        y,
        message,
    ) {
        if (!this.isPointerPressed) super.showLinkHover(invokeAction, x, y, message);
    }

    /** Keeps the last account label when the launch entry was renamed and then removed. */
    storeState() {
        const state = super.storeState();
        if (this.options.paddockShared) state.paddockShared = true;
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
        // 기본 이름(terminal N)의 번호는 만들 때 한 번 정한다. 재시작해도 같은 이름으로 보이게 함께 저장한다.
        if (this.paddockNumber) state.paddockNumber = this.paddockNumber;
        return state;
    }

    restoreState(
        oldState,
    ) {
        if (oldState.paddockShared) this.options.paddockShared = true;
        if (Number.isSafeInteger(oldState.paddockNumber) && oldState.paddockNumber > 0) this.paddockNumber = oldState.paddockNumber;
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
        if (this.options.paddockShared) throw new Error('Switch accounts in the original terminal window.');
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
            const stopped = waitForReplacementStep(Promise.all([closing, process.closed]), 'Stopping the previous shell');
            // A shell can ignore one stop request: Bash still waiting to complete a key sequence after Escape stays running.
            // Repeat the request until the shell exits; closing an already stopped process is a no-op on the backend.
            let repeat;
            const scheduleRepeat = () => {
                repeat = setTimeout(() => {
                    if (!process.isClosed) {
                        this.shellTerminalServer.close(process.id).catch(() => {});
                        scheduleRepeat();
                    }
                }, TerminalReplacement.REPEAT_CLOSE_MS);
            };
            scheduleRepeat();
            try {
                await stopped;
            } finally {
                clearTimeout(repeat);
            }
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
        // Closing a second view must leave the original window's shell running.
        if (this.options.paddockShared) this.closeOnDispose = false;
        // Process errors during startup keep the tab available for Retry. Explicit close is still allowed below.
        if (this.explicitClose || (!this.pendingReplacement && !this.pendingStartCleanup)) super.dispose();
    }

    init() {
        super.init();
        // 밝은 테마에서도 명령과 출력의 색을 읽을 수 있도록 배경과의 최소 명암비를 유지한다.
        this.term.options.minimumContrastRatio = 4.5;
        // 터미널의 기본 글자 폭 기준(Unicode 6)은 ✅·😀 같은 이모지를 한 칸으로 센다. Claude Code 같은 화면 프로그램은 두 칸으로 세고
        // 그 폭에 맞춰 커서를 옮기므로, 이모지가 든 줄부터 표 선과 이전 글자가 옆 칸에 겹쳐 남는다. 이모지를 두 칸으로 세는 Unicode 11 기준을 쓴다.
        // 글자 폭 기준 변경은 실험 기능이라 바꾸는 순간에만 허용한다. 허용 여부는 Theia가 명령 기록 설정에 맞춰 관리하므로 원래 값으로 되돌린다.
        const allowProposedApi = this.term.options.allowProposedApi;
        this.term.options.allowProposedApi = true;
        this.term.loadAddon(new Unicode11Addon());
        this.term.unicode.activeVersion = '11';
        this.term.options.allowProposedApi = allowProposedApi;
        // ⚠️처럼 원래 한 칸인 기호에 이모지 표시 문자를 붙인 경우는 이 기준으로도 한 칸이라, 출력을 넘기기 전에 빈칸을 채워 두 칸으로 맞춘다.
        this.fixEmojiWidth = createEmojiWidthFixer();
        // 터미널은 붙여 넣은 글자만 받는다. 탐색기에서 복사하거나 끌어다 놓은 파일, 캡처한 이미지는 파일 경로로 바꿔 넣는다.
        // Claude Code·Codex는 입력에 들어온 이미지 경로를 첨부 이미지로 읽는다. 터미널 그리기 모듈보다 먼저 받으려고 캡처 단계에서 듣는다.
        const insertFiles = (event, transfer) => this.insertFiles(event, transfer);
        const allowFileDrop = (event) => {
            if ([...(event.dataTransfer?.items || [])].some(item => item.kind === 'file')) {
                event.preventDefault();
                event.stopPropagation();
                event.dataTransfer.dropEffect = 'copy';
            }
        };
        const onPaste = event => insertFiles(event, event.clipboardData);
        const onDrop = event => insertFiles(event, event.dataTransfer);
        this.node.addEventListener('paste', onPaste, true);
        this.node.addEventListener('dragover', allowFileDrop, true);
        this.node.addEventListener('drop', onDrop, true);
        this.toDispose.push({
            dispose: () => {
                this.node.removeEventListener('paste', onPaste, true);
                this.node.removeEventListener('dragover', allowFileDrop, true);
                this.node.removeEventListener('drop', onDrop, true);
            },
        });
        // 끌어 고르는 동안(버튼을 누른 채)은 링크 상자를 띄우지 않는다(showLinkHover). 누르는 순간 떠 있던 상자도 숨긴다.
        // 뗌은 터미널 밖에서 끝날 수 있어 창 전체에서 듣는다.
        const press = () => {
            this.isPointerPressed = true;
            this.hideLinkHover();
        };
        const release = () => { this.isPointerPressed = false; };
        this.node.addEventListener('pointerdown', press, true);
        window.addEventListener('pointerup', release, true);
        window.addEventListener('pointercancel', release, true);
        this.toDispose.push({
            dispose: () => {
                this.node.removeEventListener('pointerdown', press, true);
                window.removeEventListener('pointerup', release, true);
                window.removeEventListener('pointercancel', release, true);
            },
        });
        // 많은 터미널을 열면 브라우저가 오래된 그래픽 자원을 회수할 수 있다.
        // 그래픽 연결이 끊기면 바로 기본 그리기로 돌아가 복구 대기 중 출력이 비지 않게 한다.
        const showText = () => queueMicrotask(() => {
            if (!this.isDisposed) this.webglAddon.dispose();
        });
        this.node.addEventListener('webglcontextlost', showText, true);
        this.toDispose.push({ dispose: () => this.node.removeEventListener('webglcontextlost', showText, true) });
        // 화면 밖에서 그래픽 복구 실패가 통지돼도 기존 출력과 계속 들어오는 내용을 보존한다.
        this.toDispose.push(this.webglAddon.onContextLoss(() => this.webglAddon.dispose()));
        // 한글처럼 서로 다른 글자를 많이 그리면 글자 그림 저장소의 페이지가 가득 차, 그리기 모듈이 페이지 넷을 하나로 합치고 뒤 페이지의 번호를 당긴다.
        // GPU에 올린 그림은 페이지 번호별 버전으로만 갱신을 판단해, 번호가 당겨진 페이지의 버전이 우연히 같으면 옛 그림 위에 새 좌표로 그려 글자가 깨진다.
        // 합치기가 끝나면 모든 페이지를 다시 올리고 화면을 다시 그린다. 저장소는 비우지 않는다 — 비우면 합쳐진 페이지가 빈 채로 남아 다시 못 쓰고,
        // 화면의 글자 수가 남은 용량에 가까우면 비우기와 합치기가 되풀이되며 글자가 빈칸으로 남는다. 한 번 합칠 때 페이지 넷이 빠지므로 한 번만 예약한다.
        let isReuploadQueued = false;
        const reuploadGlyphs = () => {
            if (!isReuploadQueued) {
                isReuploadQueued = true;
                queueMicrotask(() => {
                    isReuploadQueued = false;
                    if (!this.isDisposed) this.reuploadGlyphTextures();
                });
            }
        };
        this.toDispose.push(this.webglAddon.onRemoveTextureAtlasCanvas(reuploadGlyphs));
        // 창을 오래 가려 두거나 절전에서 돌아오면 그래픽 연결은 살아 있어도 GPU에 올린 글자 그림이 손상될 수 있다. 창으로 돌아올 때 같은 방법으로 다시 올린다.
        const redrawText = () => {
            if (document.visibilityState === 'visible') reuploadGlyphs();
        };
        document.addEventListener('visibilitychange', redrawText);
        window.addEventListener('focus', redrawText);
        this.toDispose.push({
            dispose: () => {
                document.removeEventListener('visibilitychange', redrawText);
                window.removeEventListener('focus', redrawText);
            },
        });
    }

    /**
     * GPU에 올린 글자 그림을 전부 다시 올리고 화면을 다시 그린다.
     *
     * 그리기 모듈은 옵션이 하나라도 바뀌면 글자 저장소를 다시 연결하며 페이지 버전을 초기화해, 다음 그리기에서 모든 페이지를 다시 올린다.
     * 저장소 구성에 들어가지 않는 옵션을 잠깐 바꿨다 되돌려 그 경로만 빌린다. 저장소의 글자는 그대로라 다시 그리는 비용만 든다.
     * 기본 그리기로 이미 돌아간 뒤에는 옵션 변경이 화면에 아무 영향도 주지 않는다.
     */
    reuploadGlyphTextures() {
        const duration = this.term.options.smoothScrollDuration || 0;
        this.term.options.smoothScrollDuration = duration + 1;
        this.term.options.smoothScrollDuration = duration;
        this.term.refresh(0, this.term.rows - 1);
    }

    /** 붙여넣기·끌어다 놓기에 글자 없이 파일만 있으면 그 경로를 입력한다. 글자가 있으면 터미널의 기본 붙여넣기에 맡긴다. */
    insertFiles(
        event,
        transfer,
    ) {
        const files = [...(transfer?.files || [])];
        if (files.length && !transfer.getData('text/plain') && this.enablePaste) {
            event.preventDefault();
            event.stopPropagation();
            this.pasteFilePaths(files).catch(error => this.messageService.error(error instanceof Error ? error.message : String(error)));
        }
    }

    async pasteFilePaths(
        files,
    ) {
        const paths = [];
        for (const file of files) {
            // 디스크에 있는 파일은 경로가 있다. 캡처한 화면처럼 경로가 없는 그림은 저장한 뒤 그 경로를 넣는다.
            const filePath = window.electronTheiaCore?.getPathForFile(file) || await this.savePastedImage(file);
            paths.push(filePath);
        }
        const isWindows = OS.backend.type() === OS.Type.Windows;
        const place = !isWindows ? PASTE_PLACE.POSIX : isWslShell(this.options.shellPath) ? PASTE_PLACE.WSL : PASTE_PLACE.WINDOWS;
        this.term.focus();
        // 뒤에 빈칸을 두어 이어서 입력하는 글자가 경로에 붙지 않게 한다.
        this.paste(`${pastePathText(paths, place)} `);
    }

    /** 경로 없는 클립보드 그림을 앱 설정 폴더의 pasted-images에 저장하고 그 경로를 돌려준다. 그림이 아니면 이유를 알린다. */
    async savePastedImage(
        file,
    ) {
        if (!file.type.startsWith('image/')) {
            throw new Error(`Cannot read the path of ${file.name || 'the pasted item'}. Copy the file from a folder on this computer.`);
        }
        const directory = new URI(await this.envVariablesServer.getConfigDirUri()).resolve('pasted-images');
        const stamp = new Date().toISOString().replace(/[-:]/g, '').replace('T', '-').slice(0, 15);
        const target = directory.resolve(`paste-${stamp}-${Math.random().toString(36).slice(2, 6)}.${imageExtension(file.type)}`);
        await this.fileService.createFile(target, BinaryBuffer.wrap(new Uint8Array(await file.arrayBuffer())));
        return FileUri.fsPath(target);
    }

    /** 화면 프로그램이 두 칸으로 센 기호가 실제로 두 칸을 차지하도록 출력을 고친 뒤 터미널에 넘긴다. */
    write(
        data,
    ) {
        super.write(typeof data === 'string' ? this.fixEmojiWidth(data) : data);
    }

    async attachTerminal(
        id,
    ) {
        if (!this.options.paddockShared) return super.attachTerminal(id);
        const terminalId = await this.shellTerminalServer.attach(id);
        if (!IBaseTerminalServer.validateId(terminalId)) throw new Error('The terminal in the other window has closed.');
        this.exitStatus = undefined;
        return terminalId;
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
decorate(inject(FileService), PaddockTerminal.prototype, 'fileService');
decorate(inject(EnvVariablesServer), PaddockTerminal.prototype, 'envVariablesServer');
decorate(inject(MessageService), PaddockTerminal.prototype, 'messageService');

module.exports = { PaddockTerminal };
