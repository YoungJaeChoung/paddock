const { MessageService } = require('@theia/core/lib/common/message-service');
const { URI } = require('@theia/core');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { Widget } = require('@lumino/widgets');
const { MessageLoop } = require('@lumino/messaging');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { OpenerService } = require('@theia/core/lib/browser/opener-service');
const { WidgetManager } = require('@theia/core/lib/browser/widget-manager');
const { StorageService } = require('@theia/core/lib/browser/storage-service');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { CommandRegistry } = require('@theia/core/lib/common/command');
const { KeybindingRegistry } = require('@theia/core/lib/browser/keybinding');
const { EnvVariablesServer } = require('@theia/core/lib/common/env-variables');
const { ConfirmDialog, SingleTextInputDialog } = require('@theia/core/lib/browser/dialogs');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');
const { TerminalService } = require('@theia/terminal/lib/browser/base/terminal-service');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { TerminalProfileService, NULL_PROFILE } = require('@theia/terminal/lib/browser/terminal-profile-service');
const { ShellTerminalProfile } = require('@theia/terminal/lib/browser/shell-terminal-profile');
const { HostedPluginSupport } = require('@theia/plugin-ext/lib/hosted/browser/hosted-plugin');
const { CustomEditorWidget } = require('@theia/plugin-ext/lib/main/browser/custom-editors/custom-editor-widget');
const { ScmService } = require('@theia/scm/lib/browser/scm-service');
const { VSXExtensionsContribution } = require('@theia/vsx-registry/lib/browser/vsx-extensions-contribution');
const { RemoteStatusService } = require('@theia/remote/lib/electron-common/remote-status-service');
const { PreferenceService } = require('@theia/core/lib/common/preferences/preference-service');
const { getCurrentPort } = require('@theia/core/lib/electron-browser/messaging/electron-local-ws-connection-source');
const model = require('./work-model');
const usage = require('./usage-model');
const agent = require('./agent-model');

// 파일 구획에서 숨기는 이름. VS Code의 기본 files.exclude와 같다.
const HIDDEN_FILES = new Set(['.git', '.DS_Store', 'Thumbs.db']);

// 작업 목록을 남기는 저장 키. 폴더 목록과 터미널 소속을 JSON으로 둔다.
const STORAGE_KEY = 'paddock.work-folders.v1';

// 터미널 현재 폴더·실행 중 프로그램·메모리처럼 이벤트가 없는 값을 다시 읽는 간격(ms).
const REFRESH_INTERVAL = 2000;

// Theia 보기 컨테이너 id. 사이드바 보기 줄의 Source control·Extensions가 이 보기를 품는다.
const VIEW_CONTAINER = {
    scm: 'scm-view-container',
    extensions: 'vsx-extensions-view-container',
};

// 상태 줄 게이지 묶음의 출처 표지. 막대 색이 사용량 수준을 뜻하므로 출처는 색이 아니라 표지 모양·이름으로 가른다(메모리 표지는 상태 줄 틀에 있다).
const SOURCE_MARKS = {
    claude: '✱',
    codex: '◎',
};

// 본문을 나누는 명령. 단축키는 VS Code의 새 터미널·터미널 분할과 같다.
const PADDOCK_COMMANDS = {
    splitDown: { id: 'paddock.work.splitDown', label: 'Paddock: New Work Terminal Below' },
    splitRight: { id: 'paddock.work.splitRight', label: 'Paddock: New Work Terminal to the Side' },
};

function element(
    tag,
    className,
    text,
) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
}

function codicon(
    name,
) {
    return element('span', `codicon codicon-${name}`);
}

function button(
    label,
    className,
    action,
) {
    const node = element('button', className);
    node.type = 'button';
    if (typeof label === 'string') node.textContent = label;
    else if (label) node.append(...label);
    node.addEventListener('click', (event) => {
        event.stopPropagation();
        action(event);
    });
    return node;
}

/**
 * 사이드바(Work·Source control·Extensions), 추가 터미널 줄, 본문 경로 줄, 상태 줄을 Theia 서비스와 잇는다.
 *
 * 작업 폴더는 "지금 보고 있는 터미널의 현재 폴더"를 따라 생긴다. 폴더 선택 창은 없다.
 * 작업 폴더에 속한 터미널은 사이드바에, 속하지 않은 터미널은 위쪽 추가 터미널 줄에 나온다.
 */
class PaddockWorkspace {
    constructor(
        container,
    ) {
        this.container = container;
        this.view = 'work';
        this.state = model.empty();
        this.showAll = new Set();
        this.workExpanded = true;
        this.filesExpanded = true;
        this.expandedDirectories = new Set();
        this.n_sidebarRevision = 0;
        this.cwdCache = new Map();
        this.homePath = '';
        this.remote = { alive: false };
        // -- 에이전트 완료 알림 --
        this.activity = new Map();
        this.programs = new Map();
        this.shellPids = new Map();
        this.doneIds = new Set();
        this.watched = new WeakSet();
    }

    // -- 명령·단축키 --

    registerCommands(
        commands,
    ) {
        commands.registerCommand(PADDOCK_COMMANDS.splitDown, { execute: () => this.run(() => this.newWorkTerminal({ split: 'split-bottom' })) });
        commands.registerCommand(PADDOCK_COMMANDS.splitRight, { execute: () => this.run(() => this.newWorkTerminal({ split: 'split-right' })) });
    }

    registerKeybindings(
        keybindings,
    ) {
        this.keybindings = keybindings;
    }

