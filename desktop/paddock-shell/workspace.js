const { MessageService } = require('@theia/core/lib/common/message-service');
const { URI } = require('@theia/core');
const { OS } = require('@theia/core/lib/common/os');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { Widget } = require('@lumino/widgets');
const { MessageLoop } = require('@lumino/messaging');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { OpenerService } = require('@theia/core/lib/browser/opener-service');
const { OpenWithService } = require('@theia/core/lib/browser/open-with-service');
const { WidgetManager } = require('@theia/core/lib/browser/widget-manager');
const { StorageService } = require('@theia/core/lib/browser/storage-service');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { ServiceConnectionProvider } = require('@theia/core/lib/browser/messaging/service-connection-provider');
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
const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const { ScmService } = require('@theia/scm/lib/browser/scm-service');
const { VSXExtensionsContribution } = require('@theia/vsx-registry/lib/browser/vsx-extensions-contribution');
const { VSXExtensionsSearchModel } = require('@theia/vsx-registry/lib/browser/vsx-extensions-search-model');
const { RemoteStatusService } = require('@theia/remote/lib/electron-common/remote-status-service');
const { PreferenceService } = require('@theia/core/lib/common/preferences/preference-service');
const { PreferenceScope } = require('@theia/core/lib/common/preferences/preference-scope');
const { PreferencesWidget } = require('@theia/preferences/lib/browser/views/preference-widget');
const { ThemeService } = require('@theia/core/lib/browser/theming');
const { getCurrentPort } = require('@theia/core/lib/electron-browser/messaging/electron-local-ws-connection-source');
const model = require('./work-model');
const layoutModel = require('./layout-model');
const tabOverflow = require('./tab-overflow');
const { PaddockTerminal } = require('./terminal');
const agent = require('./agent-model');
const wsl = require('./wsl-terminals');
const cwdReport = require('./cwd-report');
const { AccountDialog } = require('./account-dialog');
const { AccountLaunch, accountTerminalOptions } = require('./account-launch');
const { UsagePanel } = require('./usage-panel');

// 작업 목록, 상단 터미널 묶음의 내부 탭, 창을 닫을 때 보던 위젯과 묶음별 칸 배치를 각각 저장한다.
class STORAGE {
    static WORK_FOLDERS = 'paddock.work-folders.v1';
    static INNER_TABS = 'paddock.inner-tabs.v1';
    static SHOWN_WIDGET = 'paddock.shown-widget.v1';
    static ROOT_LAYOUTS = 'paddock.root-layouts.v1';
}

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
    memory: '▦',
};

// 상태 줄 오른쪽 항목별 표시 설정. 설정 화면·빠른 설정이 같은 값을 바꾸고, 사용량 항목은 전체 계정 목록을 연다. 기본은 모두 켜짐이다.
class STATUS_ITEMS {
    static CLAUDE = 'paddock.statusBar.claude';
    static CODEX = 'paddock.statusBar.codex';
    static MEMORY = 'paddock.statusBar.memory';
}

// 본문 나누기·탭 이동·사이드바 명령. 새 터미널·터미널 분할·사이드바 단축키는 VS Code와 같다.
const PADDOCK_COMMANDS = {
    accounts: { id: 'paddock.accounts.manage', label: 'Paddock: Manage Accounts' },
    toggleSidebar: { id: 'paddock.sidebar.toggle', label: 'Paddock: Toggle Sidebar' },
    splitDown: { id: 'paddock.work.splitDown', label: 'Paddock: New Terminal Below' },
    splitRight: { id: 'paddock.work.splitRight', label: 'Paddock: New Terminal to the Side' },
    previousTab: { id: 'paddock.tabs.previous', label: 'Paddock: Previous Terminal Tab' },
    nextTab: { id: 'paddock.tabs.next', label: 'Paddock: Next Terminal Tab' },
    firstTab: { id: 'paddock.tabs.first', label: 'Paddock: First Terminal Tab' },
    lastTab: { id: 'paddock.tabs.last', label: 'Paddock: Last Terminal Tab' },
};

const MARKDOWN_PREVIEW = {
    COMMANDS: {
        preview: 'markdown-preview-enhanced.openPreview',
        both: 'markdown-preview-enhanced.openPreviewToTheSide',
    },
    VIEW_TYPE: 'markdown-preview-enhanced',
    EXTENSIONS: new Set(['.md', '.markdown', '.mdown', '.mkdn', '.mkd', '.rmd', '.qmd']),
};

// 빠른 설정(보기 줄 오른쪽 끝)의 값. 글자 크기는 터미널과 파일 보기에 함께 적용한다.
const APPEARANCE = {
    FONT_SIZE_MIN: 10,
    FONT_SIZE_MAX: 24,
    SIDEBAR_INDENT_MIN: 6,
    SIDEBAR_INDENT_MAX: 24,
    INTERFACE_FONTS: ['Segoe UI', 'Inter', 'Noto Sans', 'Noto Sans CJK KR', 'Malgun Gothic', 'Apple SD Gothic Neo', 'Arial'],
    // 터미널 글꼴 후보. 이 컴퓨터에 설치된 것만 보인다. 한글은 뒤의 한글 글꼴이 채운다.
    TERMINAL_FONTS: ['Consolas', 'Cascadia Mono', 'JetBrains Mono', 'D2Coding', 'Menlo', 'SF Mono', 'Fira Code', 'DejaVu Sans Mono'],
    HANGUL_FALLBACK: "'Malgun Gothic', 'Apple SD Gothic Neo', 'Noto Sans Mono CJK KR', monospace",
};

// 기본 셸을 열 수 없을 때 차례로 시도하는 프로필. Windows 기본 셸은 WSL인데 배포판이 없거나 Git이 없는 PC도 있다.
const FALLBACK_PROFILES = ['Git Bash', 'PowerShell'];

// 크기 변경 뒤 Git Bash에 빈 키(NUL)를 보내기까지 기다리는 시간. 콘솔이 크기 변경을 셸에 알릴 시간을 둔다(absorbResizeKeyLoss).
const RESIZE_KEY_GUARD_MS = 200;