    /**
     * Theia 기본값(새 터미널을 아래 패널에 엶)을 걷어 내고 같은 키로 본문 칸을 나눈다.
     * 다른 기능이 모두 키를 등록한 뒤에 불러야 덮어써지지 않는다.
     */
    bindSplitKeys() {
        for (const key of ['ctrl+shift+`', 'ctrlcmd+shift+5', 'ctrlcmd+\\']) {
            this.keybindings.unregisterKeybinding(key);
        }
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitDown.id, keybinding: 'ctrl+shift+`' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitRight.id, keybinding: 'ctrlcmd+shift+5' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitRight.id, keybinding: 'ctrlcmd+\\' });
    }

    // -- 시작 --

    onStart() {
        this.shell = this.container.get(ApplicationShell);
        this.messages = this.container.get(MessageService);
        this.files = this.container.get(FileService);
        this.terminals = this.container.get(TerminalService);
        this.profiles = this.container.get(TerminalProfileService);
        this.commands = this.container.get(CommandRegistry);
        this.opener = this.container.get(OpenerService);
        this.plugins = this.container.get(HostedPluginSupport);
        this.widgets = this.container.get(WidgetManager);
        this.storage = this.container.get(StorageService);
        this.scm = this.container.get(ScmService);
        this.env = this.container.get(EnvVariablesServer);
        this.remoteStatus = this.container.get(RemoteStatusService);
        this.preferences = this.container.get(PreferenceService);
        this.shell.onDidAddWidget((widget) => {
            widget.title.changed.connect(() => this.refreshSoon());
            if (this.isTerminal(widget)) this.watchTerminal(widget);
            this.refreshSoon();
        });
        this.shell.onDidRemoveWidget((widget) => {
            if (model.folderOf(this.state, widget.id)) {
                this.state = model.forgetTerminal(this.state, widget.id);
                this.save();
            }
            this.refreshSoon();
        });
        this.shell.mainPanel.onDidChangeCurrent(() => this.refreshSoon());
        this.shell.onDidChangeActiveWidget(({ newValue }) => {
            if (newValue && this.shell.getAreaFor(newValue) === 'main') {
                this.lastMainWidget = newValue;
                this.doneIds.delete(newValue.id);
                this.refreshSoon();
            }
        });
        this.shell.mainPanel.layoutModified.connect(() => this.renderPathBars());
        this.scm.onDidAddRepository((repository) => {
            repository.provider.onDidChange(() => this.renderViewBadge());
            this.renderViewBadge();
        });
        const sidebar = this.shell.sidebar.node;
        sidebar.querySelector('.view-bar').addEventListener('click', (event) => {
            const target = event.target.closest('[data-view]');
            if (target) this.run(() => this.showView(target.dataset.view));
        });
        this.shell.header.node.querySelector('.tab-add').addEventListener('click', () => this.run(() => this.newExtraTerminal()));
        const picker = this.shell.header.node.querySelector('.shell-picker');
        const menu = this.shell.header.node.querySelector('#shell-menu');
        menu.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.renderShellMenu();
                const bounds = picker.getBoundingClientRect();
                menu.style.left = `${Math.min(bounds.left, window.innerWidth - 240)}px`;
            }
        });
        this.shell.footer.node.querySelector('.remote-indicator').addEventListener('click', () => this.run(() => this.commands.executeCommand('remote.select')));
        this.files.onDidFilesChange(() => this.refreshSoon());
        this.acceptVsixDrops(sidebar.querySelector('[data-host="extensions"]'));
    }

    /**
     * Extensions 보기에 `.vsix` 파일을 끌어다 놓으면 설치한다(VS Code와 같은 방식).
     * Theia 확장 보기에는 이 동작이 없어 사이드바 칸에서 받아 Theia의 VSIX 설치 함수로 넘긴다.
     */
    acceptVsixDrops(
        host,
    ) {
        const isVsix = (file) => file.name.toLowerCase().endsWith('.vsix');
        host.addEventListener('dragover', (event) => {
            if ([...(event.dataTransfer?.items || [])].some(item => item.kind === 'file')) {
                event.preventDefault();
                host.classList.add('is-drop-target');
            }
        });
        host.addEventListener('dragleave', () => host.classList.remove('is-drop-target'));
        host.addEventListener('drop', (event) => {
            host.classList.remove('is-drop-target');
            const files = [...(event.dataTransfer?.files || [])];
            if (files.length) {
                event.preventDefault();
                this.run(async () => {
                    const vsix = files.filter(isVsix);
                    if (!vsix.length) {
                        throw new Error('Drop a .vsix file to install an extension.');
                    }
                    for (const file of vsix) {
                        const filePath = window.electronTheiaCore.getPathForFile(file);
                        if (!filePath) {
                            throw new Error(`Cannot read the path of ${file.name}. Drop the file from a folder on this computer.`);
                        }
                        // 설치 명령(vsxExtensions.installVSIX)은 선택된 파일만 받으므로 설치 함수를 직접 부른다.
                        await this.container.get(VSXExtensionsContribution).installVsixFile(URI.fromFilePath(filePath));
                    }
                });
            }
        });
    }

    async onDidInitializeLayout() {
        this.homePath = new URI(await this.env.getHomeDirUri()).path.toString();
        const saved = await this.storage.getData(STORAGE_KEY, '');
        const liveIds = new Set(this.terminals.all.map(terminal => terminal.id));
        this.state = model.restore(saved || '', liveIds);
        for (const terminal of this.terminals.all) {
            this.watchTerminal(terminal);
            if (this.shell.getAreaFor(terminal) !== 'main') {
                await this.shell.addWidget(terminal, { area: 'main' });
            }
        }
        if (!saved) {
            // 첫 실행: Theia 기본 배치가 연 터미널을 추가 터미널로 두지 않고, 그 터미널이 열린 폴더의 작업 터미널로 둔다.
            // 터미널이 열린 폴더가 곧 작업 폴더다(현재 폴더를 아직 모르면 홈).
            for (const terminal of this.terminals.all) {
                const key = (await this.readCwd(terminal)) || `file://${this.homePath}`;
                this.state = model.assignTerminal(this.state, terminal.id, key, 'terminal');
                this.cwdCache.set(terminal.id, key);
            }
            await this.save();
        }
        if (!this.shell.mainPanel.currentTitle) {
            // 본문이 비었으면 작업 터미널 하나로 시작한다. 터미널이 열린 폴더가 곧 작업 폴더라서
            // 첫 실행이면 홈이, 저장된 작업 폴더가 있으면 그 첫 폴더가 목록에 선다.
            await this.newWorkTerminal({ folderKey: this.state.folders[0]?.key || `file://${this.homePath}` });
        }
        for (const folder of this.state.folders) {
            this.openRepository(folder.key);
        }
        // 켜자마자 입력할 수 있게 본문에 보이는 터미널에 포커스를 준다.
        const shown = this.shell.mainPanel.currentTitle?.owner;
        if (this.isTerminal(shown)) await this.shell.activateWidget(shown.id);
        this.bindSplitKeys();
        this.layoutReady = true;
        this.refreshRemote();
        setInterval(() => this.run(() => this.tick()), REFRESH_INTERVAL);
        await this.refresh();
    }

    async run(
        action,
    ) {
        try {
            await action();
        } catch (error) {
            this.messages.error(error instanceof Error ? error.message : String(error));
        }
    }

    async save() {
        await this.storage.setData(STORAGE_KEY, model.serialize(this.state));
    }

    refreshSoon() {
        if (this.layoutReady && !this.refreshQueued) {
            this.refreshQueued = true;
            queueMicrotask(() => {
                this.refreshQueued = false;
                this.run(() => this.refresh());
            });
        }
    }

    async refresh() {
        this.renderTabs();
        this.renderPathBars();
        this.renderStatus();
        this.renderViewBadge();
        if (this.view === 'work') await this.renderWork();
    }

    /** 이벤트가 없는 값(터미널 현재 폴더·프로그램·메모리)을 주기적으로 다시 읽는다. */
    async tick() {
        await Promise.all(this.terminals.all.map(terminal => this.readCwd(terminal)));
        await this.refreshPrograms();
        this.checkAgents();
        await this.refresh();
        await this.refreshMemory();
        await this.refreshUsage();
    }

    // -- 에이전트 완료 알림 --

    /** 터미널의 입력(Enter)과 출력을 에이전트 활동 기록에 넣는다. 터미널마다 한 번만 붙인다. */
    watchTerminal(
        terminal,
    ) {
        if (!this.watched.has(terminal)) {
            this.watched.add(terminal);
            this.activity.set(terminal.id, agent.idle());
            terminal.onData((data) => this.activity.set(terminal.id, agent.noteInput(this.activity.get(terminal.id) ?? agent.idle(), data, Date.now())));
            terminal.onOutput(() => this.activity.set(terminal.id, agent.noteOutput(this.activity.get(terminal.id) ?? agent.idle(), Date.now())));
            terminal.onDidDispose?.(() => {
                this.activity.delete(terminal.id);
                this.programs.delete(terminal.id);
                this.doneIds.delete(terminal.id);
            });
        }
    }

    /** 터미널에서 지금 실행 중인 프로그램 이름. 아직 모르면 터미널 제목(셸 이름)이다. */
    programOf(
        terminal,
    ) {
        return this.programs.get(terminal.id) || terminal.title.label;
    }

    /** 터미널마다 셸의 앞쪽 프로그램을 백엔드에 묻는다. 실패하면 마지막 값을 둔다. */
    async refreshPrograms() {
        const terminals = this.terminals.all;
        await Promise.all(terminals.filter(terminal => !this.shellPids.has(terminal.id)).map(async (terminal) => {
            const pid = await terminal.processId.catch(() => null);
            if (pid) this.shellPids.set(terminal.id, pid);
        }));
        const pids = terminals.map(terminal => this.shellPids.get(terminal.id)).filter(Boolean);
        if (pids.length) {
            try {
                const names = await this.fetchJson('/paddock/foreground', 'GET', `?pids=${pids.join(',')}`);
                for (const terminal of terminals) {
                    const name = names[this.shellPids.get(terminal.id)];
                    if (name) this.programs.set(terminal.id, name);
                }
            } catch {
                // 원격 연결이 끊겼을 때 등. 다음 주기에 다시 묻는다.
            }
        }
    }

    /**
     * 끝난 에이전트를 알린다. 지금 보고 있는 터미널이면 소리를 내지 않고,
     * 다른 터미널이면 소리와 함께 사이드바·탭 줄에 완료 표시를 붙인다(그 터미널을 열면 지워진다).
     */
    checkAgents() {
        const now = Date.now();
        const current = this.currentWidget();
        for (const terminal of this.terminals.all) {
            const { activity, finished } = agent.settle(this.activity.get(terminal.id) ?? agent.idle(), now, agent.isAgent(this.programs.get(terminal.id)));
            this.activity.set(terminal.id, activity);
            const watching = terminal === current && terminal.isVisible && document.hasFocus();
            if (finished && !watching) {
                this.doneIds.add(terminal.id);
                if (this.preferences.get('paddock.agentDoneSound', true)) this.playDoneSound();
            }
        }
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

    // -- 터미널 --

    /**
     * 지금 보고 있는 본문 위젯. 사이드바를 누르면 포커스가 본문을 떠나므로
     * 본문에서 마지막으로 활성이던 위젯을 기억해 둔다.
     */
    currentWidget() {
        const last = this.lastMainWidget;
        const alive = last && !last.isDisposed && this.shell.getAreaFor(last) === 'main';
        return alive ? last : this.shell.mainPanel.currentTitle?.owner;
    }

    isTerminal(
        widget,
    ) {
        return widget instanceof TerminalWidget;
    }

    async readCwd(
        terminal,
    ) {
        let cwd = this.cwdCache.get(terminal.id);
        try {
            cwd = (await terminal.cwd).toString();
        } catch {
            // 셸이 막 끝났거나 현재 폴더를 보고하지 않으면 마지막으로 알던 폴더를 쓴다.
            cwd = cwd || terminal.lastCwd?.toString();
        }
        if (cwd) this.cwdCache.set(terminal.id, cwd);
        return cwd;
    }

    /**
     * 새 작업 터미널이 열릴 폴더를 고른다.
     *
     * 지금 보는 것이 터미널이면 그 터미널의 현재 폴더, 편집기면 그 파일이 속한 작업 폴더,
     * 둘 다 아니면 첫 작업 폴더, 목록이 비었으면 홈이다.
     */
    followFolder() {
        const current = this.currentWidget();
        let folder = null;
        if (this.isTerminal(current)) {
            folder = this.cwdCache.get(current.id) || null;
        } else if (current?.getResourceUri?.()) {
            const uri = current.getResourceUri().toString();
            folder = model.folderContaining(this.state, uri);
        }
        return folder || this.state.folders[0]?.key || `file://${this.homePath}`;
    }

    async startTerminal(
        cwd,
        profile = this.profiles.defaultProfile,
    ) {
        let terminal;
        if (profile && profile !== NULL_PROFILE && profile instanceof ShellTerminalProfile) {
            const selected = profile.modify({ cwd, title: profile.shellPath?.split(/[\\/]/).pop() });
            // 절대 경로(/bin/zsh, C:\...\pwsh.exe)만 미리 확인한다. 이름만 적힌 셸은 PATH에서 찾는다.
            if (/^(\/|[A-Za-z]:[\\/])/.test(selected.shellPath ?? '')) {
                const executable = await this.files.resolve(URI.fromFilePath(selected.shellPath)).catch(() => undefined);
                if (!executable?.isFile) {
                    throw new Error(`Cannot start shell: ${selected.shellPath}. Check the executable path.`);
                }
            }
            terminal = await this.terminals.newTerminal(selected.options);
        } else if (profile && profile !== NULL_PROFILE) {
            terminal = await profile.start();
        } else {
            terminal = await this.terminals.newTerminal({ cwd });
        }
        try {
            await terminal.start();
        } catch (error) {
            terminal.dispose();
            throw new Error('Cannot start the shell. Check the executable path.', { cause: error });
        }
        return terminal;
    }

    /** 작업 폴더와 무관한 추가 터미널을 위쪽 줄에 연다. */
    async newExtraTerminal(
        profile,
    ) {
        const terminal = await this.startTerminal(`file://${this.homePath}`, profile);
        await this.terminals.open(terminal, { widgetOptions: { area: 'main' }, mode: 'activate' });
        await this.refresh();
    }

    /**
     * 새 작업 터미널을 연다. 폴더는 `followFolder()`, 또는 `folderKey`로 지정한다.
     * `split`이 있으면 지금 보는 칸을 아래(`split-bottom`)나 옆(`split-right`)으로 나눠 연다.
     */
    async newWorkTerminal(
        { folderKey, split } = {},
    ) {
        const current = this.currentWidget();
        if (this.isTerminal(current)) await this.readCwd(current);
        const key = folderKey || this.followFolder();
        const known = this.state.folders.some(folder => folder.key === key.replace(/\/+$/, ''));
        const terminal = await this.startTerminal(key);
        const n_existing = model.terminalsOf(this.state, key).length;
        this.state = model.assignTerminal(this.state, terminal.id, key, n_existing ? `terminal ${n_existing + 1}` : 'terminal');
        this.cwdCache.set(terminal.id, key);
        await this.save();
        const widgetOptions = split && current ? { area: 'main', mode: split, ref: current } : { area: 'main' };
        await this.terminals.open(terminal, { widgetOptions, mode: 'activate' });
        if (!known) this.openRepository(key);
        this.view = 'work';
        await this.refresh();
    }

    openRepository(
        key,
    ) {
        // Git 확장은 워크스페이스 루트만 찾으므로 작업 폴더의 저장소를 직접 알려 준다. 저장소가 아니면 조용히 넘긴다.
        const path = new URI(key).path.fsPath();
        this.plugins.willStart.then(() => this.commands.executeCommand('git.openRepository', path)).catch(() => undefined);
    }

    async confirmClose(
        terminals,
    ) {
        let close = true;
        if (terminals.length) {
            close = await new ConfirmDialog({
                title: terminals.length > 1 ? `Close ${terminals.length} terminals` : 'Close terminal',
                msg: 'Running shells and commands in these terminals will stop.',
                ok: 'Close',
                cancel: 'Cancel',
            }).open();
        }
        return Boolean(close);
    }

    async closeWidget(
        widget,
    ) {
        const close = this.isTerminal(widget) ? await this.confirmClose([widget]) : true;
        if (close) {
            // 문서의 변경 여부와 저장 확인은 편집기의 저장 계약을 따른다.
            await this.shell.closeWidget(widget.id);
        }
        await this.refresh();
    }

    async removeFolder(
        key,
    ) {
        const current = this.currentWidget();
        const result = model.removeFolder(this.state, key, current?.id);
        const terminals = result.closed.map(id => this.shell.getWidgetById(id)).filter(Boolean);
        if (await this.confirmClose(terminals)) {
            this.state = result.state;
            await this.save();
            for (const terminal of terminals) {
                await this.shell.closeWidget(terminal.id);
            }
            if (result.next && result.next !== current?.id) {
                await this.shell.activateWidget(result.next);
            }
            // 알림은 결과만 알리고 몇 초 뒤 사라진다. 본문을 오래 가리지 않게 한다.
            this.messages.info(`Removed ${new URI(key).path.base} from the list · folder is kept`, { timeout: 4000 });
            await this.refresh();
        }
    }

    async renameFolder(
        key,
    ) {
        // 목록에 보이는 이름만 바꾼다. 디스크의 폴더 이름은 그대로다.
        const label = await new SingleTextInputDialog({ title: 'Rename in list', initialValue: this.folderName(key) }).open();
        if (label !== undefined) {
            this.state = model.setFolderLabel(this.state, key, label);
            await this.save();
            await this.refresh();
        }
    }

    async renameTerminal(
        id,
    ) {
        const entry = this.state.terminals[id];
        const name = await new SingleTextInputDialog({ title: 'Rename terminal', initialValue: entry?.name ?? '' }).open();
        if (name) {
            this.state = model.renameTerminal(this.state, id, name);
            await this.save();
            await this.refresh();
        }
    }

    // -- 표시 도우미 --

    displayPath(
        key,
    ) {
        const path = new URI(key).path.toString();
        return this.homePath && path.startsWith(this.homePath) ? `~${path.slice(this.homePath.length)}` : path;
    }

    folderName(
        key,
    ) {
        const path = new URI(key).path;
        const fallback = path.toString() === this.homePath ? '~' : path.base || this.displayPath(key);
        return model.folderLabel(this.state, key) || fallback;
    }

    branchOf(
        key,
    ) {
        // Git 확장이 상태 줄 명령 제목에 "$(git-branch) main" 식으로 브랜치를 싣는다.
        const repository = this.scm.repositories.find(item => item.provider.rootUri && item.provider.rootUri.replace(/\/+$/, '') === key);
        const title = repository?.provider.statusBarCommands?.[0]?.title || '';
        return title.replace(/\$\([^)]*\)/g, '').trim();
    }

    // -- 사이드바 보기 --

    async showView(
        view,
    ) {
        this.view = view;
        const sidebar = this.shell.sidebar.node;
        for (const item of sidebar.querySelectorAll('[data-view]')) {
            item.classList.toggle('is-active', item.dataset.view === view);
            item.setAttribute('aria-pressed', String(item.dataset.view === view));
        }
        for (const host of sidebar.querySelectorAll('[data-host]')) {
            host.hidden = host.dataset.host !== view;
        }
        if (VIEW_CONTAINER[view]) {
            const host = sidebar.querySelector(`[data-host="${view}"]`);
            const widget = await this.widgets.getOrCreateWidget(VIEW_CONTAINER[view]);
            if (!host.contains(widget.node)) {
                // Theia가 이 보기를 화면에 없는 왼쪽 패널에 먼저 넣어 두므로, 그 부모에서 떼어 사이드바로 옮긴다.
                if (widget.parent) widget.parent = null;
                if (widget.isAttached) Widget.detach(widget);
                Widget.attach(widget, host);
            }
            // 왼쪽 패널에서 숨겨 둔 상태가 따라오므로 다시 보이게 한다.
            widget.show();
            // 사이드바 안에 직접 붙인 보기라 크기 변경을 대신 알려 준다.
            MessageLoop.sendMessage(widget, Widget.ResizeMessage.UnknownSize);
            widget.update();
        }
        await this.refresh();
    }

    renderViewBadge() {
        let n_changes = 0;
        for (const repository of this.scm.repositories) {
            for (const group of repository.provider.groups) {
                n_changes += group.resources.length;
            }
        }
        const badge = this.shell.sidebar.node.querySelector('.view-badge');
        badge.hidden = n_changes === 0;
        badge.textContent = n_changes > 99 ? '99+' : String(n_changes);
    }

    sectionHeader(
        label,
        expanded,
        toggle,
    ) {
        const header = button([codicon(expanded ? 'chevron-down' : 'chevron-right'), element('span', 'section-label', label)], 'section-header', toggle);
        header.setAttribute('aria-expanded', String(expanded));
        return header;
    }

    async renderWork() {
        const n_revision = ++this.n_sidebarRevision;
        const node = element('div', 'work-view');
        const current = this.currentWidget();
        const currentId = current?.id;
        const rows = model.visibleRows(this.state, currentId, this.showAll);
        node.append(this.sectionHeader('WORK', this.workExpanded, () => {
            this.workExpanded = !this.workExpanded;
            this.refreshSoon();
        }));
        if (this.workExpanded) {
            const list = element('div', 'work-list');
            if (!this.state.folders.length) {
                list.append(element('p', 'work-empty', 'No work folders yet.'));
                const steps = element('ol', 'work-steps');
                steps.append(element('li', '', 'In the terminal on the right, git clone or cd into a repo folder.'));
                steps.append(element('li', '', 'Click ‘New work terminal’ below.'));
                list.append(steps);
            }
            for (const row of rows) {
                list.append(this.renderRow(row, currentId));
            }
            node.append(list);
            const follow = this.followFolder();
            const newRow = button([codicon('terminal'), element('span', 'new-work-text', 'New work terminal'), element('span', 'new-work-path', `in ${this.displayPath(follow)}`)], 'new-work', () => this.run(() => this.newWorkTerminal()));
            newRow.title = `Open a work terminal in ${this.displayPath(follow)}`;
            // 목록에 없는 폴더(방금 clone·cd한 레포)를 따라가면 눈에 띄게 한다.
            const isListed = this.state.folders.some(folder => folder.key === follow.replace(/\/+$/, ''));
            const isHome = new URI(follow).path.toString() === this.homePath;
            newRow.classList.toggle('is-new-folder', !isListed && !isHome);
            newRow.dataset.followFolder = follow;
            node.append(newRow);
        }
        const filesFolder = this.filesFolder();
        if (filesFolder) {
            node.append(this.sectionHeader(`${this.folderName(filesFolder).toUpperCase()} FILES`, this.filesExpanded, () => {
                this.filesExpanded = !this.filesExpanded;
                this.refreshSoon();
            }));
            if (this.filesExpanded) {
                const files = element('div', 'file-list');
                await this.appendDirectory(files, new URI(filesFolder), 0);
                node.append(files);
            }
        }
        if (n_revision === this.n_sidebarRevision && this.view === 'work') {
            this.shell.sidebar.node.querySelector('[data-host="work"]').replaceChildren(node);
        }
    }

    /**
     * 파일 구획이 보여 줄 폴더: 지금 보는 작업 터미널의 폴더, 없으면 따라갈 폴더.
     * 작업 폴더가 없고 따라갈 폴더가 홈이면 구획을 숨긴다(홈 전체 목록은 쓸모가 없다).
     */
    filesFolder() {
        const current = this.currentWidget();
        const key = (current && model.folderOf(this.state, current.id)) || this.followFolder();
        const isHome = new URI(key).path.toString() === this.homePath;
        return this.state.folders.length || !isHome ? key : null;
    }

    renderRow(
        row,
        currentId,
    ) {
        let node;
        if (row.kind === 'folder') {
            const folder = this.state.folders.find(item => item.key === row.key);
            node = element('div', `work-row folder-row${row.current ? ' is-current' : ''}`);
            const toggle = button([codicon(row.expanded ? 'chevron-down' : 'chevron-right'), codicon(row.expanded ? 'folder-opened' : 'folder'), element('span', 'row-name', this.folderName(row.key))], 'row-main', () => {
                this.state = model.setExpanded(this.state, row.key, !folder.expanded);
                this.save();
                this.refreshSoon();
            });
            toggle.title = this.displayPath(row.key);
            toggle.setAttribute('aria-expanded', String(row.expanded));
            const count = element('span', 'row-meta', `›_ ${row.n_terminals}`);
            const menu = button([codicon('ellipsis')], 'row-action', (event) => this.openFolderMenu(row.key, event.currentTarget));
            menu.setAttribute('aria-label', `${this.folderName(row.key)} actions`);
            node.append(toggle, count, menu);
            node.dataset.folder = row.key;
        } else if (row.kind === 'terminal') {
            const terminal = this.shell.getWidgetById(row.id);
            node = element('div', `work-row terminal-row${row.id === currentId ? ' is-current' : ''}${this.doneIds.has(row.id) ? ' is-done' : ''}`);
            const select = button([element('span', 'prompt-mark', '›_'), element('span', 'row-name', row.name), element('span', 'row-suffix', row.suffix)], 'row-main', () => this.run(() => this.shell.activateWidget(row.id)));
            select.addEventListener('dblclick', () => this.run(() => this.renameTerminal(row.id)));
            select.title = this.doneIds.has(row.id) ? `${row.name} — agent finished` : `${row.name} — double-click to rename`;
            const program = element('span', 'row-meta', terminal ? this.programOf(terminal) : '');
            const close = button([codicon('close')], 'row-action', () => this.run(() => this.closeWidget(terminal)));
            close.setAttribute('aria-label', `Close ${row.name}`);
            node.append(select, program, close);
            node.dataset.widgetId = row.id;
            if (row.id === currentId) node.setAttribute('aria-current', 'true');
        } else {
            node = button([codicon('chevron-down'), element('span', '', `Show ${row.n_hidden} more`)], 'work-row more-row', () => {
                this.showAll.add(row.folder);
                this.refreshSoon();
            });
        }
        return node;
    }

    openFolderMenu(
        key,
        anchor,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const n_terminals = model.terminalsOf(this.state, key).length;
        const items = [
            ['add', 'New work terminal here', () => this.newWorkTerminal({ folderKey: key })],
            ['edit', 'Rename in list', () => this.renameFolder(key)],
            ['folder-opened', 'Reveal folder', () => this.commands.executeCommand('revealFileInOS', new URI(key))],
            ['close', 'Remove from list', () => this.removeFolder(key), true],
        ];
        for (const [icon, label, action, danger] of items) {
            const item = button([codicon(icon), element('span', '', label)], `menu-item${danger ? ' is-danger' : ''}`, () => {
                menu.hidePopover();
                this.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.append(element('p', 'menu-note', `Closes ${n_terminals} terminal${n_terminals === 1 ? '' : 's'} · folder is kept`));
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        // ⋯ 버튼은 마우스를 올렸을 때만 보이므로 위치는 폴더 행 전체를 기준으로 잡는다.
        const bounds = (anchor.closest('.work-row') || anchor).getBoundingClientRect();
        menu.style.left = `${bounds.right + 4}px`;
        menu.style.top = `${bounds.top}px`;
        menu.showPopover();
    }

    async appendDirectory(
        parent,
        uri,
        depth,
    ) {
        const stat = await this.files.resolve(uri).catch(() => undefined);
        if (!stat) {
            parent.append(element('p', 'work-empty', 'Folder not found.'));
        } else if (!stat.children?.length) {
            parent.append(element('p', 'work-empty', 'Empty folder.'));
        }
        const entries = [...(stat?.children || [])].filter(entry => !HIDDEN_FILES.has(entry.name)).sort((left, right) => Number(right.isDirectory) - Number(left.isDirectory) || left.name.localeCompare(right.name));
        const current = this.currentWidget()?.getResourceUri?.()?.toString();
        for (const entry of entries) {
            const key = entry.resource.toString();
            const expanded = this.expandedDirectories.has(key);
            const icons = entry.isDirectory ? [codicon(expanded ? 'chevron-down' : 'chevron-right'), codicon(expanded ? 'folder-opened' : 'folder')] : [codicon('file')];
            const row = button([...icons, element('span', 'row-name', entry.name)], `file-row${entry.isDirectory ? '' : ' is-file'}${key === current ? ' is-current' : ''}`, () => this.run(async () => {
                if (entry.isDirectory) {
                    if (expanded) this.expandedDirectories.delete(key);
                    else this.expandedDirectories.add(key);
                    await this.refresh();
                } else {
                    await this.openFile(entry.resource);
                }
            }));
            row.style.paddingLeft = `${8 + depth * 12}px`;
            row.title = entry.resource.path.toString();
            row.dataset.uri = key;
            parent.append(row);
            if (entry.isDirectory && expanded) {
                await this.appendDirectory(parent, entry.resource, depth + 1);
            }
        }
    }

    async openFile(
        uri,
    ) {
        await this.plugins.willStart;
        const options = { mode: 'activate', widgetOptions: { area: 'main' }, preview: false };
        const opener = await this.opener.getOpener(uri, options);
        let canOpen = true;
        if (opener.id.startsWith('custom-editor-')) {
            // 같은 파일의 일반 편집기가 열려 있으면 저장 여부를 먼저 결정한다.
            for (const widget of this.shell.mainPanel.widgets()) {
                if (!(widget instanceof CustomEditorWidget) && widget.getResourceUri?.()?.isEqual(uri)) {
                    await this.shell.closeWidget(widget.id);
                    canOpen = this.shell.getAreaFor(widget) !== 'main';
                }
            }
        }
        if (canOpen) {
            await opener.open(uri, options);
        }
    }

    // -- 위쪽 추가 터미널 줄 --

    renderTabs() {
        const strip = this.shell.tabs.node;
        const current = this.currentWidget();
        const extras = this.terminals.all.filter(terminal => !model.folderOf(this.state, terminal.id));
        strip.replaceChildren();
        for (const terminal of extras) {
            const tab = element('div', `tab${terminal === current ? ' is-active' : ''}${this.doneIds.has(terminal.id) ? ' is-done' : ''}`);
            const cwd = this.cwdCache.get(terminal.id);
            const label = cwd ? `${this.programOf(terminal)} · ${this.displayPath(cwd)}` : this.programOf(terminal);
            const select = button(label, 'tab-select', () => this.run(() => this.shell.activateWidget(terminal.id)));
            select.setAttribute('role', 'tab');
            select.setAttribute('aria-selected', String(terminal === current));
            select.dataset.widgetId = terminal.id;
            select.title = label;
            const close = button([codicon('close')], 'tab-close', () => this.run(() => this.closeWidget(terminal)));
            close.setAttribute('aria-label', `Close ${label}`);
            tab.append(select, close);
            strip.append(tab);
        }
    }

    renderShellMenu() {
        const menu = this.shell.header.node.querySelector('#shell-menu');
        menu.replaceChildren();
        for (const [id, profile] of this.profiles.all) {
            if (profile !== NULL_PROFILE) {
                const label = id === 'SHELL' ? 'Default shell' : id;
                const item = button(label, 'shell-option', () => {
                    menu.hidePopover();
                    this.run(() => this.newExtraTerminal(profile));
                });
                item.setAttribute('role', 'menuitem');
                item.title = profile instanceof ShellTerminalProfile ? profile.shellPath || label : label;
                menu.append(item);
            }
        }
    }

    // -- 본문 경로 줄 --

    /**
     * 본문 칸마다 생기는 탭 줄 위에 경로 줄을 얹는다.
     * 작업 터미널은 "폴더 › 이름 · 프로그램 … 브랜치", 추가 터미널은 "Extra terminal › 폴더 · 프로그램",
     * 파일은 "폴더 › 파일"이다. 오른쪽에 아래로 나누기·옆으로 나누기·닫기 버튼을 둔다.
     */
    renderPathBars() {
        for (const tabBar of this.shell.mainPanel.tabBars()) {
            const widget = tabBar.currentTitle?.owner;
            let bar = tabBar.node.querySelector(':scope > .path-bar');
            if (!bar) {
                bar = element('div', 'path-bar');
                tabBar.node.append(bar);
            }
            bar.replaceChildren();
            if (widget) {
                const isActive = widget === this.currentWidget();
                bar.classList.toggle('is-active', isActive);
                const parts = this.pathParts(widget);
                bar.append(codicon(parts.icon), element('span', 'path-folder', parts.folder), codicon('chevron-right'), element('span', 'path-item', parts.item));
                if (parts.meta) bar.append(element('span', 'path-meta', `· ${parts.meta}`));
                bar.append(element('span', 'path-space'));
                if (parts.branch) bar.append(codicon('git-branch'), element('span', 'path-branch', parts.branch));
                const actions = element('span', 'path-actions');
                const down = button([codicon('split-vertical')], 'path-action', () => this.run(async () => {
                    await this.shell.activateWidget(widget.id);
                    await this.newWorkTerminal({ split: 'split-bottom' });
                }));
                down.title = 'New work terminal below (Ctrl+Shift+`)';
                const right = button([codicon('split-horizontal')], 'path-action', () => this.run(async () => {
                    await this.shell.activateWidget(widget.id);
                    await this.newWorkTerminal({ split: 'split-right' });
                }));
                right.title = 'New work terminal to the side (Ctrl+Shift+5)';
                const close = button([codicon('close')], 'path-action', () => this.run(() => this.closeWidget(widget)));
                close.title = 'Close';
                actions.append(down, right, close);
                bar.append(actions);
                bar.onclick = () => this.run(() => this.shell.activateWidget(widget.id));
            }
        }
    }

    pathParts(
        widget,
    ) {
        let parts;
        const folderKey = model.folderOf(this.state, widget.id);
        if (this.isTerminal(widget) && folderKey) {
            const entry = this.state.terminals[widget.id];
            parts = { icon: 'folder-opened', folder: this.folderName(folderKey), item: entry.name, meta: this.programOf(widget), branch: this.branchOf(folderKey) };
        } else if (this.isTerminal(widget)) {
            const cwd = this.cwdCache.get(widget.id);
            parts = { icon: 'terminal', folder: 'Extra terminal', item: cwd ? this.displayPath(cwd) : '', meta: this.programOf(widget), branch: '' };
        } else {
            const uri = widget.getResourceUri?.();
            const key = uri && model.folderContaining(this.state, uri.toString());
            const item = uri ? (key ? uri.toString().slice(key.length + 1) : uri.path.base) : widget.title.label;
            parts = { icon: 'file', folder: key ? this.folderName(key) : 'File', item: decodeURIComponent(item), meta: '', branch: key ? this.branchOf(key) : '' };
        }
        return parts;
    }

    // -- 상태 줄 --

    renderStatus() {
        const current = this.currentWidget();
        const status = this.shell.footer.node.querySelector('#statusbar-right');
        const saveable = current && Saveable.get(current);
        status.replaceChildren();
        const label = element('span', '', saveable ? (saveable.dirty ? 'Unsaved changes' : 'Saved') : this.isTerminal(current) ? (this.remote.alive ? `${this.remote.name}` : 'Local shell') : 'Ready');
        status.append(label);
        if (saveable?.dirty) {
            const save = button('Save', 'save-action', () => this.run(async () => {
                save.disabled = true;
                await saveable.save();
                await this.refresh();
            }));
            status.append(save);
        }
    }

    /**
     * 게이지 하나를 그린다. 채워진 막대가 사용량, 빈 부분이 남은 양이다.
     * `percent`가 null이면 값이 아직 없다는 뜻으로 "—"를 보인다.
     */
    fillMeter(
        node,
        percent,
        title,
    ) {
        const hasValue = typeof percent === 'number';
        node.querySelector('.meter-percent').textContent = hasValue ? `${percent}%` : '—';
        node.querySelector('.meter-fill').style.width = hasValue ? `${Math.min(100, percent)}%` : '0';
        node.classList.toggle('is-empty', !hasValue);
        node.classList.toggle('is-high', hasValue && percent >= 75 && percent < 90);
        node.classList.toggle('is-critical', hasValue && percent >= 90);
        node.title = title;
    }

    meter(
        id,
        label,
        percent,
        title,
    ) {
        const node = element('span', 'meter');
        node.dataset.meter = id;
        if (label) node.append(element('span', 'meter-label', label));
        node.append(element('span', 'meter-bar'), element('span', 'meter-percent'));
        node.querySelector('.meter-bar').append(element('span', 'meter-fill'));
        this.fillMeter(node, percent, title);
        return node;
    }

    async fetchJson(
        path,
        method = 'GET',
        query = '',
    ) {
        // Endpoint는 경로 속 "?"를 쿼리로 넘기지 않으므로 쿼리는 주소를 만든 뒤 붙인다.
        const url = `${new Endpoint({ path }).getRestUrl().toString()}${query}`;
        const response = await fetch(url, { method });
        const body = await response.json();
        if (!response.ok) throw new Error(body.error || `Request failed: ${path}`);
        return body;
    }

    async refreshMemory() {
        try {
            const { percent } = await this.fetchJson('/paddock/memory');
            if (percent !== null) {
                this.fillMeter(this.shell.footer.node.querySelector('[data-meter="memory"]'), percent, `System memory in use: ${percent}%`);
            }
        } catch {
            // 요청이 실패하면 마지막 값을 그대로 둔다. 처음부터 실패면 "—"가 남는다.
        }
    }

    /**
     * Claude·Codex 사용량 게이지를 그린다. 값은 각 CLI가 터미널에서 실행될 때 남긴 마지막 기록이다.
     * Claude 표시는 처음 한 번 자동으로 켜고(Claude Code 상태 줄에 Paddock 명령 등록), 끄기·켜기는 Claude 게이지 하나로 한다.
     */
    async refreshUsage() {
        let data = null;
        try {
            data = await this.fetchJson('/paddock/usage');
        } catch {
            data = null;
        }
        if (data?.claude.state === 'unset' && !this.claudeAutoTried) {
            this.claudeAutoTried = true;
            await this.setClaudeUsage(true, true);
            data = await this.fetchJson('/paddock/usage');
        }
        const host = this.shell.footer.node.querySelector('.ai-usage');
        // 값이 그대로면 다시 그리지 않는다(마우스를 올린 말풍선이 깜빡이지 않게). 분 단위가 바뀌면 문구를 새로 쓴다.
        const key = JSON.stringify([data, Math.floor(Date.now() / 60000)]);
        if (key !== this.usageKey && data) {
            this.usageKey = key;
            host.replaceChildren();
            const now = Math.floor(Date.now() / 1000);
            const updated = (seconds) => (seconds ? ` · updated ${usage.duration(Math.max(0, now - seconds))} ago` : '');
            const toggle = () => this.run(() => this.toggleClaudeUsage());
            // 출처(Claude·Codex)마다 한 묶음: 표지·이름을 한 번 쓰고 그 뒤에 창(5h·wk)별 게이지를 둔다.
            // 막대 색은 사용량 수준을 뜻하므로, 출처는 색이 아니라 표지·이름·구분선으로 가른다.
            const tooltip = (source, window, updatedAt) => `${source} ${usage.windowName(window.label)}: ${usage.describe(window, now)}${updated(updatedAt)}`;
            if (data.claude.state === 'on') {
                const group = this.usageGroup('claude', 'Claude', 'button');
                group.title = 'Click to turn off Claude usage';
                group.addEventListener('click', toggle);
                const windows = usage.activeWindows(data.claude.windows, now);
                if (!windows.length) {
                    group.append(this.meter('claude', '', null, 'Claude usage appears after Claude Code answers in a terminal (Pro·Max plans). Click to turn off.'));
                }
                for (const window of windows) {
                    group.append(this.meter(`claude-${window.label}`, window.label, window.used, `${tooltip('Claude', window, data.claude.updatedAt)}. Click to turn off.`));
                }
                host.append(group);
            } else {
                const group = this.usageGroup('claude', null, 'span');
                const button = element('button', 'usage-toggle');
                button.type = 'button';
                button.append(element('span', '', 'Show Claude usage'));
                button.addEventListener('click', toggle);
                group.append(button);
                host.append(group);
            }
            const codexWindows = usage.activeWindows(data.codex.windows, now);
            if (codexWindows.length) {
                const group = this.usageGroup('codex', 'Codex', 'span');
                for (const window of codexWindows) {
                    group.append(this.meter(`codex-${window.label}`, window.label, window.used, tooltip('Codex', window, data.codex.updatedAt)));
                }
                host.append(group);
            }
        }
    }

    /** 사용량 출처 한 묶음의 틀(표지 + 이름). `name`이 null이면 이름을 생략한다(켜기 버튼이 대신 말한다). */
    usageGroup(
        source,
        name,
        tag,
    ) {
        const group = element(tag, 'usage-group');
        if (tag === 'button') group.type = 'button';
        group.dataset.source = source;
        group.append(element('span', 'source-mark', SOURCE_MARKS[source]));
        if (name) group.append(element('span', 'source-name', name));
        return group;
    }

    async setClaudeUsage(
        enabled,
        isAutomatic = false,
    ) {
        await this.fetchJson('/paddock/usage/claude', 'POST', `?enabled=${enabled}`);
        if (isAutomatic) {
            // 사용자 설정 파일을 바꾼 일이라 처음 한 번 알리고 바로 끌 수 있게 한다.
            // 알림 버튼을 기다리는 동안 게이지 그리기가 멈추지 않게 결과는 따로 처리한다.
            this.messages.info('Claude usage is now shown in the status bar (added a status line to Claude Code settings).', 'Turn off').then((action) => {
                if (action === 'Turn off') {
                    this.run(async () => {
                        await this.setClaudeUsage(false);
                        await this.refreshUsage();
                    });
                }
            });
        }
    }

    async toggleClaudeUsage() {
        const { claude } = await this.fetchJson('/paddock/usage');
        const turnOn = claude.state !== 'on';
        const confirmed = turnOn || await new ConfirmDialog({
            title: 'Turn off Claude usage',
            msg: 'Paddock will remove its status line from Claude Code settings and restore the one you had before.',
            ok: 'Turn off',
            cancel: 'Cancel',
        }).open();
        if (confirmed) {
            await this.setClaudeUsage(turnOn);
            await this.refreshUsage();
        }
    }

    async refreshRemote() {
        const port = getCurrentPort();
        const status = port ? await this.remoteStatus.getStatus(Number(port)).catch(() => ({ alive: false })) : { alive: false };
        this.remote = status;
        const indicator = this.shell.footer.node.querySelector('.remote-indicator');
        indicator.classList.toggle('is-connected', Boolean(status.alive));
        indicator.querySelector('.remote-label').textContent = status.alive ? `${status.type}: ${status.name}` : '';
        indicator.title = status.alive ? `Editing on ${status.name}` : 'Open a remote window';
        await this.refreshMemory();
        await this.refreshUsage();
    }
}
module.exports = { PaddockWorkspace, PADDOCK_COMMANDS };