// 운영체제별 기본 셸 설정 이름의 끝부분(terminal.integrated.defaultProfile.<끝부분>).
const OS_PREFERENCE_KEY = { [OS.Type.Windows]: 'windows', [OS.Type.Linux]: 'linux', [OS.Type.OSX]: 'osx' };

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
 * 사이드바(Work·Source control·Extensions), 상단 터미널 묶음 줄, 내부 터미널·파일 탭 줄, 본문 경로 줄, 상태 줄을 Theia 서비스와 잇는다.
 *
 * 작업 폴더는 터미널에서 에이전트(claude·codex 등)가 실행될 때 그 터미널의 현재 폴더로 생긴다.
 * 폴더 선택 창이나 등록 버튼은 없다. 작업 폴더에 속한 터미널은 사이드바와 본문 위 탭 줄에,
 * 상단 터미널과 그 안에서 추가한 내부 터미널은 각각 맨 위 줄과 본문 위 탭 줄에 나온다.
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
        this.pointerPressed = false;
        this.refreshAfterPointer = false;
        this.cwdCache = new Map();
        // 파일 이름 목록은 폴더의 수정 시각이 바뀔 때까지 재사용한다.
        this.directoryEntries = new Map();
        this.n_directoryRevision = 0;
        // 작업 폴더 탭 줄이 보여 줄 폴더. 마지막으로 본 작업 터미널·파일의 폴더다.
        this.selectedFolder = null;
        this.selectedTopTerminal = null;
        this.innerTerminalRoots = new Map();
        this.fileRoots = new Map();
        this.webviewFolders = new Map();
        this.markdownPreviewSources = new Map();
        this.pendingMarkdownSource = null;
        this.homePath = '';
        this.remote = { alive: false };
        // -- 에이전트 완료 알림 --
        this.activity = new Map();
        this.programs = new Map();
        this.shellPids = new Map();
        this.doneIds = new Set();
        this.watched = new WeakSet();
        this.remoteWorkSnapshots = [];
        this.remoteWorkRows = new Map();
        // -- 묶음별 본문 배치 --
        this.rootLayouts = new Map();
        this.displayedRoot = null;
        this.isSwitchingRoot = false;
        // 시작 때 모든 묶음이 한 화면에 복원돼도 처음 저장한 묶음별 비율을 덮어쓰지 않는다.
        this.isRestoringRootLayouts = false;
        // 전체 설정은 본문을 단독으로 쓰고, 닫으면 직전에 선택했던 작업과 칸 배치로 돌아간다.
        this.preferencesReturnRoot = null;
        this.preferencesReturnWidget = null;
        // 시작 시 저장된 작업 폴더·내부 탭 소속을 읽었는지. 읽기 전에는 묶음 전환을 하지 않는다.
        this.isGroupsRestored = false;
        // -- WSL 터미널 --
        // 터미널 id별 WSL 안의 현재 폴더(Windows 경로)·실행 프로그램. Windows 앱의 WSL 터미널만 채워진다.
        this.wslTerminals = {};
        this.windowsWslEnv = '';
        // 터미널 id별로 셸이 프롬프트마다 알린 현재 폴더 주소(OSC 7). Windows 셸에서 쓴다.
        this.reportedCwds = new Map();
        // 이 Windows에 WSL 배포판이 설치돼 있는지와 그 홈 폴더의 경로 부분(/Ubuntu/home/me). 시작할 때 한 번 읽는다.
        this.isWslReady = false;
        this.wslHomePath = '';
    }

    // -- 명령·단축키 --

    registerCommands(
        commands,
    ) {
        commands.registerCommand(PADDOCK_COMMANDS.accounts, { execute: () => this.run(() => this.manageAccounts()) });
        commands.registerCommand(PADDOCK_COMMANDS.toggleSidebar, { execute: () => this.shell.toggleSidebar() });
        commands.registerCommand(PADDOCK_COMMANDS.splitDown, { execute: () => this.run(() => this.newTerminalHere({ split: 'split-bottom' })) });
        commands.registerCommand(PADDOCK_COMMANDS.splitRight, { execute: () => this.run(() => this.newTerminalHere({ split: 'split-right' })) });
        commands.registerCommand(PADDOCK_COMMANDS.previousTab, { execute: () => this.run(() => this.moveTab('previous')) });
        commands.registerCommand(PADDOCK_COMMANDS.nextTab, { execute: () => this.run(() => this.moveTab('next')) });
        commands.registerCommand(PADDOCK_COMMANDS.firstTab, { execute: () => this.run(() => this.moveTab('first')) });
        commands.registerCommand(PADDOCK_COMMANDS.lastTab, { execute: () => this.run(() => this.moveTab('last')) });
    }

    registerKeybindings(
        keybindings,
    ) {
        this.keybindings = keybindings;
    }

    /**
     * Paddock 단축키를 건다. Theia 기본값(새 터미널을 아래 패널에 엶)을 걷어 내고 같은 키로 본문 칸을 나누고,
     * 터미널 탭 이동 키를 더한다. 다른 기능이 모두 키를 등록한 뒤에 불러야 덮어써지지 않는다.
     *
     * `Ctrl+Shift+C`(옆에 새 터미널)와 터미널 안 `Ctrl+방향키`(탭 이동)는 Cursor에서 쓰던 키 설정을 기본값으로 옮긴 것이다.
     * 그 대가로 터미널의 `Ctrl+Shift+C` 복사와 셸의 `Ctrl+←→` 단어 이동은 쓸 수 없다(사용자 결정).
     */
    bindKeys() {
        for (const key of ['ctrl+shift+`', 'ctrlcmd+shift+5', 'ctrlcmd+\\']) {
            this.keybindings.unregisterKeybinding(key);
        }
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitDown.id, keybinding: 'ctrl+shift+`' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitRight.id, keybinding: 'ctrlcmd+shift+5' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitRight.id, keybinding: 'ctrlcmd+\\' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.splitRight.id, keybinding: 'ctrl+shift+c' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.toggleSidebar.id, keybinding: 'ctrlcmd+b' });
        // 터미널의 Ctrl+B 입력 규칙보다 먼저 처리해 커서 이동 대신 사이드바를 접고 펼친다.
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.toggleSidebar.id, keybinding: 'ctrlcmd+b', when: 'terminalFocus' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.previousTab.id, keybinding: 'ctrl+left', when: 'terminalFocus' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.nextTab.id, keybinding: 'ctrl+right', when: 'terminalFocus' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.firstTab.id, keybinding: 'ctrl+up', when: 'terminalFocus' });
        this.keybindings.registerKeybinding({ command: PADDOCK_COMMANDS.lastTab.id, keybinding: 'ctrl+down', when: 'terminalFocus' });
    }

    // -- 묶음별 본문 배치 --

    /**
     * 위젯이 속한 묶음 이름. 작업 폴더의 터미널·파일은 그 폴더, 위쪽 터미널과 그 내부 터미널·파일은 위쪽 터미널이다.
     * 전체 설정은 독립 묶음으로 본문 전체를 쓰며, 그 밖에 어느 묶음에도 속하지 않으면 null이다.
     */
    rootOf(
        widget,
    ) {
        let root = null;
        const folder = widget ? this.workFolderOf(widget) : null;
        if (widget?.id === PreferencesWidget.ID) {
            root = PreferencesWidget.ID;
        } else if (folder) {
            root = `folder:${folder}`;
        } else if (widget instanceof WebviewWidget && this.webviewFolders.has(widget.id)) {
            root = `folder:${this.webviewFolders.get(widget.id)}`;
        } else {
            const top = widget ? this.topTerminalOf(widget) : null;
            if (top) root = `top:${top}`;
        }
        return root;
    }

    /**
     * 본문에 `root` 묶음만 보여야 하는데 그렇지 않은지. 다른 묶음이 화면에 있거나, 위젯이 화면에서 빠져 있으면 참이다.
     */
    needsRootSwitch(
        root,
        widget,
    ) {
        const shown = layoutModel.widgetsOf(this.shell.mainPanel.saveLayout().main);
        return !shown.includes(widget) || this.displayedRoot !== root || shown.some(item => (this.rootOf(item) ?? root) !== root);
    }

    /**
     * 본문에 `root` 묶음의 칸 배치만 보이게 한다.
     * 지금 배치는 묶음별로 나눠 기억하고, 다른 묶음의 위젯은 닫지 않고 화면에서만 뺀다(터미널 프로세스는 계속 산다).
     * 돌아오면 그 묶음이 떠날 때의 칸 나눔과 칸별 선택 탭 그대로 나타난다. 처음 보는 묶음은 위젯 하나짜리 칸이다.
     */
    showRoot(
        root,
        widget,
    ) {
        const panel = this.shell.mainPanel;
        if (root === PreferencesWidget.ID && this.displayedRoot !== root) {
            this.preferencesReturnRoot = this.displayedRoot;
        }
        this.isSwitchingRoot = true;
        try {
            const full = panel.saveLayout().main;
            // 독립 묶음이 없는 보조 화면은 지금 보이던 묶음과 함께 움직인다.
            const fallback = this.displayedRoot ?? root;
            const groupOf = item => this.rootOf(item) ?? fallback;
            for (const name of new Set(layoutModel.widgetsOf(full).map(groupOf))) {
                let area = layoutModel.prune(this.rootLayouts.get(name), item => !item.isDisposed && (this.rootOf(item) ?? name) === name);
                if ((!this.isRestoringRootLayouts && name === this.displayedRoot) || !area) {
                    area = layoutModel.prune(full, item => groupOf(item) === name);
                    // 설정 탭을 끼워 넣으며 바뀐 선택 대신 설정을 열기 직전 파일·터미널을 기억한다.
                    if (root === PreferencesWidget.ID && name === this.preferencesReturnRoot && this.preferencesReturnWidget) {
                        area = layoutModel.select(area, this.preferencesReturnWidget);
                    }
                } else {
                    // 다른 묶음에 터미널을 추가하면 새 위젯만 본문에 먼저 들어온다. 그것으로 보관 중인 전체 배치를 덮지 않는다.
                    for (const item of layoutModel.widgetsOf(full).filter(item => groupOf(item) === name)) {
                        if (!layoutModel.includes(area, item)) area = layoutModel.withWidget(area, item);
                    }
                }
                this.rootLayouts.set(name, area);
            }
            let next = layoutModel.prune(this.rootLayouts.get(root), item => !item.isDisposed && (this.rootOf(item) ?? root) === root);
            if (widget && !layoutModel.includes(next, widget)) next = layoutModel.withWidget(next, widget);
            if (widget) next = layoutModel.select(next, widget);
            if (next) panel.restoreLayout({ main: next });
            this.displayedRoot = root;
            this.isRestoringRootLayouts = false;
        } finally {
            this.isSwitchingRoot = false;
        }
    }

    /** 위젯의 묶음이 본문에 온전히 보이도록 맞춘다. 이미 맞으면 아무것도 하지 않는다. */
    showRootOf(
        widget,
    ) {
        // 저장된 묶음 소속을 읽기 전에는 모든 터미널이 제각각 상단 묶음으로 보여, 되살린 분할 배치를 터미널별로 쪼갠다.
        const root = widget && this.isGroupsRestored && !this.isSwitchingRoot ? this.rootOf(widget) : null;
        if (root && this.needsRootSwitch(root, widget)) this.showRoot(root, widget);
    }

    /** 위젯을 선택한다. 다른 묶음의 위젯이면 먼저 그 묶음의 배치를 본문에 되살린다. */
    async activate(
        id,
    ) {
        const widget = this.shell.getWidgetById(id);
        this.showRootOf(widget);
        const activation = this.shell.activateWidget(id);
        // 시작할 때 창에 입력 초점이 없어도 복원한 파일을 현재 화면으로 기억한다.
        // 같은 칸에서 이미 선택된 탭은 선택 변경 알림도 없으므로, 실제 선택을 확인해 직접 표시한다.
        if (widget && this.shell.getAreaFor(widget) === 'main' && this.shell.getTabBarFor(widget)?.currentTitle === widget.title) {
            this.shell.mainPanel.markAsCurrent(widget.title);
        }
        await activation;
    }

    /**
     * 창을 닫기 직전에 화면에서 빼 둔 다른 묶음의 위젯을 본문에 도로 모은다.
     * Theia는 화면에 있는 위젯만 저장하므로, 이때 모으지 않으면 다음 실행에 다른 묶음의 터미널이 돌아오지 않는다.
     * 다음 실행은 시작 때 묶음별로 다시 나눈다. 닫기가 취소되면 주기 갱신이 현재 묶음만 남긴다.
     */
    onWillStop() {
        // 다음 실행이 같은 묶음으로 시작하도록 지금 보는 위젯을 남긴다. 창이 닫히기 전에 끝나는 동기 저장이다.
        this.storage.setData(STORAGE.SHOWN_WIDGET, this.currentWidget()?.id ?? '');
        this.isSwitchingRoot = true;
        try {
            const panel = this.shell.mainPanel;
            const shown = panel.saveLayout().main;
            const layouts = new Map([...this.rootLayouts].map(([root, area]) => [root,
                layoutModel.prune(area, widget => !widget.isDisposed && (this.rootOf(widget) ?? root) === root),
            ]));
            const groupOf = widget => this.rootOf(widget) ?? this.displayedRoot;
            for (const root of new Set(layoutModel.widgetsOf(shown).map(groupOf))) {
                let area = layouts.get(root);
                if ((!this.isRestoringRootLayouts && root === this.displayedRoot) || !area) {
                    area = layoutModel.prune(shown, widget => !widget.isDisposed && groupOf(widget) === root);
                } else {
                    // 종료 직전에 다른 묶음의 새 탭이 들어와도 기존 탭·칸 배치를 함께 저장한다.
                    for (const widget of layoutModel.widgetsOf(shown).filter(item => !item.isDisposed && groupOf(item) === root)) {
                        if (!layoutModel.includes(area, widget)) area = layoutModel.withWidget(area, widget);
                    }
                }
                layouts.set(root, area);
            }
            // 살아 있는 화면이 보관 배치에서 누락됐어도 재시작 시 사라지지 않게 소속 묶음에 포함한다.
            for (const widget of this.shell.widgets) {
                const root = this.rootOf(widget);
                if (root && !widget.isDisposed && !layoutModel.includes(layouts.get(root), widget)) {
                    layouts.set(root, layoutModel.withWidget(layouts.get(root), widget));
                }
            }
            this.rootLayouts = layouts;
            // 모든 묶음을 좁은 창에 모으면 최소 칸 크기 때문에 비율이 달라진다. 모으기 전 배치를 따로 동기 저장한다.
            this.storage.setData(STORAGE.ROOT_LAYOUTS, [...layouts].filter(([root]) => root).map(([root, area]) => [root, layoutModel.serialize(layoutModel.withoutDisposed(area))]));
            // 다른 묶음은 탭으로 합치지 않고 칸 나눔째 옆에 붙인다. 탭으로 합치면 다음 실행에 그 묶음의 분할이 사라진다.
            const others = [...layouts]
                .filter(([root]) => root !== this.displayedRoot)
                .map(([, area]) => area);
            const merged = layoutModel.besides([layouts.get(this.displayedRoot), ...others]);
            if (merged) {
                panel.restoreLayout({ main: merged });
                // 닫기가 취소돼 다시 묶음을 나눌 때도 모으기 전의 칸 비율을 유지한다.
                this.isRestoringRootLayouts = true;
            }
        } finally {
            this.isSwitchingRoot = false;
        }
    }

    // -- 시작 --

    initialize() {
        // 기본 터미널 모듈까지 모두 등록된 뒤, 저장한 화면을 복원하기 전에 닫기 확인이 있는 위젯을 연결한다.
        this.container.rebind(TerminalWidget).to(PaddockTerminal).inTransientScope();
    }

    onStart() {
        // 초점 변경이 버튼을 누른 사이에 탭·사이드바를 다시 그리면 클릭 대상이 사라진다.
        // 버튼을 놓은 뒤 click 이벤트까지 끝난 다음 밀린 화면 갱신을 실행한다.
        document.addEventListener('pointerdown', () => {
            this.pointerPressed = true;
        }, true);
        const releasePointer = () => {
            setTimeout(() => {
                this.pointerPressed = false;
                if (this.refreshAfterPointer) {
                    this.refreshAfterPointer = false;
                    this.refreshSoon();
                }
            });
        };
        window.addEventListener('pointerup', releasePointer, true);
        window.addEventListener('pointercancel', releasePointer, true);
        window.addEventListener('blur', releasePointer);
        this.shell = this.container.get(ApplicationShell);
        this.shell.beforeActivate = id => this.showRootOf(this.shell.getWidgetById(id));
        this.messages = this.container.get(MessageService);
        this.files = this.container.get(FileService);
        this.terminals = this.container.get(TerminalService);
        // 모든 새 터미널(Theia의 첫 실행 터미널·셸 프로필·확장이 여는 터미널 포함)에 재시작해도 바뀌지 않는 화면 id를 붙인다.
        // 작업 폴더·묶음 소속과 칸 배치는 터미널 id로 저장하는데, Theia 기본 순번(terminal-0, 1…)은 재시작 때 되살리는 순서대로
        // 다시 매겨져 소속이 다른 터미널에 붙는다. 정한 id는 배치 저장에 함께 남아 그대로 되살아나고, 여러 창끼리도 겹치지 않는다.
        // 터미널 모듈이 이 모듈보다 늦게 로드돼 서비스 교체(rebind)를 쓸 수 없어, 레이아웃을 만들기 전인 여기서 감싼다.
        const newTerminal = this.terminals.newTerminal.bind(this.terminals);
        this.terminals.newTerminal = options => newTerminal({ id: `terminal-${crypto.randomUUID()}`, ...options });
        // 저장된 배치가 없는 첫 실행에 Theia가 여는 터미널은 기본 셸 설정이 반영되기 전에 시스템 셸(Windows는 cmd.exe)로 뜬다.
        // 첫 터미널은 onDidInitializeLayout이 기본 셸 프로필(없으면 대체 프로필)로 직접 연다.
        this.terminals.initializeLayout = async () => undefined;
        this.profiles = this.container.get(TerminalProfileService);
        this.commands = this.container.get(CommandRegistry);
        this.opener = this.container.get(OpenerService);
        this.openWith = this.container.get(OpenWithService);
        this.plugins = this.container.get(HostedPluginSupport);
        this.widgets = this.container.get(WidgetManager);
        this.storage = this.container.get(StorageService);
        this.accounts = ServiceConnectionProvider.createProxy(this.container, '/services/paddock-accounts');
        this.workWindowId = window.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
        window.addEventListener('pagehide', () => {
            const url = `${new Endpoint({ path: '/paddock/work-presence' }).getRestUrl().toString()}?windowId=${encodeURIComponent(this.workWindowId)}`;
            fetch(url, { method: 'DELETE', keepalive: true }).catch(() => {});
        });
        this.scm = this.container.get(ScmService);
        this.env = this.container.get(EnvVariablesServer);
        this.remoteStatus = this.container.get(RemoteStatusService);
        this.preferences = this.container.get(PreferenceService);
        this.usagePanel = new UsagePanel({
            host: this.shell.footer.node.querySelector('.ai-usage'),
            fetchJson: this.fetchJson.bind(this),
            listProfiles: () => this.accounts.list(),
            isEnabled: provider => this.isStatusItemOn(provider === 'claude' ? STATUS_ITEMS.CLAUDE : STATUS_ITEMS.CODEX),
            meter: this.meter.bind(this),
            onOpen: profile => this.openUsageAccount(profile),
            onManage: () => this.run(() => this.manageAccounts()),
            isClaudeChosen: () => {
                const inspected = this.preferences.inspect(STATUS_ITEMS.CLAUDE);
                return inspected?.globalValue !== undefined || inspected?.workspaceValue !== undefined;
            },
            onLegacyDisabled: () => this.setPreference(STATUS_ITEMS.CLAUDE, false),
            onAutomaticSetup: () => {
                // 사용자 설정 파일을 바꾼 일이라 처음 한 번 알리고 바로 끌 수 있게 한다.
                // 알림 응답을 기다리는 동안 게이지 그리기가 멈추지 않게 결과는 따로 처리한다.
                void this.messages.info('Claude usage is now shown in the status bar (added a status line to Claude Code settings).', 'Turn off').then(action => {
                    if (action === 'Turn off') this.run(() => this.setPreference(STATUS_ITEMS.CLAUDE, false));
                });
            },
        });
        this.applyInterfacePreferences();
        this.preferences.onPreferenceChanged(({ preferenceName }) => {
            if ([STATUS_ITEMS.CLAUDE, STATUS_ITEMS.CODEX, STATUS_ITEMS.MEMORY].includes(preferenceName)) {
                // 상태 줄 항목을 켜고 끄면 바로 다시 그린다. Claude는 기본 환경과 등록된 모든 계정의 상태 줄도 맞춘다.
                if (preferenceName !== STATUS_ITEMS.MEMORY) this.usagePanel.data.preferencesChanged();
                this.run(async () => {
                    await this.refreshMemory();
                    if (preferenceName !== STATUS_ITEMS.MEMORY) await this.usagePanel.refresh({ settings: preferenceName === STATUS_ITEMS.CLAUDE });
                });
                const panel = this.shell.sidebar.node.querySelector('#quick-settings');
                if (panel.matches(':popover-open')) this.renderQuickSettings();
            }
            if (preferenceName === 'paddock.interfaceFontFamily' || preferenceName === 'paddock.sidebarIndent') {
                this.applyInterfacePreferences();
                if (preferenceName === 'paddock.sidebarIndent') this.refreshSoon();
                const panel = this.shell.sidebar.node.querySelector('#quick-settings');
                if (panel.matches(':popover-open')) this.renderQuickSettings();
            }
        });
        this.shell.onDidAddWidget((widget) => {
            widget.title.changed.connect(() => this.refreshSoon());
            if (this.isTerminal(widget)) this.watchTerminal(widget);
            if (widget instanceof WebviewWidget) {
                if (widget.viewType === MARKDOWN_PREVIEW.VIEW_TYPE && this.pendingMarkdownSource) {
                    this.markdownPreviewSources.set(widget.id, this.pendingMarkdownSource);
                }
                const folder = this.shownFolder();
                if (folder) this.webviewFolders.set(widget.id, folder);
                else {
                    const root = this.fileRoots.get(this.currentWidget()?.id) || this.shownTopTerminal();
                    if (root) {
                        this.fileRoots.set(widget.id, root);
                        if (this.layoutReady && this.pendingMarkdownSource) void this.saveInnerTabs();
                    }
                }
            }
            this.refreshSoon();
        });
        this.shell.onDidRemoveWidget(widget => this.forgetClosedWidget(widget));
        this.shell.mainPanel.onDidChangeCurrent(title => {
            const widget = title?.owner;
            if (!this.isSwitchingRoot && widget && widget.id !== PreferencesWidget.ID && this.rootOf(widget)) {
                this.preferencesReturnWidget = widget;
            }
            this.refreshSoon();
        });
        this.shell.onDidChangeActiveWidget(({ newValue }) => {
            if (newValue && this.shell.getAreaFor(newValue) === 'main') {
                this.lastMainWidget = newValue;
                this.selectedFolder = this.workFolderOf(newValue) || this.selectedFolder;
                if (this.isTerminal(newValue) && !model.folderOf(this.state, newValue.id) && !this.innerTerminalRoots.has(newValue.id)) {
                    this.selectedTopTerminal = newValue.id;
                }
                this.doneIds.delete(newValue.id);
                this.showRootOf(newValue);
                this.refreshSoon();
            }
        });
        this.shell.mainPanel.layoutModified.connect(() => this.renderPathBars());
        // Git 보기는 선택된 저장소 하나만 보여 준다. 새 저장소가 열리면 지금 작업 폴더의 것인지 다시 맞추고, 브랜치가 바뀌면 탭 줄 표시도 다시 그린다.
        this.scm.onDidAddRepository((repository) => {
            repository.provider.onDidChange(() => {
                this.renderViewBadge();
                const shown = this.shell.folderBar.node.querySelector('.folder-bar-branch-name').textContent;
                if (this.branchOf(this.shownFolder()) !== shown) this.refreshSoon();
            });
            this.refreshSoon();
        });
        this.scm.onDidChangeSelectedRepository(() => this.renderViewBadge());
        const sidebar = this.shell.sidebar.node;
        sidebar.querySelector('.view-bar').addEventListener('click', (event) => {
            const target = event.target.closest('[data-view]');
            if (target) this.run(() => this.showView(target.dataset.view));
        });
        this.shell.header.node.querySelector('.tab-add').addEventListener('click', () => this.run(() => this.newExtraTerminal()));
        this.shell.header.node.querySelector('.window-minimize').addEventListener('click', () => window.electronTheiaCore.minimize());
        this.shell.header.node.querySelector('.window-maximize').addEventListener('click', () => {
            if (window.electronTheiaCore.isMaximized()) window.electronTheiaCore.unMaximize();
            else window.electronTheiaCore.maximize();
        });
        this.shell.header.node.querySelector('.window-close').addEventListener('click', () => window.electronTheiaCore.close());
        const updateMaximizeButton = () => {
            const button = this.shell.header.node.querySelector('.window-maximize');
            const maximized = window.electronTheiaCore.isMaximized();
            button.classList.toggle('is-maximized', maximized);
            button.setAttribute('aria-label', maximized ? 'Restore window' : 'Maximize window');
            button.title = maximized ? 'Restore' : 'Maximize';
        };
        window.addEventListener('resize', updateMaximizeButton);
        updateMaximizeButton();
        this.shell.folderBar.node.querySelector('.folder-tab-add').addEventListener('click', () => this.run(() => this.newTerminalFromFolderBar()));
        const accountPicker = this.shell.folderBar.node.querySelector('.account-picker');
        const accountMenu = this.shell.folderBar.node.querySelector('#account-menu');
        accountMenu.addEventListener('beforetoggle', event => {
            if (event.newState === 'open') {
                const bounds = accountPicker.getBoundingClientRect();
                accountMenu.style.left = `${Math.max(8, Math.min(bounds.left, window.innerWidth - 292))}px`;
                accountMenu.style.top = `${bounds.bottom + 6}px`;
                this.run(() => this.renderAccountMenu());
            }
        });
        accountMenu.addEventListener('keydown', event => {
            const items = [...accountMenu.querySelectorAll('[role="menuitem"]')];
            const index = items.indexOf(document.activeElement);
            let next;
            if (event.key === 'ArrowDown') next = (index + 1) % items.length;
            else if (event.key === 'ArrowUp') next = (index - 1 + items.length) % items.length;
            else if (event.key === 'Home') next = 0;
            else if (event.key === 'End') next = items.length - 1;
            else if (event.key === 'Escape') {
                accountMenu.hidePopover();
                accountPicker.focus();
            }
            if (next !== undefined) items[next]?.focus();
            if (next !== undefined || event.key === 'Escape') event.preventDefault();
        });
        this.shell.folderBar.node.querySelector('.markdown-view-actions').addEventListener('click', (event) => {
            const target = event.target.closest('[data-markdown-view]');
            if (target) this.run(() => this.setMarkdownView(target.dataset.markdownView));
        });
        // 브랜치 표시를 누르면 Git 확장의 브랜치 전환 목록을 연다. 지금 작업 폴더의 저장소 경로를 넘겨 저장소를 다시 묻지 않게 한다.
        this.shell.folderBar.node.querySelector('.folder-bar-branch').addEventListener('click', () => this.run(async () => {
            const root = this.repositoryOf(this.shownFolder())?.provider.rootUri;
            await this.plugins.willStart;
            await this.commands.executeCommand('git.checkout', ...(root ? [new URI(root).path.fsPath()] : []));
        }));
        for (const strip of [this.shell.folderBar.node.querySelector('.folder-tabs'), this.shell.tabs.node]) {
            for (const scroller of strip.parentElement.querySelectorAll(':scope > .tabs-scroll')) {
                scroller.addEventListener('click', () => {
                    strip.scrollBy({ left: Number(scroller.dataset.direction) * strip.clientWidth * 0.8 });
                    this.alignTabsToEdge(strip);
                    this.updateTabOverflow(strip);
                });
            }
            strip.addEventListener('scroll', () => this.updateTabOverflow(strip), { passive: true });
            strip.addEventListener('keydown', event => tabOverflow.moveTabFocus(strip, event));
            // 창·사이드바 폭이 바뀌면 잘린 탭과 넘김 버튼을 맞춘다. 사용자가 다른 탭을 찾는 스크롤 위치는 유지한다.
            new ResizeObserver(() => this.updateTabOverflow(strip)).observe(strip);
        }
        this.shell.folderBar.node.querySelector('.folder-split-down').addEventListener('click', () => this.run(() => this.newTerminalHere({ split: 'split-bottom' })));
        this.shell.folderBar.node.querySelector('.folder-split-right').addEventListener('click', () => this.run(() => this.newTerminalHere({ split: 'split-right' })));
        const picker = this.shell.header.node.querySelector('.shell-picker');
        const menu = this.shell.header.node.querySelector('#shell-menu');
        menu.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.renderShellMenu();
                const bounds = picker.getBoundingClientRect();
                menu.style.left = `${Math.max(8, Math.min(bounds.left, window.innerWidth - 240))}px`;
                menu.style.top = `${bounds.bottom + 6}px`;
            }
        });
        const quickSettings = sidebar.querySelector('#quick-settings');
        quickSettings.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.renderQuickSettings();
                // 버튼 아래에 연다. 아래 공간이 모자라면 위로 열고, 창 가장자리를 넘지 않게 좌우를 당긴다.
                const bounds = document.querySelector('.quick-settings-button').getBoundingClientRect();
                const height = Math.min(quickSettings.scrollHeight || 510, window.innerHeight - 120);
                const opensUp = bounds.bottom + 6 + height > window.innerHeight && bounds.top - 6 - height >= 0;
                quickSettings.style.left = `${Math.max(8, Math.min(bounds.left, window.innerWidth - 352))}px`;
                quickSettings.style.top = `${opensUp ? bounds.top - 6 - height : bounds.bottom + 6}px`;
            }
        });
        // 열 때 현재 테마에 초점을 두어 키보드로도 바로 고를 수 있게 한다.
        quickSettings.addEventListener('toggle', (event) => {
            if (event.newState === 'open') quickSettings.querySelector('[data-setting="theme"]')?.focus({ preventScroll: true });
        });
        this.container.get(ThemeService).onDidColorThemeChange(() => {
            if (quickSettings.matches(':popover-open')) this.renderQuickSettings();
        });
        this.shell.footer.node.querySelector('.remote-indicator').addEventListener('click', () => this.run(() => this.commands.executeCommand('remote.select')));
        this.files.onDidFilesChange((event) => {
            // 파일 내용 변경은 행 표시를 바꾸지 않는다. 추가·삭제 때는 수정 시각의 정밀도에 기대지 않고 목록을 다시 읽는다.
            const changes = [...event.getAdded(), ...event.getDeleted()];
            if (changes.length) {
                this.n_directoryRevision += 1;
                for (const key of this.directoryEntries.keys()) {
                    const uri = new URI(key);
                    if (changes.some(change => uri.isEqual(change.resource.parent) || change.resource.isEqualOrParent(uri))) {
                        this.directoryEntries.delete(key);
                    }
                }
            }
            this.refreshSoon();
        });
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
        // 셸 메뉴에 운영체제 기본 셸의 실제 이름을 보이려고 백엔드의 기본 셸 경로를 읽어 둔다(Windows는 COMSPEC).
        this.systemShellPath = (await this.env.getValue('SHELL'))?.value || (await this.env.getValue('COMSPEC'))?.value || '';
        // WSL 터미널에 표지를 넘길 때 사용자가 이미 정해 둔 WSLENV를 지우지 않으려고 읽어 둔다.
        this.windowsWslEnv = (await this.env.getValue('WSLENV'))?.value || '';
        // 첫 터미널을 열기 전에 WSL 배포판이 있는지 확인한다. 없으면 기본 셸(WSL) 대신 대체 셸로 연다.
        if (OS.backend.type() === OS.Type.Windows) {
            const { ready, home } = await this.fetchJson('/paddock/wsl-ready').catch(() => ({ ready: false, home: '' }));
            this.isWslReady = ready === true;
            this.wslHomePath = home ? URI.fromFilePath(home.replace(/\\/g, '/')).path.toString() : '';
        }
        const saved = await this.storage.getData(STORAGE.WORK_FOLDERS, '');
        const liveIds = new Set(this.terminals.all.map(terminal => terminal.id));
        this.state = model.restore(saved || '', liveIds);
        const savedInner = await this.storage.getData(STORAGE.INNER_TABS, '{}');
        try {
            const groups = JSON.parse(savedInner || '{}');
            this.innerTerminalRoots = new Map(Object.entries(groups.terminals || {}).filter(([id, root]) => liveIds.has(id) && liveIds.has(root)));
            this.fileRoots = new Map(Object.entries(groups.files || {}).filter(([id, root]) => liveIds.has(root) || this.shell.getWidgetById(id)));
        } catch {
            this.innerTerminalRoots.clear();
            this.fileRoots.clear();
        }
        const shownWidgetId = await this.storage.getData(STORAGE.SHOWN_WIDGET, '');
        const restoredLayoutWidget = await this.restoreRootLayouts(shownWidgetId);
        this.isGroupsRestored = true;
        for (const terminal of this.terminals.all) {
            this.watchTerminal(terminal);
            if (this.shell.getAreaFor(terminal) !== 'main') {
                await this.shell.addWidget(terminal, { area: 'main' });
            }
        }
        // 저장된 작업 폴더나 파일 화면이 있어도 상단 줄에는 추가 터미널 하나를 둔다. 첫 화면에서 바로 명령을 입력할 수 있다.
        // 창을 닫을 때 보던 위젯의 묶음으로 시작한다. 그 위젯이 없으면 상단 첫 터미널이다.
        // Theia는 닫을 때의 활성 위젯을 늘 남기지 않아(창이 닫히며 초점이 빠짐), 닫기 직전에 직접 저장한 위젯을 쓴다.
        const restored = restoredLayoutWidget ?? this.shell.getWidgetById(shownWidgetId) ?? this.currentWidget();
        const extra = this.topTerminals()[0];
        if (!extra) await this.newExtraTerminal();
        else await this.activate((restored && this.rootOf(restored) ? restored : extra).id);
        this.selectedTopTerminal = this.currentWidget()?.id || null;
        this.selectedFolder = this.workFolderOf(this.currentWidget()) || this.state.folders[0]?.key || null;
        for (const folder of this.state.folders) {
            this.openRepository(folder.key);
        }
        // 켜자마자 입력할 수 있게 본문에 보이는 터미널에 포커스를 준다.
        const shown = this.shell.mainPanel.currentTitle?.owner;
        if (this.isTerminal(shown)) await this.activate(shown.id);
        this.showRootOf(this.currentWidget());
        this.bindKeys();
        this.layoutReady = true;
        void this.updateAccountLabels().catch(() => {});
        this.refreshRemote();
        setInterval(() => this.run(() => this.tick()), REFRESH_INTERVAL);
        void this.refreshWorkPresence();
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
        await this.storage.setData(STORAGE.WORK_FOLDERS, model.serialize(this.state));
        if (this.layoutReady) void this.refreshWorkPresence();
    }

    async saveInnerTabs() {
        await this.storage.setData(STORAGE.INNER_TABS, JSON.stringify({ terminals: Object.fromEntries(this.innerTerminalRoots), files: Object.fromEntries(this.fileRoots) }));
    }

    /** 저장된 묶음별 비율을 되살린 위젯에 연결한다. 복원되지 않는 화면만 빼고 나머지 칸 비율은 유지한다. */
    async restoreRootLayouts(
        shownWidgetId,
    ) {
        const saved = await this.storage.getData(STORAGE.ROOT_LAYOUTS, []).catch(() => []);
        const widgets = this.shell.widgets.filter(widget => !widget.isDisposed);
        const used = new Set();
        let filesChanged = false;
        let shownWidget;
        const resolveWidget = reference => {
            let widget = widgets.find(item => item.id === reference.id);
            if (!widget && reference.uri) {
                // 이미지 등 확장 편집기는 재시작 때 id가 바뀐다. 같은 파일·같은 편집기인 화면 하나만 연결한다.
                const matches = widgets.filter(item => item.getResourceUri?.()?.toString() === reference.uri
                    && (item.viewType || '') === (reference.viewType || ''));
                if (matches.length === 1) widget = matches[0];
            }
            if (widget && used.has(widget)) widget = undefined;
            if (widget) {
                used.add(widget);
                if (reference.id === shownWidgetId) shownWidget = widget;
                if (widget.id !== reference.id && this.fileRoots.has(reference.id)) {
                    this.fileRoots.set(widget.id, this.fileRoots.get(reference.id));
                    this.fileRoots.delete(reference.id);
                    filesChanged = true;
                }
            }
            return widget;
        };
        if (Array.isArray(saved)) {
            for (const entry of saved) {
                if (Array.isArray(entry) && typeof entry[0] === 'string') {
                    const area = layoutModel.restore(entry[1], resolveWidget);
                    if (area) {
                        const root = entry[0];
                        this.rootLayouts.set(root, area);
                        for (const widget of layoutModel.widgetsOf(area)) {
                            if (root.startsWith('top:') && !this.isTerminal(widget) && this.fileRoots.get(widget.id) !== root.slice(4)) {
                                this.fileRoots.set(widget.id, root.slice(4));
                                filesChanged = true;
                            } else if (root.startsWith('folder:') && widget instanceof WebviewWidget) {
                                this.webviewFolders.set(widget.id, root.slice(7));
                            }
                        }
                    }
                }
            }
        }
        this.isRestoringRootLayouts = this.rootLayouts.size > 0;
        if (filesChanged) await this.saveInnerTabs();
        return shownWidget;
    }

    /** 다른 창에는 작업 폴더와 터미널의 이름·소속·프로그램만 알린다. 화면과 입력은 보내지 않는다. */
    async refreshWorkPresence() {
        try {
            const terminals = Object.entries(this.state.terminals).map(([id, entry]) => {
                const widget = this.shell.getWidgetById(id);
                return { id, folder: entry.folder, name: entry.name, program: widget ? this.programOf(widget) : '' };
            });
            const url = new Endpoint({ path: '/paddock/work-presence' }).getRestUrl().toString();
            const response = await fetch(url, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ windowId: this.workWindowId, folders: this.state.folders, terminals }),
                signal: AbortSignal.timeout(3000),
            });
            if (!response.ok) throw new Error(`Work list request failed: ${response.status}`);
            const snapshots = await response.json();
            if (JSON.stringify(snapshots) !== JSON.stringify(this.remoteWorkSnapshots)) {
                this.remoteWorkSnapshots = snapshots;
                this.refreshSoon();
            }
        } catch {
            // 백엔드가 잠시 연결되지 않으면 다음 주기에 다시 목록을 맞춘다.
        }
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
        if (this.pointerPressed) {
            this.refreshAfterPointer = true;
        } else {
            this.renderTabs();
            this.renderFolderTabs();
            this.renderPathBars();
            this.renderStatus();
            this.renderViewBadge();
            if (this.view === 'work') await this.renderWork();
        }
    }

    /** 이벤트가 없는 값(터미널 현재 폴더·프로그램·메모리)을 주기적으로 다시 읽는다. */
    async tick() {
        await this.refreshWslTerminals();
        await Promise.all(this.terminals.all.map(terminal => this.readCwd(terminal)));
        await this.refreshPrograms();
        await this.promoteAgents();
        this.checkAgents();
        this.showRootOf(this.currentWidget());
        void this.refreshWorkPresence();
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
            terminal.onData((data) => {
                this.activity.set(terminal.id, agent.noteInput(this.activity.get(terminal.id) ?? agent.idle(), data, Date.now()));
                if (data.includes('\r')) {
                    // 새 요청의 상태가 이전 응답의 완료 알림에 가리지 않게 한다.
                    this.doneIds.delete(terminal.id);
                    this.refreshSoon();
                }
            });
            // 입력 직후 첫 출력은 동기로 처리되어 Theia의 출력 수집보다 먼저 알림이 온다.
            // 화면에 파싱된 출력을 직접 관찰하면 그 첫 출력과 시작할 때 모아 둔 출력도 빠뜨리지 않는다.
            terminal.term.onWriteParsed(() => {
                const now = Date.now();
                const before = this.activity.get(terminal.id) ?? agent.idle();
                const after = agent.noteOutput(before, now);
                this.activity.set(terminal.id, after);
                // 첫 출력과 잠시 멎었다가 재개된 출력은 즉시 표시한다. 계속되는 출력은 기존 주기 갱신에 맡긴다.
                const agentNow = agent.isAgent(this.programs.get(terminal.id));
                if (agent.activityState(before, now, agentNow) !== agent.activityState(after, now, agentNow)) this.refreshSoon();
            });
            if (OS.backend.type() === OS.Type.Windows && cwdReport.isBashShell(terminal.options?.shellPath)) this.absorbResizeKeyLoss(terminal);
            // 셸이 프롬프트마다 알리는 현재 폴더(OSC 7)를 기억한다. Windows 셸은 createTerminal이 이 알림을 켠다.
            terminal.term?.parser?.registerOscHandler(7, (data) => {
                const reported = cwdReport.parseCwdReport(data);
                if (reported) this.reportedCwds.set(terminal.id, URI.fromFilePath(reported).toString());
                return true;
            });
            terminal.onDidDispose?.(() => {
                this.reportedCwds.delete(terminal.id);
                this.activity.delete(terminal.id);
                this.programs.delete(terminal.id);
                this.doneIds.delete(terminal.id);
                // 다른 묶음에 가려져 본문에서 빠진 터미널은 패널의 제거 이벤트 없이 종료될 수 있다.
                this.forgetClosedWidget(terminal);
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
        // Windows 백엔드는 앞쪽 프로그램을 모르므로 WSL 터미널은 WSL 안에서 읽은 프로그램을 쓴다.
        for (const terminal of terminals) {
            const program = this.wslTerminals[terminal.id]?.program;
            if (program) this.programs.set(terminal.id, program);
        }
    }

    /** WSL 터미널이 있으면 WSL 안의 현재 폴더·실행 프로그램을 한 번에 읽어 둔다. 실패하면 지난 값을 둔다. */
    async refreshWslTerminals() {
        if (this.terminals.all.some(terminal => wsl.isWslShell(terminal.options?.shellPath))) {
            try {
                this.wslTerminals = await this.fetchJson('/paddock/wsl-terminals');
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
        const { state, promoted, added } = model.promoteAgentTerminals(this.state, this.terminals.all.map((terminal) => {
            const program = this.programs.get(terminal.id);
            return { id: terminal.id, cwd: this.cwdCache.get(terminal.id), program, isAgent: agent.isAgent(program) };
        }));
        if (promoted.length) {
            this.state = state;
            let innerChanged = false;
            for (const id of promoted) {
                const terminal = this.terminals.all.find(item => item.id === id);
                if (this.accountLabel(terminal)) this.state = model.renameTerminal(this.state, id, this.accountLabel(terminal));
                innerChanged = this.innerTerminalRoots.delete(id) || innerChanged;
                const folder = model.folderOf(this.state, id);
                for (const [childId, root] of this.innerTerminalRoots) {
                    if (root === id) {
                        const n_existing = model.terminalsOf(this.state, folder).length;
                        this.state = model.assignTerminal(this.state, childId, folder, `terminal ${n_existing + 1}`);
                        innerChanged = this.innerTerminalRoots.delete(childId) || innerChanged;
                    }
                }
            }
            if (innerChanged) await this.saveInnerTabs();
            await this.save();
            for (const key of added) {
                this.openRepository(key);
            }
            const current = this.currentWidget();
            if (current && promoted.includes(current.id)) this.selectedFolder = model.folderOf(this.state, current.id);
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
     * 지금 선택한 본문 위젯. 파일 화면이 입력 초점을 받지 않아도 선택된 탭을 따른다.
     * 사이드바를 누르면 포커스가 본문을 떠나므로, 선택된 탭이 없을 때는 본문에서 마지막으로 활성이던 위젯을 쓴다.
     */
    currentWidget() {
        let current = this.shell.mainPanel.currentTitle?.owner;
        const last = this.lastMainWidget;
        const alive = last && !last.isDisposed && this.shell.getAreaFor(last) === 'main';
        if (!current || current.isDisposed || this.shell.getAreaFor(current) !== 'main') {
            current = alive ? last : undefined;
        }
        return current;
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
        const inWsl = this.wslTerminals[terminal.id]?.cwd;
        const reported = this.reportedCwds.get(terminal.id);
        try {
            // Windows는 터미널 프로세스의 처음 폴더만 알려 준다. WSL 터미널은 WSL 안에서 읽은 폴더를,
            // Git Bash·PowerShell·명령 프롬프트는 셸이 프롬프트마다 알린 폴더를 쓴다.
            if (inWsl) cwd = URI.fromFilePath(inWsl.replace(/\\/g, '/')).toString();
            else if (reported) cwd = reported;
            else cwd = (await terminal.cwd).toString();
        } catch {
            // 셸이 막 끝났거나 현재 폴더를 보고하지 않으면 마지막으로 알던 폴더를 쓴다.
            cwd = cwd || terminal.lastCwd?.toString();
        }
        if (cwd) this.cwdCache.set(terminal.id, cwd);
        return cwd;
    }

    /**
     * 위젯이 속한 작업 폴더. 작업 터미널이면 그 폴더, 파일이면 그 파일을 품은 작업 폴더, 그 밖(추가 터미널 등)은 null.
     */
    workFolderOf(
        widget,
    ) {
        let folder = null;
        if (widget && this.isTerminal(widget)) {
            folder = model.folderOf(this.state, widget.id);
        } else if (widget?.getResourceUri?.()) {
            folder = model.folderContaining(this.state, widget.getResourceUri().toString());
        }
        return folder;
    }

    /** 아래 탭 줄에 보일 작업 폴더. 위쪽 터미널을 보면 마지막 작업 폴더가 남아 있어도 그 폴더 탭은 비운다. */
    shownFolder() {
        const current = this.currentWidget();
        const key = this.workFolderOf(current) || (this.isTerminal(current) || current?.getResourceUri?.() ? null : this.selectedFolder);
        return key && this.state.folders.some(folder => folder.key === key) ? key : null;
    }

    /** 위쪽 터미널 묶음의 시작 터미널. 내부 터미널·파일을 선택해도 같은 묶음을 가리킨다. */
    /**
     * 상단 줄 터미널(작업 폴더·묶음 안 탭이 아닌 터미널)을 만든 순서대로 돌려준다.
     * `terminals.all`은 재시작 때 되살린 순서라 창을 다시 열면 탭 순서가 바뀐다. 만든 시각은 배치 저장에 함께 남아 순서가 유지된다.
     * 만든 시각이 없는 터미널(확장이 연 터미널 등)은 뒤에 원래 순서대로 둔다.
     */
    topTerminals() {
        const createdAt = terminal => {
            const [time, counter] = String(terminal.options?.created ?? '').split('-').map(Number);
            return Number.isFinite(time) && Number.isFinite(counter) ? [time, counter] : [Infinity, 0];
        };
        return this.terminals.all
            .filter(terminal => !model.folderOf(this.state, terminal.id) && !this.innerTerminalRoots.has(terminal.id))
            .map(terminal => ({ terminal, order: createdAt(terminal) }))
            .sort((left, right) => left.order[0] - right.order[0] || left.order[1] - right.order[1])
            .map(({ terminal }) => terminal);
    }

    topTerminalOf(
        widget,
    ) {
        let root = null;
        if (widget && this.isTerminal(widget) && !model.folderOf(this.state, widget.id)) {
            root = this.innerTerminalRoots.get(widget.id) || widget.id;
        } else if (widget) {
            root = this.fileRoots.get(widget.id) || null;
        }
        // 시작 터미널이 닫혀도 파일은 원래 묶음의 배치로 돌아갈 수 있다.
        return root && !model.folderOf(this.state, root) && (!this.isTerminal(widget) || this.terminals.all.some(terminal => terminal.id === root)) ? root : null;
    }

    /** 시작 터미널을 닫고 파일만 남은 묶음인지. 파일 탭과 새 터미널 진입을 계속 보여 주는 데 쓴다. */
    isFileOnlyGroup(
        widget,
    ) {
        const root = widget && this.fileRoots.get(widget.id);
        return Boolean(root && !this.workFolderOf(widget) && !this.terminals.all.some(terminal => terminal.id === root));
    }

    /** 닫힌 화면의 소속을 정리한다. 화면에서 가려 둔 터미널의 종료도 같은 정리를 거친다. */
    forgetClosedWidget(
        widget,
    ) {
        // 다른 묶음의 배치로 바꾸는 중에 화면에서 빠진 위젯은 닫힌 것이 아니라 소속을 그대로 둔다.
        if (!this.isSwitchingRoot) {
            const nextRoot = this.isTerminal(widget) ? this.keepGroupAfterTerminalClose(widget.id) : null;
            if (model.folderOf(this.state, widget.id)) {
                this.state = model.forgetTerminal(this.state, widget.id);
                void this.save();
            }
            const removedInner = this.innerTerminalRoots.delete(widget.id);
            const removedFile = this.fileRoots.delete(widget.id);
            this.webviewFolders.delete(widget.id);
            this.markdownPreviewSources.delete(widget.id);
            let innerChanged = removedInner || removedFile;
            if (widget.id === this.selectedTopTerminal) this.selectedTopTerminal = nextRoot;
            if (this.isTerminal(widget)) {
                for (const [id, root] of this.innerTerminalRoots) {
                    if (root === widget.id) innerChanged = this.innerTerminalRoots.delete(id) || innerChanged;
                }
                // 파일만 남은 묶음은 닫힌 터미널의 id로 배치와 파일 연결을 유지한다. 새 셸을 자동으로 만들지 않는다.
            }
            if (innerChanged) void this.saveInnerTabs();
            if (widget.id === PreferencesWidget.ID) {
                this.rootLayouts.delete(PreferencesWidget.ID);
                // 설정은 닫아도 위젯 객체가 남는다. 패널의 제거 처리가 끝난 다음 작업 배치를 복원한다.
                queueMicrotask(() => this.run(() => this.restoreWorkAfterPreferences()));
            }
            this.refreshSoon();
        }
    }

    /** 전체 설정을 닫으면 이전 작업으로 돌아간다. 그 작업이 모두 끝났으면 남아 있는 다른 작업을 보여 준다. */
    async restoreWorkAfterPreferences() {
        if (this.displayedRoot === PreferencesWidget.ID) {
            const previous = this.preferencesReturnWidget;
            const previousRoot = previous && !previous.isDisposed ? this.rootOf(previous) : this.preferencesReturnRoot;
            const candidates = [previousRoot, ...this.rootLayouts.keys()].filter(root => root && root !== PreferencesWidget.ID);
            const root = candidates.find(name => layoutModel.widgetsOf(layoutModel.withoutDisposed(this.rootLayouts.get(name))).length);
            const area = root ? layoutModel.withoutDisposed(this.rootLayouts.get(root)) : null;
            const widget = previous && layoutModel.includes(area, previous) ? previous : layoutModel.widgetsOf(area)[0];
            if (widget) {
                this.showRoot(root, widget);
                await this.activate(widget.id);
            } else {
                this.displayedRoot = null;
            }
            this.refreshSoon();
        }
    }

    /**
     * 시작 터미널이 닫히면 첫 내부 터미널이 나머지 터미널·파일·칸 배치를 이어받는다.
     * 내부 터미널이 없으면 파일 연결은 그대로 두어 파일만 남은 묶음으로 다시 열 수 있다.
     */
    keepGroupAfterTerminalClose(
        id,
    ) {
        const children = [...this.innerTerminalRoots].filter(([child, root]) => {
            const widget = this.shell.getWidgetById(child);
            return root === id && widget && !widget.isDisposed;
        });
        const nextRoot = children[0]?.[0] || null;
        if (nextRoot) {
            for (const [child] of children) {
                if (child === nextRoot) this.innerTerminalRoots.delete(child);
                else this.innerTerminalRoots.set(child, nextRoot);
            }
            this.moveFileGroup(id, nextRoot);
            void this.saveInnerTabs();
        }
        return nextRoot;
    }

    /** 파일 연결과 보관한 칸 배치를 새 시작 터미널로 옮긴다. 닫힌 시작 터미널은 배치에서 뺀다. */
    moveFileGroup(
        previousRoot,
        nextRoot,
    ) {
        for (const [id, root] of this.fileRoots) {
            if (root === previousRoot) this.fileRoots.set(id, nextRoot);
        }
        const previousKey = `top:${previousRoot}`;
        const nextKey = `top:${nextRoot}`;
        const area = this.displayedRoot === previousKey ? this.shell.mainPanel.saveLayout().main : this.rootLayouts.get(previousKey);
        const kept = layoutModel.prune(area, widget => widget.id !== previousRoot && !widget.isDisposed);
        if (kept) this.rootLayouts.set(nextKey, kept);
        this.rootLayouts.delete(previousKey);
        if (this.displayedRoot === previousKey) this.displayedRoot = nextKey;
        if (this.preferencesReturnRoot === previousKey) this.preferencesReturnRoot = nextKey;
    }

    /** 현재 화면에 해당하는 위쪽 터미널 묶음. 작업 폴더를 보고 있으면 해당 묶음은 표시하지 않는다. */
    shownTopTerminal() {
        const current = this.currentWidget();
        const root = this.shownFolder() ? null : this.topTerminalOf(current) || this.selectedTopTerminal;
        return root && !model.folderOf(this.state, root) && this.terminals.all.some(terminal => terminal.id === root) ? root : null;
    }

    /** 셸 프로필의 실행 파일이 절대 경로로 적혀 있는데 그 파일이 없으면 true. 이름만 적힌 셸은 PATH에서 찾으므로 false다. */
    async isShellMissing(
        profile,
    ) {
        const shellPath = profile instanceof ShellTerminalProfile ? profile.shellPath ?? '' : '';
        let missing = false;
        if (/^(\/|[A-Za-z]:[\\/])/.test(shellPath)) {
            const executable = await this.files.resolve(URI.fromFilePath(shellPath)).catch(() => undefined);
            missing = !executable?.isFile;
        }
        return missing;
    }

    /** 이 PC에서 열 수 있는 셸 프로필인지. 실행 파일이 없거나, WSL 배포판이 하나도 없는데 WSL 셸이면 false다. */
    async isShellUsable(
        profile,
    ) {
        const isWsl = profile instanceof ShellTerminalProfile && wsl.isWslShell(profile.shellPath);
        return Boolean(profile) && profile !== NULL_PROFILE && !(await this.isShellMissing(profile)) && (!isWsl || this.isWslReady);
    }

    /** 셸 프로필로 터미널을 만든다. 셸은 아직 시작하지 않았다 — 소속을 정한 뒤 openTerminal로 열면 시작된다. */
    async createTerminal(
        cwd,
        profileChoice,
    ) {
        // Theia는 셸 프로필 설정을 비동기로 합친다. 첫 실행처럼 일찍 부르면 기본 프로필(Windows는 WSL)이 아직 정해지지 않아
        // 다른 셸로 열리므로, 합치기가 끝난 뒤 기본 프로필을 읽는다.
        await this.terminals.mergePreferencesPromise;
        // WSL 안의 폴더(\\wsl.localhost\…)에서 여는 터미널은 셸을 따로 고르지 않았으면 WSL 셸로 연다. 그 폴더의 도구는 WSL에 있다.
        const wslProfile = [...this.profiles.all].find(([, item]) => item instanceof ShellTerminalProfile && wsl.isWslShell(item.shellPath))?.[1];
        const isWslFolder = new URI(cwd).authority.toLowerCase() === 'wsl.localhost';
        const requested = profileChoice ?? (isWslFolder && wslProfile ? wslProfile : this.profiles.defaultProfile);
        let terminal;
        let profile = requested;
        // 기본 셸(Windows는 WSL)을 이 PC에서 열 수 없으면 오류 대신 대체 프로필(Git Bash → PowerShell)로 연다. 사용자가 고른 셸은 바꾸지 않는다.
        if (requested === this.profiles.defaultProfile && !(await this.isShellUsable(requested))) {
            for (const id of FALLBACK_PROFILES) {
                const fallback = this.profiles.getProfile(id);
                if (profile === requested && fallback && await this.isShellUsable(fallback)) profile = fallback;
            }
        }
        if (profile && profile !== NULL_PROFILE && profile instanceof ShellTerminalProfile) {
            let selected = profile.modify({ cwd, title: profile.shellPath?.split(/[\\/]/).pop() });
            if (wsl.isWslShell(profile.shellPath)) {
                // WSL 안의 셸에 이 터미널의 id를 표지로 넘겨, 주기 갱신이 WSL 안의 현재 폴더·실행 프로그램을 찾게 한다.
                // Windows 홈에서 열면 WSL도 Linux 홈에서 시작한다. 작업 폴더(\\wsl.localhost\…)에서 열면 wsl.exe가 그 폴더로 옮겨 준다.
                const id = `terminal-${crypto.randomUUID()}`;
                const isWindowsHome = new URI(cwd).path.toString() === this.homePath;
                selected = selected.modify({
                    id,
                    env: { ...selected.options.env, [wsl.MARKER]: id, WSLENV: [this.windowsWslEnv, wsl.MARKER].filter(Boolean).join(':') },
                    shellArgs: isWindowsHome ? ['--cd', '~', ...(selected.options.shellArgs || [])] : selected.options.shellArgs,
                });
            } else if (OS.backend.type() === OS.Type.Windows) {
                // Windows 셸은 현재 폴더를 알 방법이 없어, 프롬프트마다 셸이 직접 알리게 한다(OSC 7).
                const report = cwdReport.cwdReportOptions(profile.shellPath, selected.options.shellArgs || []);
                if (report) selected = selected.modify({ env: { ...selected.options.env, ...report.env }, shellArgs: report.shellArgs });
            }
            // 절대 경로(/bin/zsh, C:\...\pwsh.exe)만 미리 확인한다. 이름만 적힌 셸은 PATH에서 찾는다.
            if (await this.isShellMissing(profile)) {
                throw new Error(`Cannot start shell: ${selected.shellPath}. Check the executable path.`);
            }
            terminal = await this.terminals.newTerminal(selected.options);
        } else if (profile && profile !== NULL_PROFILE) {
            terminal = await profile.start();
        } else {
            terminal = await this.terminals.newTerminal({ cwd });
        }
        return terminal;
    }

    /**
     * 만든 터미널을 본문에 열고, 칸 크기에 맞춘 뒤 셸을 시작한다. 셸을 시작하지 못하면 터미널을 닫고 오류를 낸다.
     * 기본 크기(80x24)로 셸을 띄운 뒤 칸에 맞춰 줄이면 셸이 시작하는 도중에 크기 변경 신호를 받는다 — Git Bash는 그 뒤 처음 친 글자를 잃었다.
     * 이미 시작된 터미널(셸이 아닌 프로필이 연 터미널)은 열기만 한다.
     */
    async openTerminal(
        terminal,
        openOptions,
    ) {
        await this.terminals.open(terminal, openOptions);
        await this.balanceSplit(terminal, openOptions.widgetOptions?.mode);
        // 시작 전 터미널의 id는 -1이다.
        if (!(terminal.terminalId >= 0)) {
            terminal.doResizeTerminal?.();
            try {
                await terminal.start();
            } catch (error) {
                terminal.dispose();
                throw new Error('Cannot start the shell. Check the executable path.', { cause: error });
            }
        }
    }

    /** 새 터미널이나 미리보기 칸을 추가한 분할에 같은 공간을 준다. 기존 칸을 다시 볼 때는 부르지 않는다. */
    async balanceSplit(
        widget,
        mode,
    ) {
        if (mode === 'split-right' || mode === 'split-bottom') {
            // 세 번째 이후 칸도 명령·파일을 읽을 공간을 갖게 새로 나눈 형제 칸만 고르게 배분한다.
            // 배치를 옮기는 동안 발생하는 제거 알림은 실제 터미널·파일 종료가 아니다.
            const layout = this.shell.mainPanel.saveLayout();
            const main = layoutModel.balanceNewSplit(layout.main, widget, mode === 'split-right' ? 'horizontal' : 'vertical');
            this.isSwitchingRoot = true;
            try {
                this.shell.mainPanel.restoreLayout({ main });
            } finally {
                this.isSwitchingRoot = false;
            }
            await this.shell.activateWidget(widget.id);
        }
    }

    /**
     * Git Bash(Windows)에서 터미널 크기가 바뀐 뒤 사용자가 처음 친 키가 사라지지 않게 한다.
     * Git Bash는 크기 변경 신호를 바로 처리하지 않고 다음 키를 읽을 때 처리하는데, Windows 콘솔에서는 이때 그 키가 함께 사라진다.
     * 터미널은 처음 열릴 때 화면에 맞춰 크기가 바뀌므로, 창이 뜨자마자 친 첫 글자가 사라졌다. 칸 나누기·창 크기 조절 뒤에도 같다.
     * 크기가 바뀔 때마다 잠시 뒤 빈 키(NUL)를 먼저 보내 그 키가 대신 사라지게 한다. 사라지지 않고 읽혀도 NUL은 입력 줄 편집기에서
     * 표시 위치만 기억하는 키(set-mark)라 화면과 입력에 흔적이 없다.
     */
    absorbResizeKeyLoss(
        terminal,
    ) {
        const resize = terminal.resizeTerminalProcess.bind(terminal);
        let size = '';
        let timer = null;
        terminal.resizeTerminalProcess = () => {
            resize();
            const next = `${terminal.term.cols}x${terminal.term.rows}`;
            if (next !== size) {
                size = next;
                clearTimeout(timer);
                timer = setTimeout(() => terminal.waitForConnection?.promise.then(connection => connection.getWriteBuffer().writeString('\0').commit()), RESIZE_KEY_GUARD_MS);
            }
        };
    }

    /**
     * 내부 터미널이 있는 묶음의 본문 탭 순서. 처음 연 터미널도 새 터미널 옆에서 다시 선택할 수 있게 포함한다.
     * 내부 터미널이 없으면 빈 목록이다. 그때는 맨 위 탭만으로 처음 연 터미널을 선택한다.
     */
    innerTabIds(
        root,
    ) {
        const children = [...this.innerTerminalRoots].filter(([, owner]) => owner === root).map(([id]) => id);
        const ids = children.length ? [root, ...children].filter(id => this.terminals.all.some(terminal => terminal.id === id)) : [];
        return ids;
    }

    /** 현재 터미널과 같은 묶음의 터미널 id. 작업 폴더·내부 탭·상단 탭 중 한 줄의 순서다. */
    siblingTerminals(
        terminal,
    ) {
        const folder = model.folderOf(this.state, terminal.id);
        const root = this.innerTerminalRoots.get(terminal.id) || terminal.id;
        const inner = this.innerTabIds(root);
        let ids;
        if (folder) ids = model.terminalRows(this.state, folder).map(row => row.id);
        else if (inner.length) ids = inner;
        else ids = this.topTerminals().map(item => item.id);
        return ids;
    }

    /** 지금 보는 터미널에서 같은 줄의 이전·다음·처음·마지막 탭으로 옮긴다. 터미널이 아니면 아무것도 하지 않는다. */
    async moveTab(
        target,
    ) {
        const current = this.currentWidget();
        const id = this.isTerminal(current) ? model.tabTarget(this.siblingTerminals(current), current.id, target) : null;
        if (id && id !== current.id) await this.activate(id);
    }

    /**
     * 지금 보는 곳에서 새 터미널을 연다(나누기 버튼·단축키).
     * 작업 폴더에 속한 것을 보고 있으면 그 폴더의 작업 터미널, 아니면 현재 상단 터미널 묶음의 내부 터미널이다.
     */
    async newTerminalHere(
        { split } = {},
    ) {
        const folderKey = this.workFolderOf(this.currentWidget());
        const terminal = folderKey ? await this.newWorkTerminal({ folderKey, split }) : await this.newInnerTerminal({ split });
        return terminal;
    }

    /** 본문 위 탭 줄의 ＋: 작업 폴더가 보이면 그 안에, 아니면 현재 터미널의 폴더에 추가 터미널을 연다. */
    async newTerminalFromFolderBar() {
        const folderKey = this.shownFolder();
        if (folderKey) {
            await this.newWorkTerminal({ folderKey });
        } else {
            await this.newInnerTerminal();
        }
    }

    /** Shows labels chosen by the user; the official CLI owns login and account identity. */
    async manageAccounts() {
        const isWindows = OS.backend.type() === OS.Type.Windows;
        const runtimeOptions = [{ value: 'native', label: isWindows ? 'Windows' : 'This device' }];
        if (isWindows && this.isWslReady) runtimeOptions.push({ value: 'wsl', label: 'WSL' });
        const current = this.currentWidget();
        const prefersWsl = isWindows && this.isWslReady && (wsl.isWslShell(current?.options?.shellPath)
            || wsl.isWslShell(this.profiles.defaultProfile?.shellPath));
        let opened;
        await new AccountDialog({
            service: this.accounts,
            runtimeOptions,
            defaultRuntime: prefersWsl ? 'wsl' : 'native',
            onOpen: async profile => { opened = await this.openAccount(profile.id); },
        }).open();
        await this.updateAccountLabels().catch(() => {});
        await this.usagePanel.data.updateProfiles();
        void this.refreshUsage();
        // Dialog disposal restores the previous focus. Activate the new terminal only after that finishes.
        if (opened && !opened.isDisposed) await this.activate(opened.id);
        this.refreshSoon();
    }

    async updateAccountLabels() {
        const profiles = await this.accounts.list();
        let changed = false;
        for (const terminal of this.terminals.all) {
            const previous = terminal.options?.paddockAccount;
            const current = previous && profiles.find(profile => profile.id === previous.id);
            if (current && current.label !== previous.label) {
                const oldLabel = this.accountLabel(terminal);
                Object.assign(previous, current);
                changed = true;
                if (this.state.terminals[terminal.id]?.name === oldLabel) {
                    this.state = model.renameTerminal(this.state, terminal.id, this.accountLabel(terminal));
                }
            }
        }
        if (changed) {
            await this.save();
            this.refreshSoon();
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
            terminal = await this.newTerminalHere();
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

    /** Opens the chosen environment beside the current terminal without changing its process or history. */
    async openAccount(
        id,
    ) {
        const current = this.currentWidget();
        const folderKey = this.shownFolder();
        const root = this.shownTopTerminal();
        const fileRoot = this.isFileOnlyGroup(current) ? this.fileRoots.get(current.id) : null;
        const cwd = (this.isTerminal(current) ? await this.readCwd(current) : current?.getResourceUri?.()?.parent?.toString())
            || folderKey || `file://${this.homePath}`;
        const directory = await this.files.resolve(new URI(cwd)).catch(() => undefined);
        if (!directory?.isDirectory) throw new Error('The current folder is unavailable. Open an existing folder and try the account again.');
        const prepared = await this.accounts.prepare(id);
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
            } else if (uri.path.toString() === this.homePath) {
                launchCwd = '~';
            } else {
                launchCwd = uri.path.fsPath();
            }
        } else if (isWindows && ['wsl.localhost', 'wsl$'].includes(uri.authority.toLowerCase())) {
            throw new Error('Choose a WSL account for this Linux folder, or open a Windows folder first.');
        }
        const options = accountTerminalOptions(prepared, { cwd: launchCwd, isWindows, wslEnv: this.windowsWslEnv });
        const shellFile = await this.files.resolve(URI.fromFilePath(options.shellPath)).catch(() => undefined);
        if (!shellFile?.isFile) throw new Error('The account shell is unavailable. Install Bash, PowerShell, or WSL for the selected environment and try again.');
        options.cwd = cwd;
        if (prepared.profile.runtime === 'wsl') {
            options.id = `terminal-${crypto.randomUUID()}`;
            options.env[wsl.MARKER] = options.id;
            options.env.WSLENV = [this.windowsWslEnv, wsl.MARKER].filter(Boolean).join(':');
        }
        const terminal = await this.terminals.newTerminal(options);
        if (folderKey) {
            this.state = model.assignTerminal(this.state, terminal.id, folderKey, options.title);
            this.cwdCache.set(terminal.id, cwd);
            await this.save();
        } else if (root) {
            this.innerTerminalRoots.set(terminal.id, root);
            await this.saveInnerTabs();
        } else if (fileRoot) {
            this.moveFileGroup(fileRoot, terminal.id);
            await this.saveInnerTabs();
        }
        const widgetOptions = current && !current.isDisposed
            ? { area: 'main', mode: fileRoot ? 'split-left' : 'tab-after', ref: current }
            : { area: 'main' };
        await this.openTerminal(terminal, { widgetOptions, mode: 'activate' });
        for (const warning of prepared.warnings || []) void this.messages.warn(warning);
        await this.refresh();
        return terminal;
    }

    async renderAccountMenu() {
        const menu = this.shell.folderBar.node.querySelector('#account-menu');
        menu.replaceChildren(element('p', 'account-menu-hint', 'Loading accounts…'));
        try {
            const profiles = await this.updateAccountLabels();
            menu.replaceChildren(element('p', 'account-menu-hint', 'Open a new terminal'));
            for (const profile of profiles) {
                const item = button([
                    element('span', 'shell-option-name', `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`),
                    element('span', 'shell-option-meta', profile.runtime === 'wsl' ? 'WSL' : ''),
                ], 'shell-option', () => {
                    menu.hidePopover();
                    this.run(() => this.openAccount(profile.id));
                });
                item.title = `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`;
                item.setAttribute('role', 'menuitem');
                menu.append(item);
            }
        } catch {
            menu.replaceChildren(element('p', 'account-menu-hint', 'Accounts could not be loaded. Open Manage accounts to retry.'));
        }
        const manage = button('Manage accounts…', 'shell-option account-menu-manage', () => {
            menu.hidePopover();
            this.run(() => this.manageAccounts());
        });
        manage.setAttribute('role', 'menuitem');
        menu.append(manage);
        if (menu.matches(':popover-open')) menu.querySelector('button')?.focus({ preventScroll: true });
    }

    /** 아래 ＋로 위쪽 터미널 묶음 안에 터미널을 연다. 같은 묶음의 터미널과 파일 탭 옆에 나타난다. */
    async newInnerTerminal(
        { split } = {},
    ) {
        const current = this.currentWidget();
        const root = this.shownTopTerminal();
        const fileRoot = this.isFileOnlyGroup(current) ? this.fileRoots.get(current.id) : null;
        const cwd = this.isTerminal(current) ? await this.readCwd(current) : current?.getResourceUri?.()?.parent?.toString();
        const terminal = await this.createTerminal(cwd || `file://${this.homePath}`);
        if (root) this.innerTerminalRoots.set(terminal.id, root);
        else if (fileRoot) this.moveFileGroup(fileRoot, terminal.id);
        await this.saveInnerTabs();
        // 파일만 남아 있으면 그 왼쪽에 새 시작 터미널을 두어 파일과 실행 화면을 함께 유지한다.
        const widgetOptions = fileRoot ? { area: 'main', mode: split || 'split-left', ref: current } : this.widgetOptions(split);
        await this.openTerminal(terminal, { widgetOptions, mode: 'activate' });
        await this.refresh();
        return terminal;
    }

    /** 나눠 열 때 기준 칸. 나누지 않으면 본문의 새 탭으로 연다. */
    widgetOptions(
        split,
    ) {
        const current = this.currentWidget();
        return split && current ? { area: 'main', mode: split, ref: current } : { area: 'main' };
    }

    /**
     * 작업 폴더와 무관한 새 상단 터미널을 맨 위 줄에 연다.
     * 상단 ＋와 셸 선택에서 열면 홈에서 시작한다. 분할 옵션이 있으면 현재 터미널의 폴더를 따른다.
     */
    async newExtraTerminal(
        { profile, split, cwd } = {},
    ) {
        const current = this.currentWidget();
        const nearby = cwd || (split && this.isTerminal(current) ? await this.readCwd(current) : null);
        const terminal = await this.createTerminal(nearby || `file://${this.homePath}`, profile);
        await this.openTerminal(terminal, { widgetOptions: this.widgetOptions(split), mode: 'activate' });
        await this.refresh();
    }

    /**
     * 이미 목록에 있는 작업 폴더(`folderKey`)에 새 작업 터미널을 연다.
     * `split`이 있으면 지금 보는 칸을 아래(`split-bottom`)나 옆(`split-right`)으로 나눠 연다.
     */
    async newWorkTerminal(
        { folderKey, split },
    ) {
        if (!folderKey) {
            throw new Error('Choose a work folder first. Run claude or codex in a terminal to add its folder.');
        }
        const terminal = await this.createTerminal(folderKey);
        const n_existing = model.terminalsOf(this.state, folderKey).length;
        this.state = model.assignTerminal(this.state, terminal.id, folderKey, n_existing ? `terminal ${n_existing + 1}` : 'terminal');
        this.cwdCache.set(terminal.id, folderKey);
        this.selectedFolder = model.folderOf(this.state, terminal.id);
        await this.save();
        await this.openTerminal(terminal, { widgetOptions: this.widgetOptions(split), mode: 'activate' });
        this.view = 'work';
        await this.refresh();
        return terminal;
    }

    openRepository(
        key,
    ) {
        // Git 확장은 워크스페이스 루트만 찾으므로 작업 폴더의 저장소를 직접 알려 준다. 저장소가 아니면 조용히 넘긴다.
        const path = new URI(key).path.fsPath();
        this.plugins.willStart.then(() => this.commands.executeCommand('git.openRepository', path)).catch(() => undefined);
    }

    async closeWidget(
        widget,
    ) {
        // 문서의 변경 여부와 저장 확인, 터미널의 종료 확인은 각 위젯의 닫기 계약을 따른다.
        await this.shell.closeWidget(widget.id);
        await this.refresh();
    }

    async removeFolder(
        key,
    ) {
        const current = this.currentWidget();
        const result = model.removeFolder(this.state, key, current?.id);
        const terminals = result.closed.map(id => this.shell.getWidgetById(id)).filter(Boolean);
        if (await this.shell.confirmCloseTerminals(terminals)) {
            this.state = result.state;
            await this.save();
            for (const terminal of terminals) {
                await terminal.closeWithoutSaving();
            }
            if (result.next && result.next !== current?.id) {
                await this.activate(result.next);
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
            this.state = model.setFolderLabel(model.ensureFolder(this.state, key), key, label);
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

    /** 폴더 주소가 속한 홈 폴더의 경로 부분. WSL 폴더(\\wsl.localhost\…)면 WSL 홈, 아니면 이 컴퓨터의 홈이다. 모르면 빈 문자열이다. */
    homePathOf(
        key,
    ) {
        return new URI(key).authority.toLowerCase() === 'wsl.localhost' ? this.wslHomePath : this.homePath;
    }

    /**
     * 화면에 보일 폴더 경로. 홈 아래는 ~로 줄이고, 홈 밖의 WSL 폴더는 배포판 이름을 뺀 Linux 경로로 보인다.
     *
     * Examples
     * --------
     * | 폴더 주소                                         | 결과                  |
     * | ------------------------------------------------- | --------------------- |
     * | `file:///home/me/app` (홈 /home/me)               | `~/app`               |
     * | `file://wsl.localhost/Ubuntu/home/me/app`         | `~/app` (WSL 홈 같음) |
     * | `file://wsl.localhost/Ubuntu/srv/data`            | `/srv/data`           |
     */
    displayPath(
        key,
    ) {
        const uri = new URI(key);
        const path = uri.path.toString();
        const home = this.homePathOf(key);
        let shown = path;
        if (home && (path === home || path.startsWith(`${home}/`))) shown = `~${path.slice(home.length)}`;
        else if (uri.authority.toLowerCase() === 'wsl.localhost') shown = path.replace(/^\/[^/]+/, '') || '/';
        return shown;
    }

    folderName(
        key,
    ) {
        const path = new URI(key).path;
        const fallback = path.toString() === this.homePathOf(key) ? '~' : path.base || this.displayPath(key);
        return model.folderLabel(this.state, key) || model.folderLabel(this.displayWorkState || model.empty(), key) || fallback;
    }

    /**
     * 작업 폴더가 속한 Git 저장소를 찾는다.
     *
     * 작업 폴더는 저장소 루트일 수도, 그 안쪽 폴더일 수도 있다. 폴더를 품는 저장소가 여럿이면(저장소 안의 하위 저장소)
     * 가장 안쪽 것을 고른다. 폴더가 없거나 어느 저장소에도 속하지 않으면 undefined.
     */
    repositoryOf(
        key,
    ) {
        let found;
        let n_rootLength = -1;
        for (const repository of this.scm.repositories) {
            const root = repository.provider.rootUri?.replace(/\/+$/, '');
            const contains = !!key && !!root && (key === root || key.startsWith(`${root}/`));
            if (contains && root.length > n_rootLength) {
                found = repository;
                n_rootLength = root.length;
            }
        }
        return found;
    }

    /**
     * Git 보기가 지금 작업 폴더의 저장소를 보이게 한다.
     *
     * Git 보기·변경 수 배지·브랜치 버튼이 모두 선택된 저장소를 따르므로, 작업 폴더를 바꾸면 여기서 선택을 옮긴다.
     * 작업 폴더가 그대로면 다시 옮기지 않는다 — 사용자가 Git 보기에서 다른 저장소를 직접 고른 것을 되돌리지 않기 위해서다.
     * 폴더가 저장소 밖이면 선택을 비우고 안내를 보인다. 이전 폴더의 변경을 현재 폴더의 변경으로 오인하지 않게 한다.
     */
    selectRepositoryOf(
        key,
    ) {
        const repository = this.repositoryOf(key);
        const changed = repository !== this.syncedRepository;
        if (changed || (key && !repository && this.scm.selectedRepository)) {
            this.scm.selectedRepository = repository;
        }
        this.syncedRepository = repository;
        const host = this.shell.sidebar.node.querySelector('[data-host="scm"]');
        const hasNoRepository = Boolean(key && !repository);
        host.classList.toggle('has-no-repository', hasNoRepository);
        let notice = host.querySelector('.scm-empty');
        if (hasNoRepository) {
            if (!notice) {
                notice = element('div', 'scm-empty');
                notice.append(element('p', 'work-empty-title', 'No Git repository'));
                notice.append(element('p', 'work-empty work-empty-note'));
                notice.append(element('p', 'work-empty work-empty-note', 'Select a work folder that contains a Git repository.'));
                host.append(notice);
            }
            notice.children[1].textContent = this.displayPath(key);
        } else {
            notice?.remove();
        }
    }

    branchOf(
        key,
    ) {
        // Git 확장이 상태 줄 명령 제목에 "$(git-branch) main" 식으로 브랜치를 싣는다.
        const repository = this.repositoryOf(key);
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
        // 배지는 Git 보기가 보여 주는 저장소(지금 작업 폴더의 저장소)의 변경 수만 센다 — 다른 폴더의 변경까지 더하면 보기를 열었을 때 숫자가 맞지 않는다.
        let n_changes = 0;
        for (const group of this.scm.selectedRepository?.provider.groups ?? []) {
            n_changes += group.resources.length;
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
        const current = this.currentWidget();
        const currentId = current?.id;
        const merged = model.mergeWorkPresence(this.state, this.remoteWorkSnapshots);
        this.displayWorkState = merged.state;
        this.remoteWorkRows = merged.remote;
        const rows = model.visibleRows(merged.state, currentId, this.showAll);
        const filesFolder = this.filesFolder();
        if (filesFolder !== this.directoryFolder) {
            this.directoryEntries.clear();
            this.directoryFolder = filesFolder;
        }
        const directory = filesFolder && this.filesExpanded ? await this.readDirectory(new URI(filesFolder)) : null;
        // 표시할 내용이 같으면 기존 행을 유지한다. 큰 파일 목록의 재생성·배치가 터미널 입력을 막지 않게 한다.
        const key = JSON.stringify([
            merged.state.folders, rows, currentId, this.workExpanded, this.filesExpanded, filesFolder, directory,
            current?.getResourceUri?.()?.toString(), this.sidebarIndent(),
            rows.map(row => {
                const terminal = this.shell.getWidgetById(row.id);
                return [this.remoteWorkRows.get(row.id), terminal ? this.programOf(terminal) : '', this.doneIds.has(row.id),
                    agent.activityState(this.activity.get(row.id) ?? agent.idle(), Date.now(), agent.isAgent(this.programs.get(row.id)))];
            }),
        ]);
        if (n_revision === this.n_sidebarRevision && this.view === 'work' && key !== this.workRenderKey) {
            const node = element('div', 'work-view');
            node.append(this.sectionHeader('WORK', this.workExpanded, () => {
                this.workExpanded = !this.workExpanded;
                this.refreshSoon();
            }));
            if (this.workExpanded) {
                const list = element('div', 'work-list');
                if (!merged.state.folders.length) {
                    // 작업 폴더는 에이전트를 실행하면 생긴다. 등록 버튼 대신 그 다음 행동을 알려 준다.
                    list.append(element('p', 'work-empty-title', 'Start a work session'));
                    list.append(element('p', 'work-empty', 'No work folders yet.'));
                    const steps = element('ol', 'work-steps');
                    const directoryStep = element('li', '', 'Go to your project in the terminal.');
                    directoryStep.append(element('code', 'work-command', 'cd /path/to/project'));
                    const agentStep = element('li', '', 'Run claude or codex.');
                    steps.append(directoryStep, agentStep);
                    list.append(steps);
                    list.append(element('p', 'work-empty work-empty-note', 'Its terminals and files will appear here together.'));
                }
                for (const row of rows) {
                    list.append(this.renderRow(row, currentId));
                }
                node.append(list);
            }
            if (filesFolder) {
                node.append(this.sectionHeader(`${this.folderName(filesFolder).toUpperCase()} FILES`, this.filesExpanded, () => {
                    this.filesExpanded = !this.filesExpanded;
                    this.refreshSoon();
                }));
                if (this.filesExpanded) {
                    const files = element('div', 'file-list');
                    this.appendDirectory(files, directory, 0);
                    node.append(files);
                }
            }
            if (this.pointerPressed) {
                // 파일 목록을 읽는 동안 클릭이 시작됐으면 기존 행을 유지하고 다음 갱신에서 교체한다.
                this.refreshAfterPointer = true;
            } else {
                this.shell.sidebar.node.querySelector('[data-host="work"]').replaceChildren(node);
                this.workRenderKey = key;
            }
        }
    }

    /**
     * 파일 구획이 보여 줄 폴더: 작업 폴더가 없으면 지금 보는 상단·내부 터미널의 현재 폴더.
     * 폴더를 모르거나 홈이면 구획을 숨긴다(홈 전체 목록은 쓸모가 없다).
     */
    filesFolder() {
        const current = this.currentWidget();
        const root = this.shownTopTerminal();
        const key = this.shownFolder() || (this.isTerminal(current) ? this.cwdCache.get(current.id) : root ? this.cwdCache.get(root) : this.isFileOnlyGroup(current) ? current.getResourceUri?.()?.parent?.toString() : null) || null;
        const isHome = key && new URI(key).path.toString() === this.homePathOf(key);
        return key && !isHome ? key : null;
    }

    renderRow(
        row,
        currentId,
    ) {
        let node;
        if (row.kind === 'folder') {
            const folder = this.displayWorkState.folders.find(item => item.key === row.key);
            node = element('div', `work-row folder-row${row.current ? ' is-current' : ''}`);
            const toggle = button([codicon(row.expanded ? 'chevron-down' : 'chevron-right'), codicon(row.expanded ? 'folder-opened' : 'folder'), element('span', 'row-name', this.folderName(row.key))], 'row-main', () => {
                this.state = model.setExpanded(model.ensureFolder(this.state, row.key), row.key, !folder.expanded);
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
            // 우클릭은 ⋯와 같은 메뉴를 누른 자리에 연다. ⋯는 우클릭을 모르는 사람이 메뉴를 찾는 단서라 남겨 둔다.
            // Linux는 버튼을 누르는 순간 우클릭 이벤트가 오고, 바로 뒤 버튼을 떼는 동작이 메뉴 바깥 클릭으로 읽혀 메뉴가 닫힌다 — 버튼을 뗀 뒤에 연다.
            node.addEventListener('contextmenu', (event) => {
                event.preventDefault();
                const target = event.currentTarget;
                const point = { x: event.clientX, y: event.clientY };
                const open = () => this.openFolderMenu(row.key, target, point);
                if (event.buttons & 2) {
                    window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
                } else {
                    open();
                }
            });
        } else if (row.kind === 'terminal' && this.remoteWorkRows.has(row.id)) {
            const remote = this.remoteWorkRows.get(row.id);
            node = element('div', 'work-row terminal-row is-remote');
            const label = element('div', 'row-main');
            label.append(element('span', 'prompt-mark', '›_'), element('span', 'row-name', row.name), element('span', 'row-suffix', row.suffix));
            node.append(label, element('span', 'row-meta', 'other window'));
            node.title = `${row.name}${remote.program ? ` · ${remote.program}` : ''} — open in another Paddock window`;
        } else if (row.kind === 'terminal') {
            const terminal = this.shell.getWidgetById(row.id);
            node = element('div', `work-row terminal-row${row.id === currentId ? ' is-current' : ''}${this.doneIds.has(row.id) ? ' is-done' : ''}`);
            const select = button([element('span', 'prompt-mark', '›_'), element('span', 'row-name', row.name), element('span', 'row-suffix', row.suffix)], 'row-main', () => this.run(() => this.activate(row.id)));
            select.addEventListener('dblclick', () => this.run(() => this.renameTerminal(row.id)));
            select.title = this.doneIds.has(row.id) ? `${row.name} — agent output paused; double-click to rename` : `${row.name} — double-click to rename`;
            const programName = terminal ? this.programOf(terminal) : '';
            const state = agent.activityState(this.activity.get(row.id) ?? agent.idle(), Date.now(), agent.isAgent(this.programs.get(row.id)));
            // 기본 이름이 프로그램명과 같으면 한 번만 적는다. 사용자가 바꾼 이름 옆에는 실행 프로그램을 유지한다.
            const program = element('span', 'row-meta', state && row.name.toLowerCase() === programName.toLowerCase() ? '' : programName);
            node.append(select, program);
            if (terminal && state) {
                const labels = {
                    working: ['Working', 'Recent terminal output was detected; this can include prompt redraws.'],
                    waiting: ['Waiting', 'Enter was sent; no response output has been detected yet.'],
                    quiet: ['Quiet', 'No terminal output in the last 3 seconds. The agent may be waiting for input or still thinking.'],
                };
                const [label, detail] = labels[state];
                const status = element('span', `agent-status is-${state}`, label);
                status.title = `${programName} — ${detail} Estimated from terminal input and output.`;
                status.setAttribute('aria-label', `${programName}: ${label}`);
                node.append(status);
                node.dataset.agentState = state;
            }
            this.attachTerminalMenu(node, row.id);
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
        point = null,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const n_terminals = model.terminalsOf(this.state, key).length;
        const n_remote = [...this.remoteWorkRows.keys()].filter(id => model.folderOf(this.displayWorkState, id) === key).length;
        const items = [
            ['add', 'New work terminal here', () => this.newWorkTerminal({ folderKey: key })],
            ['edit', 'Rename in list', () => this.renameFolder(key)],
            ['folder-opened', 'Reveal folder', () => this.commands.executeCommand('revealFileInOS', new URI(key))],
        ];
        if (!n_remote) items.push(['close', 'Remove from list', () => this.removeFolder(key), true]);
        for (const [icon, label, action, danger] of items) {
            const item = button([codicon(icon), element('span', '', label)], `menu-item${danger ? ' is-danger' : ''}`, () => {
                menu.hidePopover();
                this.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.append(element('p', 'menu-note', n_remote ? `${n_remote} terminal${n_remote === 1 ? '' : 's'} open in another window` : `Closes ${n_terminals} terminal${n_terminals === 1 ? '' : 's'} · folder is kept`));
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        // ⋯ 버튼은 마우스를 올렸을 때만 보이므로 위치는 폴더 행 전체를 기준으로 잡는다. 우클릭이면 누른 자리에 연다.
        const bounds = (anchor.closest('.work-row') || anchor).getBoundingClientRect();
        const left = point ? point.x : bounds.right + 4;
        const top = point ? point.y : bounds.top;
        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;
        menu.showPopover();
        // 창 오른쪽·아래 끝을 넘으면 안쪽으로 당긴다.
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(left, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(top, window.innerHeight - size.height - 4))}px`;
    }

    /** 작업 터미널 행과 탭의 우클릭 메뉴를 연결한다. Linux에서는 버튼을 놓은 뒤 메뉴를 연다. */
    attachTerminalMenu(
        node,
        id,
    ) {
        node.addEventListener('contextmenu', (event) => {
            event.preventDefault();
            const point = { x: event.clientX, y: event.clientY };
            const open = () => this.openTerminalMenu(id, point);
            if (event.buttons & 2) window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
            else open();
        });
    }

    /** 이름 바꾸기와 닫기를 작업 터미널의 우클릭 메뉴에 모은다. */
    openTerminalMenu(
        id,
        point,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const items = [
            ['edit', 'Rename terminal', () => this.renameTerminal(id), false],
            ['close', 'Close terminal', () => this.closeWidget(this.shell.getWidgetById(id)), true],
        ];
        for (const [icon, label, action, danger] of items) {
            const item = button([codicon(icon), element('span', '', label)], `menu-item${danger ? ' is-danger' : ''}`, () => {
                menu.hidePopover();
                this.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        menu.style.left = `${point.x}px`;
        menu.style.top = `${point.y}px`;
        menu.showPopover();
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(point.x, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(point.y, window.innerHeight - size.height - 4))}px`;
    }

    /** 표시할 파일·폴더와 펼친 하위 목록. 읽기 실패와 빈 폴더를 구별하고, 화면에 쓰지 않는 파일 내용·수정 시각은 담지 않는다. */
    async readDirectory(
        uri,
    ) {
        const directoryKey = uri.toString();
        const n_revision = this.n_directoryRevision;
        let stat;
        try {
            const provider = await this.files.activateProvider(uri.scheme);
            // 폴더가 그대로면 파일 이름 목록 대신 수정 시각 하나만 읽는다. 시각 정보가 없으면 실제 목록을 확인한다.
            const modified = (await provider.stat(uri)).mtime;
            const cached = this.directoryEntries.get(directoryKey);
            if (typeof modified === 'number' && cached?.modified === modified) {
                stat = cached.stat;
            } else {
                stat = await this.files.resolve(uri);
                if (typeof modified === 'number' && n_revision === this.n_directoryRevision) {
                    this.directoryEntries.set(directoryKey, { modified, stat });
                }
            }
        } catch {
            this.directoryEntries.delete(directoryKey);
        }
        const hidden = this.hiddenNames();
        const entries = [...(stat?.children || [])].filter(entry => !hidden.has(entry.name)).sort((left, right) => Number(right.isDirectory) - Number(left.isDirectory) || left.name.localeCompare(right.name));
        const children = [];
        for (const entry of entries) {
            const key = entry.resource.toString();
            const expanded = entry.isDirectory && this.expandedDirectories.has(key);
            children.push({
                uri: key,
                name: entry.name,
                isDirectory: entry.isDirectory,
                expanded,
                directory: expanded ? await this.readDirectory(entry.resource) : null,
            });
        }
        return { found: Boolean(stat), empty: !stat?.children?.length, children };
    }

    appendDirectory(
        parent,
        directory,
        depth,
    ) {
        if (!directory.found) {
            parent.append(element('p', 'work-empty', 'Folder not found.'));
        } else if (directory.empty) {
            parent.append(element('p', 'work-empty', 'Empty folder.'));
        }
        const current = this.currentWidget()?.getResourceUri?.()?.toString();
        for (const entry of directory.children) {
            const key = entry.uri;
            const uri = new URI(key);
            const expanded = entry.expanded;
            const icons = entry.isDirectory ? [codicon(expanded ? 'chevron-down' : 'chevron-right'), codicon(expanded ? 'folder-opened' : 'folder')] : [codicon('file')];
            const row = button([...icons, element('span', 'row-name', entry.name)], `file-row${entry.isDirectory ? '' : ' is-file'}${key === current ? ' is-current' : ''}`, () => this.run(async () => {
                if (entry.isDirectory) {
                    if (expanded) this.expandedDirectories.delete(key);
                    else this.expandedDirectories.add(key);
                    await this.refresh();
                } else {
                    await this.openFile(uri);
                }
            }));
            row.style.paddingLeft = `${8 + depth * this.sidebarIndent()}px`;
            row.title = uri.path.toString();
            row.dataset.uri = key;
            if (!entry.isDirectory) {
                row.addEventListener('contextmenu', (event) => {
                    event.preventDefault();
                    const point = { x: event.clientX, y: event.clientY };
                    const open = () => this.openFileMenu(uri, point);
                    if (event.buttons & 2) window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
                    else open();
                });
            }
            parent.append(row);
            if (entry.isDirectory && expanded) {
                this.appendDirectory(parent, entry.directory, depth + 1);
            }
        }
    }

    /** 파일을 우클릭하면 설치된 확장이 제공하는 선택형 편집기도 고를 수 있게 한다. */
    openFileMenu(
        uri,
        point,
    ) {
        const menu = element('div', 'paddock-menu');
        menu.setAttribute('popover', '');
        menu.setAttribute('role', 'menu');
        const items = [
            ['file', 'Open', () => this.openFile(uri)],
            ['open-preview', 'Open With...', () => this.openFileWith(uri)],
            ['terminal', 'Command Palette...', async () => {
                const opened = [...this.shell.mainPanel.widgets()].find(widget => widget.getResourceUri?.()?.isEqual(uri));
                if (opened) await this.activate(opened.id);
                else await this.openFile(uri);
                await this.commands.executeCommand('workbench.action.showCommands');
            }],
        ];
        for (const [icon, label, action] of items) {
            const item = button([codicon(icon), element('span', '', label)], 'menu-item', () => {
                menu.hidePopover();
                this.run(action);
            });
            item.setAttribute('role', 'menuitem');
            menu.append(item);
        }
        menu.addEventListener('toggle', (event) => {
            if (event.newState === 'closed') menu.remove();
        });
        document.body.append(menu);
        menu.style.left = `${point.x}px`;
        menu.style.top = `${point.y}px`;
        menu.showPopover();
        const size = menu.getBoundingClientRect();
        menu.style.left = `${Math.max(4, Math.min(point.x, window.innerWidth - size.width - 4))}px`;
        menu.style.top = `${Math.max(4, Math.min(point.y, window.innerHeight - size.height - 4))}px`;
    }

    /** Theia의 편집기 선택 목록에서 고른 화면을 터미널 옆 파일 칸에 둔다. */
    async openFileWith(
        uri,
    ) {
        await this.plugins.willStart;
        const fileRoot = this.fileRoots.get(this.currentWidget()?.id) || this.shownTopTerminal();
        const widget = await this.openWith.openWith(uri);
        if (widget) {
            const bar = this.shell.getTabBarFor(widget);
            if (bar?.titles.some(title => this.isTerminal(title.owner))) {
                const terminal = bar.titles.find(title => this.isTerminal(title.owner))?.owner;
                await this.shell.addWidget(widget, { area: 'main', mode: 'split-right', ref: terminal });
                await this.activate(widget.id);
            }
            if (fileRoot && !model.folderContaining(this.state, uri.toString())) {
                this.fileRoots.set(widget.id, fileRoot);
                await this.saveInnerTabs();
            }
            this.refreshSoon();
        }
    }

    /** 현재 Markdown 파일의 원문 편집기. 미리보기 탭을 보고 있어도 연결된 원문을 찾는다. */
    markdownSourceWidget() {
        const current = this.currentWidget();
        const uri = current?.getResourceUri?.();
        const sourceUri = current instanceof WebviewWidget ? this.markdownPreviewSources.get(current.id) : uri?.toString();
        let source = null;
        if (sourceUri) {
            source = [...this.shell.mainPanel.widgets()].find(widget => widget.getResourceUri?.()?.toString() === sourceUri && !(widget instanceof CustomEditorWidget)) || null;
            if (source && !MARKDOWN_PREVIEW.EXTENSIONS.has(source.getResourceUri().path.ext.toLowerCase())) source = null;
        }
        return source;
    }

    /** 연결된 Markdown 미리보기 웹 화면을 찾는다. */
    markdownPreviewWidget(
        uri,
    ) {
        const sourceUri = uri.toString();
        const widgets = [...this.shell.mainPanel.widgets()];
        let preview = widgets.find(widget => widget instanceof WebviewWidget && this.markdownPreviewSources.get(widget.id) === sourceUri) || null;
        if (!preview) {
            const matching = widgets.filter(widget => widget instanceof WebviewWidget
                && widget.viewType === MARKDOWN_PREVIEW.VIEW_TYPE
                && !this.markdownPreviewSources.has(widget.id)
                && widget.title.label === `Preview ${uri.path.base}`);
            if (matching.length === 1) {
                preview = matching[0];
                this.markdownPreviewSources.set(preview.id, sourceUri);
            }
        }
        return preview;
    }

    /** 확장이 같은 칸의 미리보기 명령을 끝내기 전에 웹 화면을 추가하는 경우를 기다린다. */
    async waitForMarkdownPreview(
        uri,
    ) {
        let preview = this.markdownPreviewWidget(uri);
        if (!preview) {
            preview = await new Promise(resolve => {
                const listener = this.shell.onDidAddWidget(widget => {
                    if (widget instanceof WebviewWidget && widget.viewType === MARKDOWN_PREVIEW.VIEW_TYPE && widget.title.label === `Preview ${uri.path.base}`) {
                        clearTimeout(timer);
                        listener.dispose();
                        this.markdownPreviewSources.set(widget.id, uri.toString());
                        resolve(widget);
                    }
                });
                const timer = setTimeout(() => {
                    listener.dispose();
                    resolve(null);
                }, 3000);
            });
        }
        return preview;
    }

    /** 파일 보기·미리 보기·같이 보기 버튼이 원문과 렌더링 화면의 배치를 바꾼다. */
    async setMarkdownView(
        mode,
    ) {
        const source = this.markdownSourceWidget();
        if (source && ['file', 'preview', 'both'].includes(mode)) {
            const uri = source.getResourceUri();
            let preview = this.markdownPreviewWidget(uri);
            const wasSplit = Boolean(preview && this.shell.getTabBarFor(source) !== this.shell.getTabBarFor(preview));
            if (mode === 'file') {
                if (preview) await this.shell.closeWidget(preview.id);
                await this.activate(source.id);
            } else if (mode === 'both' && wasSplit) {
                // 기존 칸을 다시 추가하면 원문 칸을 반으로 나눈다. 이미 나란히 열려 있으면 선택만 바꿔 사용자가 끈 너비를 유지한다.
                await this.activate(source.id);
            } else {
                await this.plugins.willStart;
                const command = MARKDOWN_PREVIEW.COMMANDS[mode];
                if (!this.commands.getCommand(command)) {
                    await this.offerExtensionSearch('Install Markdown Preview Enhanced to preview this file.', 'Markdown Preview Enhanced');
                } else {
                    this.pendingMarkdownSource = uri.toString();
                    try {
                        await this.activate(source.id);
                        await this.commands.executeCommand(command);
                        preview = await this.waitForMarkdownPreview(uri);
                    } finally {
                        this.pendingMarkdownSource = null;
                    }
                    if (preview) {
                        const options = mode === 'both'
                            ? { area: 'main', mode: 'split-right', ref: source }
                            : { area: 'main', ref: source };
                        this.pendingMarkdownSource = uri.toString();
                        try {
                            await this.shell.addWidget(preview, options);
                        } finally {
                            this.pendingMarkdownSource = null;
                        }
                        // 처음 나란히 볼 때만 공간을 배분한다.
                        if (mode === 'both') await this.balanceSplit(preview, 'split-right');
                        await this.activate(mode === 'both' ? source.id : preview.id);
                    }
                }
            }
            this.refreshSoon();
        }
    }

    /** 전용 편집 기능이 없으면 이유를 알리고 필요한 확장의 검색 결과로 연결한다. */
    async offerExtensionSearch(
        message,
        query,
    ) {
        const action = await this.messages.warn(message, 'Find extension');
        if (action === 'Find extension') {
            await this.showView('extensions');
            this.container.get(VSXExtensionsSearchModel).query = query;
        }
    }

    /**
     * 파일 구획에서 숨길 이름. `files.exclude` 설정(기본값은 VS Code와 같은 `.git`·`.DS_Store` 등 + `__pycache__`)에서
     * 켜진 패턴 중 이름 하나로 된 것을 모은다. 앞에 붙은 "모든 하위 폴더"(별표 둘 + 슬래시)는 떼고 본다.
     * 와일드카드가 든 패턴(예: 모든 `.pyc` 파일)은 다루지 않는다.
     */
    hiddenNames() {
        const patterns = this.preferences.get('files.exclude', {}) || {};
        const names = Object.keys(patterns)
            .filter(pattern => patterns[pattern] === true)
            .map(pattern => pattern.replace(/^\*\*\//, ''))
            .filter(name => !/[*?[\]{}/]/.test(name));
        return new Set(names);
    }

    async openFile(
        uri,
    ) {
        await this.plugins.willStart;
        const fileRoot = this.fileRoots.get(this.currentWidget()?.id) || this.shownTopTerminal();
        const bars = [...this.shell.mainPanel.tabBars()];
        const fileBar = bars.find(bar => bar.titles.length && bar.titles.every(title => !this.isTerminal(title.owner)) && bar.currentTitle?.owner?.getResourceUri?.());
        const current = this.currentWidget();
        const terminal = this.isTerminal(current) ? current : bars.flatMap(bar => bar.titles.map(title => title.owner)).find(widget => this.isTerminal(widget));
        // 파일은 터미널 옆 칸에 둔다. 이미 파일 칸이 있으면 그 칸의 탭을 재사용한다.
        const widgetOptions = fileBar
            ? { area: 'main', ref: fileBar.currentTitle.owner }
            : terminal ? { area: 'main', mode: 'split-right', ref: terminal } : { area: 'main' };
        const options = { mode: 'activate', widgetOptions, preview: false };
        const opener = await this.opener.getOpener(uri, options);
        if (uri.path.ext.toLowerCase() === '.pen' && !opener.id.startsWith('custom-editor-')) {
            // 원문 열기는 유지하되 디자인 화면으로 열린 것으로 오해하지 않게 전용 편집기의 설치 경로를 알린다.
            void this.run(() => this.offerExtensionSearch('Showing source text. Install Pencil to edit this design.', 'Pencil'));
        }
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
            const widget = await opener.open(uri, options);
            // 이전에 터미널 칸에 열린 파일은 같은 URI를 다시 열어도 새 위치 옵션을 무시할 수 있다.
            const bar = widget && this.shell.getTabBarFor(widget);
            if (widget && bar?.titles.some(title => this.isTerminal(title.owner))) {
                await this.shell.addWidget(widget, widgetOptions);
                await this.activate(widget.id);
            }
            if (widget && fileRoot && !model.folderContaining(this.state, uri.toString())) {
                this.fileRoots.set(widget.id, fileRoot);
                await this.saveInnerTabs();
            }
            this.refreshSoon();
        }
    }

    // -- 위쪽 추가 터미널 줄 --

    /** 탭이 줄을 넘치면 활성 탭이 보이도록 스크롤하고 잘린 탭을 경계로 맞춘 뒤, 넘침 표시를 갱신한다. */
    keepActiveTabVisible(
        strip,
    ) {
        strip.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        this.alignTabsToEdge(strip);
        this.updateTabOverflow(strip);
    }

    /** 잘린 탭을 경계로 맞추되 활성 탭을 보이는 범위에 유지한다. */
    alignTabsToEdge(
        strip,
    ) {
        tabOverflow.alignTabsToEdge(strip);
    }

    /** 스크롤 위치를 바꾸지 않고 잘림·넘김 버튼·숨은 탭의 점을 갱신한다. */
    updateTabOverflow(
        strip,
    ) {
        tabOverflow.updateTabOverflow(strip);
    }

    renderTabs() {
        const strip = this.shell.tabs.node;
        const current = this.currentWidget();
        const extras = this.topTerminals();
        const contents = document.createDocumentFragment();
        for (const terminal of extras) {
            const isGroupShown = this.topTerminalOf(current) === terminal.id && !this.shownFolder();
            // 이 묶음의 내부 터미널·파일을 보는 중이면 아래 줄에서 선택을 표시하고, 위 탭은 묶음이 열려 있다는 배경을 유지한다.
            const isActive = isGroupShown && current === terminal;
            const tab = element('div', `tab${isActive ? ' is-active' : ''}${isGroupShown && !isActive ? ' is-group' : ''}${this.doneIds.has(terminal.id) ? ' is-done' : ''}`);
            const cwd = this.cwdCache.get(terminal.id);
            const label = cwd ? `${this.programOf(terminal)} · ${this.displayPath(cwd)}` : this.programOf(terminal);
            const select = button(label, 'tab-select', () => this.run(() => this.activate(terminal.id)));
            select.setAttribute('role', 'tab');
            select.setAttribute('aria-selected', String(isGroupShown));
            select.dataset.widgetId = terminal.id;
            select.title = label;
            const close = button([codicon('close')], 'tab-close', () => this.run(() => this.closeWidget(terminal)));
            close.setAttribute('aria-label', `Close ${label}`);
            tab.append(select, close);
            contents.append(tab);
        }
        if (tabOverflow.replaceTabs(strip, contents)) this.keepActiveTabVisible(strip);
        else this.updateTabOverflow(strip);
    }

    // -- 본문 위 내부 터미널·파일 탭 줄 --

    /**
     * 아래 줄에 현재 묶음의 내부 터미널과 파일을 그린다. 내부 터미널을 추가하면 처음 연 터미널도 함께 보여 준다.
     * 작업 터미널 이름은 사이드바와 같은 행(`terminalRows`)을 쓴다.
     * 새 위쪽 터미널은 내부 탭이 없으므로 아래 줄에 ＋만 보인다.
     */
    renderFolderTabs() {
        const bar = this.shell.folderBar;
        const accountLabel = this.accountLabel(this.currentWidget());
        const accountPicker = bar.node.querySelector('.account-picker');
        accountPicker.querySelector('.account-picker-label').textContent = accountLabel || 'Accounts';
        accountPicker.title = accountLabel ? `${accountLabel} — open another account in a new terminal` : 'Open a new terminal with a Claude or Codex account';
        const key = this.shownFolder();
        this.selectRepositoryOf(key);
        const root = key ? null : this.shownTopTerminal();
        const strip = bar.node.querySelector('.folder-tabs');
        const contents = document.createDocumentFragment();
        bar.node.querySelector('.folder-bar-actions').hidden = !key;
        const add = bar.node.querySelector('.folder-tab-add');
        // 어느 폴더에서 열리는지를 이름에 쓴다. 위쪽 묶음에서는 지금 보는 터미널의 폴더에서 열린다.
        const innerCwd = root ? this.cwdCache.get(this.currentWidget()?.id) : this.isFileOnlyGroup(this.currentWidget()) ? this.currentWidget().getResourceUri?.()?.parent?.toString() : null;
        add.title = key ? `New terminal in ${this.displayPath(key)}` : `New terminal in ${innerCwd ? this.displayPath(innerCwd) : 'this terminal\'s folder'}`;
        add.setAttribute('aria-label', add.title);
        if (key) {
            const current = this.currentWidget();
            bar.node.dataset.folder = key;
            for (const row of model.terminalRows(this.state, key)) {
                const terminal = this.shell.getWidgetById(row.id);
                const isActive = terminal === current;
                // 탭 줄과 본문 칸의 위치는 일치하지 않는다. 다른 칸에 보이는 터미널은 도움말로 알리고 선택 표시는 현재 탭에만 둔다.
                const isShown = !isActive && Boolean(terminal?.isVisible);
                const tab = element('div', `tab${isActive ? ' is-active' : ''}${this.doneIds.has(row.id) ? ' is-done' : ''}`);
                const label = `${row.name}${row.suffix}`;
                const select = button(label, 'tab-select', () => this.run(() => this.activate(row.id)));
                select.setAttribute('role', 'tab');
                select.setAttribute('aria-selected', String(isActive));
                select.dataset.widgetId = row.id;
                select.title = terminal ? `${label} · ${this.programOf(terminal)}${isShown ? ' · Visible in another pane' : ''}` : label;
                select.addEventListener('dblclick', () => this.run(() => this.renameTerminal(row.id)));
                const close = button([codicon('close')], 'tab-close', () => this.run(() => this.closeWidget(terminal)));
                close.setAttribute('aria-label', `Close ${label}`);
                tab.append(select, close);
                this.attachTerminalMenu(tab, row.id);
                contents.append(tab);
            }
            const branch = this.branchOf(key);
            bar.node.querySelector('.folder-bar-branch').hidden = !branch;
            bar.node.querySelector('.folder-bar-branch-name').textContent = branch;
        } else {
            delete bar.node.dataset.folder;
            bar.node.querySelector('.folder-bar-branch').hidden = true;
            bar.node.querySelector('.folder-bar-branch-name').textContent = '';
            if (root) {
                let n_inner = 0;
                for (const id of this.innerTabIds(root)) {
                    const terminal = this.shell.getWidgetById(id);
                    if (terminal) {
                        n_inner += 1;
                        const tab = element('div', `tab${terminal === this.currentWidget() ? ' is-active' : ''}`);
                        const label = this.accountLabel(terminal) || `terminal ${n_inner}`;
                        const select = button(label, 'tab-select', () => this.run(() => this.activate(id)));
                        select.setAttribute('role', 'tab');
                        select.setAttribute('aria-selected', String(terminal === this.currentWidget()));
                        select.dataset.widgetId = id;
                        const close = button([codicon('close')], 'tab-close', () => this.run(() => this.closeWidget(terminal)));
                        close.setAttribute('aria-label', `Close ${label}`);
                        tab.append(select, close);
                        contents.append(tab);
                    }
                }
            }
        }
        for (const widget of this.shell.widgets) {
            const uri = widget.getResourceUri?.();
            const belongs = !this.isTerminal(widget) && (this.isFileOnlyGroup(widget) || (uri
                ? (key ? model.folderContaining(this.state, uri.toString()) === key : root && this.fileRoots.get(widget.id) === root)
                : widget instanceof WebviewWidget && (key ? this.webviewFolders.get(widget.id) === key : root && this.fileRoots.get(widget.id) === root)));
            if (belongs) {
                const tab = element('div', `tab${widget === this.currentWidget() ? ' is-active' : ''}`);
                const label = uri?.path.base || widget.title.label;
                const isDirty = Saveable.isDirty(widget);
                const select = button(label, 'tab-select', () => this.run(() => this.activate(widget.id)));
                select.setAttribute('role', 'tab');
                select.setAttribute('aria-selected', String(widget === this.currentWidget()));
                select.setAttribute('aria-label', `${label}${isDirty ? ' — unsaved changes' : ''}`);
                select.dataset.widgetId = widget.id;
                select.title = `${uri?.path.toString() || widget.title.label}${isDirty ? ' — unsaved changes' : ''}`;
                tab.append(select);
                if (isDirty) {
                    const mark = element('span', 'tab-unsaved');
                    mark.setAttribute('aria-hidden', 'true');
                    mark.title = 'Unsaved changes';
                    tab.append(mark);
                }
                const close = button([codicon('close')], 'tab-close', () => this.run(() => this.closeWidget(widget)));
                close.setAttribute('aria-label', `Close ${label}`);
                tab.append(close);
                contents.append(tab);
            }
        }
        const source = this.markdownSourceWidget();
        const viewActions = bar.node.querySelector('.markdown-view-actions');
        viewActions.hidden = !source;
        if (source) {
            const preview = this.markdownPreviewWidget(source.getResourceUri());
            const mode = !preview ? 'file' : this.shell.getTabBarFor(source) === this.shell.getTabBarFor(preview) ? 'preview' : 'both';
            for (const action of viewActions.querySelectorAll('[data-markdown-view]')) {
                action.setAttribute('aria-pressed', String(action.dataset.markdownView === mode));
            }
        }
        if (tabOverflow.replaceTabs(strip, contents)) this.keepActiveTabVisible(strip);
        else this.updateTabOverflow(strip);
    }

    /**
     * 셸 메뉴를 그린다. 실행 파일이 이 컴퓨터에 없는 셸은 빼고, 기본 셸과 같은 실행 파일을 가리키는 항목은 한 번만 보인다.
     * 'SHELL'은 운영체제 기본 셸을 가리키는 Theia 내부 이름이라 실제 실행 파일 이름(bash·zsh 등)으로 보인다.
     */
    async renderShellMenu() {
        const menu = this.shell.header.node.querySelector('#shell-menu');
        const entries = await this.shellEntries();
        menu.replaceChildren();
        for (const { profile, shellPath, name, place } of entries) {
            const isDefault = profile === this.profiles.defaultProfile;
            const label = isDefault ? `${name} (default)` : name;
            const meta = [place, isDefault ? 'default' : ''].filter(Boolean).join(' · ');
            const item = button([codicon(isDefault ? 'check' : 'blank'), element('span', 'shell-option-name', name), element('span', 'shell-option-meta', meta)], 'shell-option', () => {
                menu.hidePopover();
                this.run(() => this.newExtraTerminal({ profile }));
            });
            item.setAttribute('role', 'menuitem');
            item.title = shellPath || label;
            menu.append(item);
        }
    }

    /**
     * 이 컴퓨터에서 열 수 있는 셸 프로필 목록. 실행 파일이 없는 셸은 빼고, 같은 실행 파일을 가리키는 항목은 한 번만 넣는다.
     * 'SHELL'은 운영체제 기본 셸을 가리키는 Theia 내부 이름이라 실제 실행 파일 이름(bash·zsh 등)을 이름으로 쓴다.
     */
    async shellEntries() {
        const entries = [];
        const isWindows = OS.backend.type() === OS.Type.Windows;
        // Windows 경로는 대소문자를 가리지 않아 C:\WINDOWS\system32\cmd.exe와 C:\Windows\System32\cmd.exe가 같은 셸이다.
        const sameFile = (left, right) => (isWindows ? left.toLowerCase() === right.toLowerCase() : left === right);
        for (const [id, profile] of this.profiles.all) {
            const shellPath = (profile instanceof ShellTerminalProfile && profile.shellPath) || (id === 'SHELL' ? this.systemShellPath : '');
            const isDuplicate = entries.some(entry => shellPath && sameFile(entry.shellPath, shellPath));
            if (!isDuplicate && await this.isShellUsable(profile)) {
                const shellName = shellPath ? shellPath.split(/[\\/]/).pop().replace(/\.exe$/i, '') : id;
                // Windows에서는 셸이 Windows에서 도는지 WSL의 Linux에서 도는지 함께 보인다. 같은 bash라도 쓰는 도구와 파일이 다르다.
                const place = !isWindows ? '' : wsl.isWslShell(shellPath) ? 'Linux' : 'Windows';
                entries.push({ id, profile, shellPath, name: id === 'SHELL' ? shellName : id, place });
            }
        }
        return entries;
    }

    // -- 빠른 설정 --

    /** 이 컴퓨터에 설치된 글꼴인지. 대체 글꼴과 글자 폭이 다르면 설치된 것으로 본다. */
    isFontInstalled(
        family,
    ) {
        const context = document.createElement('canvas').getContext('2d');
        const sample = 'mmmmmmmmmmlli10OO';
        return ['monospace', 'serif'].some((fallback) => {
            context.font = `20px ${fallback}`;
            const base = context.measureText(sample).width;
            context.font = `20px '${family}', ${fallback}`;
            return context.measureText(sample).width !== base;
        });
    }

    /** 사이드바의 한 단계 들여쓰기 폭을 설정 범위 안에서 읽는다. */
    sidebarIndent() {
        const value = this.preferences.get('paddock.sidebarIndent', 12);
        return Math.max(APPEARANCE.SIDEBAR_INDENT_MIN, Math.min(APPEARANCE.SIDEBAR_INDENT_MAX, Number(value) || 12));
    }

    /** 화면 글꼴과 사이드바 들여쓰기를 현재 창에 적용한다. */
    applyInterfacePreferences() {
        const font = this.preferences.get('paddock.interfaceFontFamily', '');
        if (font) {
            const family = font.replaceAll('\\', '\\\\').replaceAll("'", "\\'");
            this.shell.node.style.setProperty('--p-interface-font', `'${family}', -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`);
        } else {
            this.shell.node.style.removeProperty('--p-interface-font');
        }
        this.shell.node.style.setProperty('--p-sidebar-indent', `${this.sidebarIndent()}px`);
    }

    /** 설정 하나를 사용자 설정 파일에 저장한다. 바로 화면에 적용된다. */
    async setPreference(
        key,
        value,
    ) {
        await this.preferences.set(key, value, PreferenceScope.User);
    }

    /**
     * 빠른 설정 팝오버를 그린다: 색 테마, 글자 크기, 터미널·화면 글꼴, 사이드바 들여쓰기, 전체 설정 열기.
     * 자주 바꾸는 겉모습만 한곳에 모으고, 나머지는 전체 설정(Theia 설정 화면)으로 넘긴다.
     */
    renderQuickSettings() {
        const panel = this.shell.sidebar.node.querySelector('#quick-settings');
        const focusedSetting = panel.contains(document.activeElement) ? document.activeElement.dataset.setting : null;
        const scrollTop = panel.scrollTop;
        const themes = this.container.get(ThemeService);
        const current = themes.getCurrentTheme().id;
        panel.replaceChildren(element('p', 'quick-title', 'Quick settings'));

        const themeRow = element('label', 'quick-row');
        const themeSelect = element('select', 'quick-select');
        themeSelect.dataset.setting = 'theme';
        const sorted = [...themes.getThemes()].sort((left, right) => left.type.localeCompare(right.type) || left.label.localeCompare(right.label));
        for (const theme of sorted) {
            const option = element('option', '', theme.label);
            option.value = theme.id;
            option.selected = theme.id === current;
            themeSelect.append(option);
        }
        themeSelect.title = themes.getCurrentTheme().label;
        themeSelect.addEventListener('change', () => themes.setCurrentTheme(themeSelect.value));
        // 색 테마는 창 틀·터미널·파일 화면 전체에 한 번에 적용된다 — 고르는 즉시 보이므로 설명 문구를 두지 않는다.
        themeRow.append(element('span', 'quick-label', 'Color theme'), element('span', 'quick-space'), themeSelect);
        panel.append(themeRow);

        const size = this.preferences.get('terminal.integrated.fontSize', 14);
        const sizeRow = element('div', 'quick-row');
        const setSize = (next) => this.run(async () => {
            const value = Math.max(APPEARANCE.FONT_SIZE_MIN, Math.min(APPEARANCE.FONT_SIZE_MAX, next));
            await this.setPreference('terminal.integrated.fontSize', value);
            await this.setPreference('editor.fontSize', value);
            this.renderQuickSettings();
        });
        const smaller = button([codicon('remove')], 'quick-step', () => setSize(size - 1));
        smaller.dataset.setting = 'smaller-text';
        smaller.setAttribute('aria-label', 'Smaller text');
        smaller.disabled = size <= APPEARANCE.FONT_SIZE_MIN;
        const larger = button([codicon('add')], 'quick-step', () => setSize(size + 1));
        larger.dataset.setting = 'larger-text';
        larger.setAttribute('aria-label', 'Larger text');
        larger.disabled = size >= APPEARANCE.FONT_SIZE_MAX;
        sizeRow.append(element('span', 'quick-label', 'Terminal & editor size'), element('span', 'quick-space'), smaller, element('span', 'quick-value', `${size}px`), larger);
        panel.append(sizeRow);

        const family = this.preferences.get('terminal.integrated.fontFamily', '');
        const fontRow = element('label', 'quick-row');
        const select = element('select', 'quick-select');
        select.dataset.setting = 'code-font';
        select.title = 'Font for terminals and code editors';
        const installed = APPEARANCE.TERMINAL_FONTS.filter(name => this.isFontInstalled(name));
        const chosen = installed.find(name => family.trim().replace(/^['"]/, '').startsWith(name)) || '';
        for (const name of installed) {
            const option = element('option', '', name);
            option.value = name;
            option.selected = name === chosen;
            select.append(option);
        }
        select.addEventListener('change', () => this.run(async () => {
            const value = `'${select.value}', ${APPEARANCE.HANGUL_FALLBACK}`;
            await this.setPreference('terminal.integrated.fontFamily', value);
            await this.setPreference('editor.fontFamily', value);
        }));
        fontRow.append(element('span', 'quick-label', 'Code font'), element('span', 'quick-space'), select);
        panel.append(fontRow);

        // 새 터미널(＋)이 여는 셸. 고른 값은 이 운영체제의 기본 셸 설정에 저장되고, 이미 열린 터미널은 그대로다.
        const shellRow = element('label', 'quick-row');
        const shellSelect = element('select', 'quick-select');
        shellSelect.dataset.setting = 'default-shell';
        shellSelect.title = 'Shell for new terminals';
        shellRow.append(element('span', 'quick-label', 'Default shell'), element('span', 'quick-space'), shellSelect);
        panel.append(shellRow);
        void this.shellEntries().then((entries) => {
            for (const { id, name, place } of entries) {
                const option = element('option', '', place ? `${name} (${place})` : name);
                option.value = id;
                option.selected = this.profiles.getProfile(id) === this.profiles.defaultProfile;
                shellSelect.append(option);
            }
        });
        shellSelect.addEventListener('change', () => this.run(() => this.setPreference(`terminal.integrated.defaultProfile.${OS_PREFERENCE_KEY[OS.backend.type()]}`, shellSelect.value)));

        const interfaceFont = this.preferences.get('paddock.interfaceFontFamily', '');
        const interfaceRow = element('label', 'quick-row');
        const interfaceSelect = element('select', 'quick-select');
        interfaceSelect.dataset.setting = 'interface-font';
        const systemFont = element('option', '', 'System default');
        systemFont.value = '';
        interfaceSelect.append(systemFont);
        const installedInterfaceFonts = APPEARANCE.INTERFACE_FONTS.filter(name => this.isFontInstalled(name));
        if (interfaceFont && !installedInterfaceFonts.includes(interfaceFont)) {
            installedInterfaceFonts.unshift(interfaceFont);
        }
        for (const name of installedInterfaceFonts) {
            const option = element('option', '', name);
            option.value = name;
            interfaceSelect.append(option);
        }
        interfaceSelect.value = interfaceFont;
        interfaceSelect.addEventListener('change', () => this.run(() => this.setPreference('paddock.interfaceFontFamily', interfaceSelect.value)));
        interfaceRow.append(element('span', 'quick-label', 'Interface font'), element('span', 'quick-space'), interfaceSelect);
        panel.append(interfaceRow);

        const indent = this.sidebarIndent();
        const indentRow = element('div', 'quick-row');
        const setIndent = (next) => this.run(() => this.setPreference('paddock.sidebarIndent', next));
        const decreaseIndent = button([codicon('remove')], 'quick-step', () => setIndent(indent - 1));
        decreaseIndent.dataset.setting = 'decrease-indent';
        decreaseIndent.setAttribute('aria-label', 'Decrease sidebar indent');
        decreaseIndent.disabled = indent <= APPEARANCE.SIDEBAR_INDENT_MIN;
        const increaseIndent = button([codicon('add')], 'quick-step', () => setIndent(indent + 1));
        increaseIndent.dataset.setting = 'increase-indent';
        increaseIndent.setAttribute('aria-label', 'Increase sidebar indent');
        increaseIndent.disabled = indent >= APPEARANCE.SIDEBAR_INDENT_MAX;
        indentRow.append(element('span', 'quick-label', 'Sidebar indent'), element('span', 'quick-space'), decreaseIndent, element('span', 'quick-value', `${indent}px`), increaseIndent);
        panel.append(indentRow);

        // 상태 줄 오른쪽 항목을 켜고 끈다. 사용량은 Quick settings에서 숨기며, 끄면 Claude의 원래 상태 줄도 되돌린다.
        panel.append(element('p', 'quick-title quick-section', 'Status bar'));
        for (const [preferenceName, label] of [[STATUS_ITEMS.CLAUDE, 'Claude usage'], [STATUS_ITEMS.CODEX, 'Codex usage'], [STATUS_ITEMS.MEMORY, 'Memory']]) {
            const row = element('label', 'quick-row');
            const checkbox = element('input', 'quick-check');
            checkbox.dataset.setting = preferenceName;
            checkbox.type = 'checkbox';
            checkbox.checked = this.isStatusItemOn(preferenceName);
            checkbox.addEventListener('change', () => this.run(() => this.setPreference(preferenceName, checkbox.checked)));
            row.append(element('span', 'quick-label', label), element('span', 'quick-space'), checkbox);
            panel.append(row);
        }

        const all = button([codicon('settings-gear'), element('span', '', 'All settings')], 'quick-all', () => {
            panel.hidePopover();
            this.run(() => this.commands.executeCommand('preferences:open'));
        });
        panel.append(all);
        // 값을 바꿔 요소를 다시 만들어도 조작 중인 입력의 초점과 스크롤 위치는 유지한다.
        if (focusedSetting) panel.querySelector(`[data-setting="${CSS.escape(focusedSetting)}"]`)?.focus({ preventScroll: true });
        panel.scrollTop = scrollTop;
    }

    // -- 본문 경로 줄 --

    /**
     * 본문 칸마다 생기는 탭 줄을 꾸민다.
     *
     * 터미널·파일 칸은 둘 이상일 때 입력받는 칸에만 얇은 선을 남긴다. 파일 이름과 닫기는 본문 위 탭 줄에 있다.
     * 설정처럼 파일 탭이 없는 화면은 이 줄에 화면 이름과 닫기 버튼을 둔다.
     */
    renderPathBars() {
        let isResized = false;
        const isSplit = [...this.shell.mainPanel.tabBars()].length > 1;
        for (const tabBar of this.shell.mainPanel.tabBars()) {
            const widget = tabBar.currentTitle?.owner;
            let bar = tabBar.node.querySelector(':scope > .path-bar');
            if (!bar) {
                bar = element('div', 'path-bar');
                tabBar.node.append(bar);
            }
            bar.replaceChildren();
            const isCompactPane = this.isTerminal(widget) || Boolean(widget?.getResourceUri?.()) || widget instanceof WebviewWidget && (this.webviewFolders.has(widget.id) || this.fileRoots.has(widget.id));
            // 칸이 둘 이상이면 입력이 가지 않는 칸을 흐리게 해 지금 입력할 칸을 드러낸다(탭 줄은 칸과 따로 있어 탭만으로는 칸을 가리키지 못한다).
            widget?.node.classList.toggle('is-inactive-pane', isSplit && widget !== this.currentWidget());
            isResized = isResized || tabBar.node.classList.contains('is-compact-pane') !== isCompactPane;
            tabBar.node.classList.toggle('is-compact-pane', isCompactPane);
            if (widget) {
                bar.classList.toggle('is-active', widget === this.currentWidget() && (!isCompactPane || isSplit));
                bar.onclick = () => this.run(() => this.activate(widget.id));
            }
            if (widget && !isCompactPane) {
                const parts = this.filePathParts(widget);
                // 파일이 아닌 화면(설정 등)은 폴더 없이 화면 이름만 보인다.
                if (parts.folder) bar.append(codicon('file'), element('span', 'path-folder', parts.folder), codicon('chevron-right'));
                bar.append(element('span', 'path-item', parts.item));
                bar.append(element('span', 'path-space'));
                if (parts.branch) bar.append(codicon('git-branch'), element('span', 'path-branch', parts.branch));
                const actions = element('span', 'path-actions');
                const close = button([codicon('close')], 'path-action', () => this.run(() => this.closeWidget(widget)));
                close.title = 'Close';
                actions.append(close);
                bar.append(actions);
            }
        }
        // 화면 이름 줄(30px)과 얇은 구분선(1px)이 바뀌면 본문 칸 배치를 다시 잰다.
        if (isResized) this.shell.mainPanel.fit();
    }

    /**
     * 파일 칸 경로 줄의 조각: 파일을 품은 작업 폴더 이름(작업 폴더 밖 파일이면 "File"), 그 폴더 기준 상대 경로, 폴더의 브랜치.
     * 파일이 아닌 화면(설정 등)은 폴더가 빈 값이고 화면 이름만 돌려준다.
     */
    filePathParts(
        widget,
    ) {
        const uri = widget.getResourceUri?.();
        const key = uri && model.folderContaining(this.state, uri.toString());
        const item = uri ? (key ? uri.toString().slice(key.length + 1) : uri.path.base) : widget.title.label;
        const folder = uri ? (key ? this.folderName(key) : 'File') : '';
        return { folder, item: decodeURIComponent(item), branch: key ? this.branchOf(key) : '' };
    }

    // -- 상태 줄 --

    renderStatus() {
        const current = this.currentWidget();
        const status = this.shell.footer.node.querySelector('#statusbar-right');
        // Each gauge reads its selected CLI environment; default values never describe a named account.
        const account = current?.options?.paddockAccount;
        this.usagePanel?.observe(current?.id, account, this.programs.get(current?.id));
        const saveable = current && Saveable.get(current);
        status.replaceChildren();
        const label = element('span', '', account ? this.accountLabel(current) : saveable ? (saveable.dirty ? 'Unsaved changes' : 'Saved') : this.isTerminal(current) ? (this.remote.alive ? `${this.remote.name}` : 'Local shell') : 'Ready');
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
        node.querySelector('.meter-percent').textContent = hasValue ? `${percent}%` : 'no data';
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

    /** 상태 줄의 Memory 항목을 그린다. 꺼져 있으면 다시 켜는 이름표만 둔다. */
    async refreshMemory() {
        const host = this.shell.footer.node.querySelector('.memory-usage');
        const isOn = this.isStatusItemOn(STATUS_ITEMS.MEMORY);
        if (host.dataset.state !== (isOn ? 'on' : 'off')) {
            host.dataset.state = isOn ? 'on' : 'off';
            host.replaceChildren(isOn ? this.memoryGroup() : this.offGroup('memory', 'Memory', STATUS_ITEMS.MEMORY));
        }
        if (isOn) {
            try {
                const { percent } = await this.fetchJson('/paddock/memory');
                if (percent !== null) {
                    this.fillMeter(host.querySelector('[data-meter="memory"]'), percent, `System memory in use: ${percent}%. Click to hide.`);
                }
            } catch {
                // 요청이 실패하면 마지막 값을 그대로 둔다. 처음부터 실패면 "—"가 남는다.
            }
        }
    }

    /** 켜진 Memory 항목. 누르면 숨긴다. */
    memoryGroup() {
        const group = this.usageGroup('memory', 'Memory', 'button');
        group.title = 'Click to hide memory';
        group.addEventListener('click', () => this.run(() => this.setPreference(STATUS_ITEMS.MEMORY, false)));
        const meter = this.meter('memory', '', null, 'System memory in use. Click to hide.');
        meter.querySelector('.meter-percent').textContent = '—';
        group.append(meter);
        return group;
    }

    /** 꺼진 상태 줄 항목: 표지·이름만 흐리게 보이고, 누르면 다시 켠다. 완전히 숨기면 다시 켤 곳이 없어 남겨 둔다. */
    offGroup(
        source,
        name,
        preferenceName,
    ) {
        const group = this.usageGroup(source, name, 'button');
        group.classList.add('is-off');
        group.title = `${name} is hidden. Click to show.`;
        group.setAttribute('aria-pressed', 'false');
        group.addEventListener('click', () => this.run(() => this.setPreference(preferenceName, true)));
        return group;
    }

    /** 상태 줄 항목이 켜져 있는지. 설정이 없으면 켜짐이다. */
    isStatusItemOn(
        preferenceName,
    ) {
        return this.preferences.get(preferenceName, true) !== false;
    }

    /** Reads account-scoped values; empty data and failed requests remain visible in the status bar. */
    async refreshUsage() {
        await this.usagePanel.refresh();
    }

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
