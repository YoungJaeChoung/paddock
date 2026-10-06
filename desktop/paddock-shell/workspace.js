const { MessageService } = require('@theia/core/lib/common/message-service');
const { URI } = require('@theia/core');
const { OS } = require('@theia/core/lib/common/os');
const { Widget } = require('@lumino/widgets');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { OpenerService } = require('@theia/core/lib/browser/opener-service');
const { OpenWithService } = require('@theia/core/lib/browser/open-with-service');
const { WidgetManager } = require('@theia/core/lib/browser/widget-manager');
const { StorageService } = require('@theia/core/lib/browser/storage-service');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { ServiceConnectionProvider } = require('@theia/core/lib/browser/messaging/service-connection-provider');
const { CommandRegistry } = require('@theia/core/lib/common/command');
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
const { PreferencesWidget } = require('@theia/preferences/lib/browser/views/preference-widget');
const { ThemeService } = require('@theia/core/lib/browser/theming');
const model = require('./work-model');
const layoutModel = require('./layout-model');
const tabOverflow = require('./tab-overflow');
const { attachTabDrag, attachNativeTabDrag } = require('./tab-detach');
const { PaddockTerminal } = require('./terminal');
const wsl = require('./wsl-terminals');
const { distributionOf, withDistribution, environmentLabel } = require('./terminal-environment');
const cwdReport = require('./cwd-report');
const { UsagePanel } = require('./usage-panel');
const agentAccount = require('./agent-account');
const { STORAGE, STATUS_ITEMS, MARKDOWN_PREVIEW, element, button } = require('./workspace/shared');
const { RootSwitcher } = require('./workspace/root-switcher');
const { AgentActivity } = require('./workspace/agent-activity');
const { AccountTerminals } = require('./workspace/account-terminals');
const { WorkSidebar } = require('./workspace/work-sidebar');
const { FileTree } = require('./workspace/file-tree');
const { MarkdownPreview } = require('./workspace/markdown-preview');
const { FolderTabs } = require('./workspace/folder-tabs');
const { QuickSettings } = require('./workspace/quick-settings');
const { PathBar } = require('./workspace/path-bar');
const { StatusBar } = require('./workspace/status-bar');

// 터미널 현재 폴더·실행 중 프로그램·메모리처럼 이벤트가 없는 값을 다시 읽는 간격(ms).
const REFRESH_INTERVAL = 2000;

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

// 기본 셸을 열 수 없을 때 차례로 시도하는 프로필. Windows 기본 셸은 WSL인데 배포판이 없거나 Git이 없는 PC도 있다.
const FALLBACK_PROFILES = ['Git Bash', 'PowerShell'];

// 크기 변경 뒤 Git Bash에 빈 키(NUL)를 보내기까지 기다리는 시간. 콘솔이 크기 변경을 셸에 알릴 시간을 둔다(absorbResizeKeyLoss).
const RESIZE_KEY_GUARD_MS = 200;

/**
 * 사이드바(Work·Source control·Extensions), Unassigned 작업 목록, 내부 터미널·파일 탭 줄, 본문 경로 줄, 상태 줄을 Theia 서비스와 잇는다.
 *
 * 작업 폴더는 터미널에서 에이전트(claude·codex 등)가 실행될 때 그 터미널의 현재 폴더로 생긴다.
 * 폴더 선택 창이나 등록 버튼은 없다. 작업 폴더에 속한 터미널은 사이드바와 본문 위 탭 줄에,
 * 미연결 묶음은 Work의 Unassigned에 나오고 그 안의 터미널과 파일은 본문 위 탭 줄에 나온다.
 */
class PaddockWorkspace {
    constructor(
        container,
    ) {
        this.container = container;
        this.view = 'work';
        this.state = model.empty();
        this.workExpanded = true;
        this.filesExpanded = true;
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
        // Unassigned 묶음(시작 터미널 id)별로 마지막에 쓴 터미널. 사이드바 행을 누르면 이 터미널로 돌아간다.
        this.lastGroupTerminals = new Map();
        // 화면을 닫은 직후 다음에 보여 줄 터미널을 고르는 예약. 여러 화면을 한꺼번에 닫으면 처음 닫은 묶음을 기준으로 한 번만 고른다.
        this.closeFocus = null;
        this.closeFocusTimer = null;
        // 코드가 화면을 닫고 곧바로 다음 화면을 정하는 구간(폴더 제거, 전용 편집기로 다시 열기)의 수. 0보다 크면 위 예약을 건너뛴다.
        this.n_closesByCode = 0;
        this.fileRoots = new Map();
        this.webviewFolders = new Map();
        this.markdownPreviewSources = new Map();
        this.pendingMarkdownSource = null;
        this.homePath = '';
        // -- 에이전트 완료 알림 --
        this.activity = new Map();
        this.programs = new Map();
        // 실행 중인 claude·codex가 자기 환경 변수로 쓰는 계정(백엔드 판정). 환경을 읽지 못한 터미널은 값이 없다.
        this.agentAccounts = new Map();
        this.shellPids = new Map();
        this.doneIds = new Set();
        this.remoteWorkSnapshots = [];
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
        this.wslDistributions = [];
        this.defaultWslDistribution = '';
        this.wslHomePath = '';
        // -- 화면 조각·기능 묶음 --
        this.rootSwitcher = new RootSwitcher(this);
        this.agentActivity = new AgentActivity(this);
        this.accountTerminals = new AccountTerminals(this);
        this.workSidebar = new WorkSidebar(this);
        this.fileTree = new FileTree(this);
        this.markdownPreview = new MarkdownPreview(this);
        this.folderTabs = new FolderTabs(this);
        this.quickSettings = new QuickSettings(this);
        this.pathBar = new PathBar(this);
        this.statusBar = new StatusBar(this);
    }

    // -- 명령·단축키 --

    registerCommands(
        commands,
    ) {
        commands.registerCommand(PADDOCK_COMMANDS.accounts, { execute: () => this.run(() => this.accountTerminals.manageAccounts()) });
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
     * Ctrl+` 키는 Theia에서 아래 패널 터미널 열기·닫기(workbench.action.terminal.toggleTerminal)가 차지해 홈에 새 묶음을 만들었다.
     * 이를 걷어 내고 New Terminal(terminal:new:active:workspace)에 걸어, 본문 위 ＋처럼 지금 보는 폴더·묶음에 연다(routeTerminalCommands).
     */
    bindKeys() {
        for (const key of ['ctrl+shift+`', 'ctrlcmd+shift+5', 'ctrlcmd+\\', 'ctrl+`']) {
            this.keybindings.unregisterKeybinding(key);
        }
        this.keybindings.registerKeybinding({ command: 'terminal:new:active:workspace', keybinding: 'ctrl+`' });
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

    /**
     * Theia의 새 터미널 명령을 Paddock의 소속 규칙으로 보낸다. 어디서 불러도 지금 보는 맥락을 따른다.
     * Theia 처리기는 현재 폴더와 무관하게 홈의 새 Unassigned 묶음을 만들었다. 이제 그런 묶음은 사이드바 Work ＋ 하나만 새로 시작한다.
     * - New Terminal(terminal:new, terminal:new:active:workspace): 본문 위 ＋와 같은 판정(newTerminalFromFolderBar)으로 지금 보는 폴더·묶음에, 현재 칸의 탭으로 연다.
     *   메뉴 Terminal > New Terminal, 명령 팔레트, 터미널 우클릭 메뉴의 New Terminal과 Ctrl+`(bindKeys가 이 명령에 건다)가 모두 여기로 온다.
     *   아무 폴더·묶음도 보이지 않으면 Work ＋처럼 홈에서 새 Unassigned 묶음을 만든다.
     * - Split Terminal(terminal:split): 나누기 버튼과 같은 판정(newTerminalHere)으로 대상 터미널의 오른쪽 칸에 연다.
     * 대상 터미널을 먼저 고른 뒤 연다. 대상은 명령이 받은 인자로 정한다.
     * - 위젯: 분할 중 칸마다 보이는 Theia 탭 줄의 도구 버튼(＋, Split)은 그 칸의 위젯을 넘긴다. 터미널이 아닌 위젯(파일 편집기 칸)이면 대상이 없어 Split을 숨긴다.
     * - 마우스 이벤트: Theia 우클릭 메뉴는 우클릭 위치(이벤트)를 넘긴다. 우클릭한 터미널이 지금 보는 터미널이 아니어도 그 터미널 옆에 연다.
     *   다른 창으로 분리된 터미널의 이벤트도 같은 판정으로 찾는다(work-model의 pointerTargetOf).
     * - 그 밖(메뉴 막대·단축키·명령 팔레트는 인자가 없다): 지금 보는 터미널이다.
     * 분리된 창의 터미널은 칸을 나눌 수 없어 Split을 숨긴다. 그 터미널의 New Terminal은 고르지 않고 본문에 지금 보는 폴더·묶음에 연다.
     * 새 칸은 대상 터미널을 고른 직후에 생겨, Theia가 본문의 현재 탭을 새 터미널로 옮기지 않을 때가 있다. 연 터미널을 한 번 더 골라 탭 줄·사이드바의 현재 표시를 맞춘다.
     * Theia 터미널 모듈이 처리기를 등록한 뒤(레이아웃 초기화 뒤)에 불러야 앞자리에 선다.
     */
    routeTerminalCommands() {
        const target = arg => {
            const pointed = model.pointerTargetOf(arg);
            let terminal;
            if (arg instanceof Widget) terminal = arg;
            else if (pointed) terminal = this.terminals.all.find(item => item.node.contains(pointed));
            else terminal = this.currentWidget();
            return this.isTerminal(terminal) ? terminal : null;
        };
        // 나눌 수 있는 대상: 본문에 있는 터미널. 분리된 창의 터미널은 칸이 하나뿐이다.
        const splitTarget = arg => {
            const terminal = target(arg);
            return terminal && !terminal.secondaryWindow ? terminal : null;
        };
        const open = (terminal, split) => this.run(async () => {
            if (terminal) await this.activate(terminal.id);
            const opened = split ? await this.newTerminalHere({ split }) : await this.newTerminalFromFolderBar();
            if (opened && !opened.isDisposed) await this.activate(opened.id);
        });
        for (const id of ['terminal:new', 'terminal:new:active:workspace']) {
            this.commands.registerHandler(id, { execute: arg => open(splitTarget(arg)) });
        }
        this.commands.registerHandler('terminal:split', {
            isEnabled: arg => Boolean(splitTarget(arg)),
            isVisible: arg => Boolean(splitTarget(arg)),
            execute: arg => open(splitTarget(arg), 'split-right'),
        });
    }

    // -- 묶음별 본문 배치 --

    /** 위젯을 선택한다. 다른 묶음의 위젯이면 먼저 그 묶음의 배치를 본문에 되살린다. */
    async activate(
        id,
    ) {
        const widget = this.shell.getWidgetById(id);
        this.rootSwitcher.showRootOf(widget);
        const activation = this.shell.activateWidget(id);
        // 시작할 때 창에 입력 초점이 없어도 복원한 파일을 현재 화면으로 기억한다.
        // 같은 칸에서 이미 선택된 탭은 선택 변경 알림도 없으므로, 실제 선택을 확인해 직접 표시한다.
        if (widget && this.shell.getAreaFor(widget) === 'main' && this.shell.getTabBarFor(widget)?.currentTitle === widget.title) {
            this.shell.mainPanel.markAsCurrent(widget.title);
        }
        await activation;
        this.agentActivity.acknowledgeActivity(widget);
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
                layoutModel.prune(area, widget => !widget.isDisposed && (this.rootSwitcher.rootOf(widget) ?? root) === root),
            ]));
            const groupOf = widget => this.rootSwitcher.rootOf(widget) ?? this.displayedRoot;
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
                const root = this.rootSwitcher.rootOf(widget);
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
        this.shell.beforeActivate = id => this.rootSwitcher.showRootOf(this.shell.getWidgetById(id));
        // 닫기 확인창이 셸 제목(bash 등) 대신 탭·Work에 보이는 이름으로 대상을 밝힌다.
        this.shell.terminalName = terminal => this.terminalDisplayName(terminal);
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
            meter: this.statusBar.meter.bind(this.statusBar),
            onOpen: profile => this.accountTerminals.openUsageAccount(profile),
            onManage: () => this.run(() => this.accountTerminals.manageAccounts()),
            isClaudeChosen: () => {
                const inspected = this.preferences.inspect(STATUS_ITEMS.CLAUDE);
                return inspected?.globalValue !== undefined || inspected?.workspaceValue !== undefined;
            },
            onLegacyDisabled: () => this.quickSettings.setPreference(STATUS_ITEMS.CLAUDE, false),
            saveSelection: selection => this.storage.setData(STORAGE.USAGE_SELECTION, selection),
        });
        this.quickSettings.applyInterfacePreferences();
        this.preferences.onPreferenceChanged(({ preferenceName }) => {
            if ([STATUS_ITEMS.CLAUDE, STATUS_ITEMS.CODEX, STATUS_ITEMS.MEMORY].includes(preferenceName)) {
                // 상태 줄 항목을 켜고 끄면 바로 다시 그린다. Claude는 기본 환경과 등록된 모든 계정의 상태 줄도 맞춘다.
                if (preferenceName !== STATUS_ITEMS.MEMORY) this.usagePanel.data.preferencesChanged();
                this.run(async () => {
                    await this.statusBar.refreshMemory();
                    if (preferenceName !== STATUS_ITEMS.MEMORY) await this.usagePanel.refresh({ settings: preferenceName === STATUS_ITEMS.CLAUDE });
                });
                const panel = this.shell.sidebar.node.querySelector('#quick-settings');
                if (panel.matches(':popover-open')) this.quickSettings.renderQuickSettings();
            }
            if (preferenceName === 'paddock.interfaceFontFamily' || preferenceName === 'paddock.sidebarIndent') {
                this.quickSettings.applyInterfacePreferences();
                if (preferenceName === 'paddock.sidebarIndent') this.refreshSoon();
                const panel = this.shell.sidebar.node.querySelector('#quick-settings');
                if (panel.matches(':popover-open')) this.quickSettings.renderQuickSettings();
            }
        });
        this.shell.onDidAddWidget((widget) => {
            widget.title.changed.connect(() => this.refreshSoon());
            if (this.isTerminal(widget)) this.agentActivity.watchTerminal(widget);
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
            if (!this.isSwitchingRoot && widget && widget.id !== PreferencesWidget.ID && this.rootSwitcher.rootOf(widget)) {
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
                const groupRoot = this.isTerminal(newValue) ? this.topTerminalOf(newValue) : null;
                if (groupRoot) this.lastGroupTerminals.set(groupRoot, newValue.id);
                this.rootSwitcher.showRootOf(newValue);
                this.agentActivity.acknowledgeActivity(newValue);
                this.refreshSoon();
            }
        });
        this.shell.mainPanel.layoutModified.connect(() => this.pathBar.renderPathBars());
        // Git 보기는 선택된 저장소 하나만 보여 준다. 새 저장소가 열리면 지금 작업 폴더의 것인지 다시 맞추고, 브랜치가 바뀌면 탭 줄 표시도 다시 그린다.
        this.scm.onDidAddRepository((repository) => {
            repository.provider.onDidChange(() => {
                this.workSidebar.renderViewBadge();
                // 파일 목록의 변경 표시도 따라가야 하므로 늘 다시 그리기를 요청한다. 같은 내용이면 다시 그리기 판단 값이 같아 건너뛴다.
                this.refreshSoon();
            });
            this.refreshSoon();
        });
        this.scm.onDidChangeSelectedRepository(() => this.workSidebar.renderViewBadge());
        const sidebar = this.shell.sidebar.node;
        sidebar.querySelector('.view-bar').addEventListener('click', (event) => {
            const target = event.target.closest('[data-view]');
            if (target) this.run(() => this.workSidebar.showView(target.dataset.view));
        });
        sidebar.querySelector('.work-add').addEventListener('click', () => this.run(() => this.startWork()));
        sidebar.querySelector('.work-toggle').addEventListener('click', () => {
            this.workExpanded = !this.workExpanded;
            this.refreshSoon();
        });
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
        this.watchDisplayScale();
        this.shell.folderBar.node.querySelector('.folder-tab-add').addEventListener('click', (event) => this.workSidebar.openNewTerminalMenu(event.currentTarget));
        const accountPicker = this.shell.folderBar.node.querySelector('.account-picker');
        const accountMenu = this.shell.folderBar.node.querySelector('#account-menu');
        accountMenu.addEventListener('beforetoggle', event => {
            if (event.newState === 'open') {
                const bounds = accountPicker.getBoundingClientRect();
                accountMenu.style.left = `${Math.max(8, Math.min(bounds.left, window.innerWidth - 292))}px`;
                accountMenu.style.top = `${bounds.bottom + 6}px`;
                this.run(() => this.accountTerminals.renderAccountMenu());
            }
        });
        accountMenu.addEventListener('keydown', event => {
            const items = [...accountMenu.querySelectorAll('[role="menuitem"]:not(:disabled)')];
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
            if (target) this.run(() => this.markdownPreview.setMarkdownView(target.dataset.markdownView));
        });
        // 브랜치 표시를 누르면 Git 확장의 브랜치 전환 목록을 연다. 지금 작업 폴더의 저장소 경로를 넘겨 저장소를 다시 묻지 않게 한다.
        this.shell.folderBar.node.querySelector('.folder-bar-branch').addEventListener('click', () => this.run(async () => {
            const root = this.repositoryOf(this.shownFolder())?.provider.rootUri;
            await this.plugins.willStart;
            await this.commands.executeCommand('git.checkout', ...(root ? [new URI(root).path.fsPath()] : []));
        }));
        const nativeTabId = target => {
            const tab = target.closest('.lm-TabBar-tab');
            const bar = [...this.shell.mainPanel.tabBars()].find(item => item.contentNode.contains(tab));
            const index = bar && [...bar.contentNode.children].indexOf(tab);
            return bar?.titles[index]?.owner.id;
        };
        attachNativeTabDrag(this.shell.mainPanel.node, nativeTabId, id => this.run(() => this.folderTabs.moveTabToWindow(id)));
        this.shell.mainPanel.node.addEventListener('contextmenu', event => {
            const id = nativeTabId(event.target);
            if (id) {
                event.preventDefault();
                event.stopPropagation();
                const point = { x: event.clientX, y: event.clientY };
                const open = () => this.isTerminal(this.shell.getWidgetById(id)) ? this.workSidebar.openTerminalMenu(id, point) : this.folderTabs.openTabWindowMenu(id, point);
                if (event.buttons & 2) window.addEventListener('pointerup', () => setTimeout(open), { once: true, capture: true });
                else open();
            }
        }, true);
        for (const strip of [this.shell.folderBar.node.querySelector('.folder-tabs')]) {
            attachTabDrag(strip, id => this.run(() => this.folderTabs.moveTabToWindow(id)), {
                preview: (id, x, y) => this.folderTabs.previewTabDock(id, x, y),
                cancel: () => this.tabDockPreview?.remove(),
                drop: (id, x, y) => this.run(() => this.folderTabs.dockTab(id, x, y)),
                // 다른 창에 분리된 탭은 칸 대상이 없다(tabDockTarget). 끌 때 나누기 안내를 보이지 않는다.
                canDock: id => !this.shell.getWidgetById(id)?.secondaryWindow,
            });
            for (const scroller of strip.parentElement.querySelectorAll(':scope > .tabs-scroll')) {
                scroller.addEventListener('click', () => {
                    strip.scrollBy({ left: Number(scroller.dataset.direction) * strip.clientWidth * 0.8 });
                    this.folderTabs.alignTabsToEdge(strip);
                    this.folderTabs.updateTabOverflow(strip);
                });
            }
            strip.addEventListener('scroll', () => this.folderTabs.updateTabOverflow(strip), { passive: true });
            strip.addEventListener('keydown', event => tabOverflow.moveTabFocus(strip, event));
            // 창·사이드바 폭이 바뀌면 잘린 탭과 넘김 버튼을 맞춘다. 사용자가 다른 탭을 찾는 스크롤 위치는 유지한다.
            new ResizeObserver(() => this.folderTabs.updateTabOverflow(strip)).observe(strip);
        }
        this.shell.folderBar.node.querySelector('.folder-split-down').addEventListener('click', () => this.run(() => this.newTerminalHere({ split: 'split-bottom' })));
        this.shell.folderBar.node.querySelector('.folder-split-right').addEventListener('click', () => this.run(() => this.newTerminalHere({ split: 'split-right' })));
        const picker = sidebar.querySelector('.shell-picker');
        const menu = sidebar.querySelector('#shell-menu');
        menu.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.run(() => this.folderTabs.renderShellMenu());
                const bounds = picker.getBoundingClientRect();
                menu.style.left = `${Math.max(8, Math.min(bounds.left, window.innerWidth - 240))}px`;
                menu.style.top = `${bounds.bottom + 6}px`;
            }
        });
        menu.addEventListener('toggle', event => {
            picker.setAttribute('aria-expanded', String(event.newState === 'open'));
            if (event.newState === 'open') menu.querySelector('button')?.focus({ preventScroll: true });
        });
        menu.addEventListener('keydown', event => {
            const items = [...menu.querySelectorAll('[role="menuitem"]:not(:disabled)')];
            const index = items.indexOf(document.activeElement);
            let next;
            if (event.key === 'ArrowDown') next = (index + 1) % items.length;
            else if (event.key === 'ArrowUp') next = (index - 1 + items.length) % items.length;
            else if (event.key === 'Home') next = 0;
            else if (event.key === 'End') next = items.length - 1;
            else if (event.key === 'Escape') {
                menu.hidePopover();
                picker.focus({ preventScroll: true });
            }
            if (next !== undefined) {
                event.preventDefault();
                items[next]?.focus({ preventScroll: true });
            }
        });
        const quickSettings = sidebar.querySelector('#quick-settings');
        quickSettings.addEventListener('beforetoggle', (event) => {
            if (event.newState === 'open') {
                this.quickSettings.renderQuickSettings();
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
            if (quickSettings.matches(':popover-open')) this.quickSettings.renderQuickSettings();
        });
        this.shell.footer.node.querySelector('.remote-indicator').addEventListener('click', () => this.run(() => this.commands.executeCommand('remote.select')));
        this.files.onDidFilesChange((event) => {
            this.refreshRepositoriesOf(event.changes.map(change => change.resource));
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
        // WSL 터미널에 표지·COLORTERM을 넘길 때 사용자가 이미 정해 둔 WSLENV를 지우지 않으려고 읽어 둔다.
        this.windowsWslEnv = (await this.env.getValue('WSLENV'))?.value || '';
        // 첫 터미널을 열기 전에 WSL 배포판이 있는지 확인한다. 없으면 기본 셸(WSL) 대신 대체 셸로 연다.
        if (OS.backend.type() === OS.Type.Windows) {
            const { ready, home, distributions = [] } = await this.fetchJson('/paddock/wsl-ready').catch(() => ({ ready: false, home: '' }));
            this.isWslReady = ready === true;
            this.wslDistributions = distributions;
            this.defaultWslDistribution = distributionOf({}, home);
            this.wslHomePath = home ? URI.fromFilePath(home.replace(/\\/g, '/')).path.toString() : '';
        }
        const saved = await this.storage.getData(STORAGE.WORK_FOLDERS, '');
        // 지난 실행의 도구별 마지막 사용량 계정을 되살린다. 저장소는 작업 목록과 같은 시점에 읽는다 — 더 일찍 읽으면 빈 값이 온다.
        // 읽지 못하면 빈 선택으로 시작하고 이후 선택부터 저장한다.
        this.usagePanel.data.restoreSelection(await this.storage.getData(STORAGE.USAGE_SELECTION, {}).catch(() => ({})));
        void this.usagePanel.refresh();
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
        const restoredLayoutWidget = await this.rootSwitcher.restoreRootLayouts(shownWidgetId);
        this.isGroupsRestored = true;
        for (const terminal of this.terminals.all) {
            this.agentActivity.watchTerminal(terminal);
            if (this.shell.getAreaFor(terminal) !== 'main') {
                await this.shell.addWidget(terminal, { area: 'main' });
            }
        }
        // 저장된 작업 폴더나 파일 화면이 있어도 Unassigned에는 추가 터미널 하나를 둔다. 첫 화면에서 바로 명령을 입력할 수 있다.
        // 창을 닫을 때 보던 위젯의 묶음으로 시작한다. 그 위젯이 없으면 첫 Unassigned 터미널이다.
        // Theia는 닫을 때의 활성 위젯을 늘 남기지 않아(창이 닫히며 초점이 빠짐), 닫기 직전에 직접 저장한 위젯을 쓴다.
        const restored = restoredLayoutWidget ?? this.shell.getWidgetById(shownWidgetId) ?? this.currentWidget();
        const extra = this.topTerminals()[0];
        if (!extra) await this.newExtraTerminal();
        else await this.activate((restored && this.rootSwitcher.rootOf(restored) ? restored : extra).id);
        this.selectedTopTerminal = this.currentWidget()?.id || null;
        this.selectedFolder = this.workFolderOf(this.currentWidget()) || this.state.folders[0]?.key || null;
        for (const folder of this.state.folders) {
            this.openRepository(folder.key);
        }
        // 켜자마자 입력할 수 있게 본문에 보이는 터미널에 포커스를 준다.
        const shown = this.shell.mainPanel.currentTitle?.owner;
        if (this.isTerminal(shown)) await this.activate(shown.id);
        this.rootSwitcher.showRootOf(this.currentWidget());
        this.bindKeys();
        this.routeTerminalCommands();
        this.layoutReady = true;
        void this.accountTerminals.updateAccountLabels().catch(() => {});
        this.statusBar.refreshRemote();
        setInterval(() => this.run(() => this.agentActivity.tick()), REFRESH_INTERVAL);
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

    /** 다른 창에는 작업 폴더와 터미널의 이름·소속·프로그램만 알린다. 화면과 입력은 보내지 않는다. */
    async refreshWorkPresence() {
        try {
            const terminals = Object.entries(this.state.terminals).map(([id, entry]) => {
                const widget = this.shell.getWidgetById(id);
                return { id, folder: entry.folder, name: entry.name, program: widget ? this.programOf(widget) : '', terminalId: widget?.terminalId, shared: widget?.options.paddockShared };
            }).filter(terminal => !terminal.shared);
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
            this.folderTabs.renderFolderTabs();
            this.pathBar.renderPathBars();
            this.renderMainEmpty();
            this.statusBar.renderStatus();
            this.workSidebar.renderViewBadge();
            if (this.view === 'work') await this.workSidebar.renderWork();
        }
    }

    // -- 에이전트 완료 알림 --

    /**
     * 사이드바 행에 붙일 실행 중 계정 이름. 실행 중인 도구의 계정 표시(`도구 · 계정`)가 행 이름과 같거나 비어 있으면 빈 문자열이다.
     * 예: 일반 터미널에서 등록 계정 폴더로 codex 실행 → 'Work Codex', Claude 계정 터미널에서 기본 codex 실행 → 'Default'.
     * 기본 codex의 로그인이 등록 계정 'Work Codex' 하나와 같다고 확인되면 'Default' 대신 'Work Codex'다.
     */
    rowAccountName(
        terminal,
        rowName,
    ) {
        const label = this.agentActivity.runningAccountLabel(terminal);
        return label && label !== rowName ? agentAccount.scopeName(this.agentActivity.runningScope(terminal)) : '';
    }

    /** 탭과 Work 목록에 보이는 터미널 이름. 작업 폴더 터미널은 폴더 안 이름, 그 밖에는 Unassigned 이름이다. */
    terminalDisplayName(
        terminal,
    ) {
        const folder = model.folderOf(this.state, terminal.id);
        const row = folder && model.terminalRows(this.state, folder).find(item => item.id === terminal.id);
        return row ? `${row.name}${row.suffix}` : this.unassignedName(terminal);
    }

    /** 터미널에서 지금 실행 중인 프로그램 이름. 아직 모르면 터미널 제목(셸 이름)이다. */
    programOf(
        terminal,
    ) {
        return this.programs.get(terminal.id) || terminal.title.label;
    }

    terminalEnvironment(
        terminal,
    ) {
        return environmentLabel(terminal.options, this.cwdCache.get(terminal.id) || terminal.options.cwd, OS.backend.type());
    }

    /**
     * Sidebar sessions and their internal tabs share one name, including distinguishable default names.
     * 사용자가 바꾼 이름이 먼저이고, 다음은 계정 이름, 없으면 `terminal N`이다.
     * 번호는 처음 이름을 붙일 때 한 번 정해 유지한다. 앞 터미널이 닫히거나 폴더로 옮겨도 남은 터미널의 이름은 바뀌지 않는다.
     */
    unassignedName(
        terminal,
    ) {
        // 아직 번호가 없는 터미널(새 터미널, 번호 저장 전 버전에서 되살린 터미널)은 사이드바 순서대로 번호를 받는다.
        for (const id of this.topTerminals().flatMap(root => this.innerTabIds(root.id))) {
            const item = this.shell.getWidgetById(id);
            if (this.isTerminal(item) && !item.paddockNumber) this.terminalNumber(item);
        }
        return terminal.hasUserTitle ? terminal.title.label : this.accountTerminals.accountLabel(terminal) || `terminal ${this.terminalNumber(terminal)}`;
    }

    /**
     * Unassigned 터미널 기본 이름(`terminal N`)의 번호. 처음 부를 때 다른 Unassigned 터미널이 쓰지 않는 가장 작은 번호를 정해 터미널에 붙인다.
     * 번호는 Work 목록 전체(Unassigned와 작업 폴더 터미널의 `terminal N`)에서 겹치지 않게 고른다(model.folderTerminalName과 같은 규칙).
     */
    terminalNumber(
        terminal,
    ) {
        if (!(terminal.paddockNumber > 0)) {
            const used = new Set([...this.unassignedNumbers(terminal), ...model.usedTerminalNumbers(this.state)]);
            terminal.paddockNumber = model.freeTerminalNumber(used);
        }
        return terminal.paddockNumber;
    }

    /** 작업 폴더에 속하지 않은 터미널이 기본 이름에 쓰고 있는 번호. `except`는 번호를 새로 받을 터미널이다. */
    unassignedNumbers(
        except = null,
    ) {
        return new Set(this.terminals.all.filter(item => item !== except && !this.state.terminals[item.id])
            .map(item => item.paddockNumber).filter(Boolean));
    }

    /** Reuses the selected shell, without carrying account launch commands into a plain new terminal. */
    profileForTerminal(
        terminal,
    ) {
        const shellPath = terminal?.options?.shellPath;
        if (!shellPath) return undefined;
        const base = [...this.profiles.all].find(([, profile]) => profile instanceof ShellTerminalProfile
            && profile.shellPath?.toLowerCase() === shellPath.toLowerCase())?.[1];
        const args = Array.isArray(base?.options.shellArgs) ? base.options.shellArgs : [];
        const distribution = distributionOf(terminal.options, this.cwdCache.get(terminal.id));
        return new ShellTerminalProfile(this.terminals, {
            shellPath,
            shellArgs: wsl.isWslShell(shellPath) && distribution ? withDistribution(args, distribution) : args,
        });
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
            folder = model.folderOf(this.state, this.fileRoots.get(widget.id))
                || model.folderContaining(this.state, widget.getResourceUri().toString());
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
     * Unassigned 시작 터미널(작업 폴더·묶음 안 탭이 아닌 터미널)을 만든 순서대로 돌려준다.
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
        if (!this.isSwitchingRoot && (!widget.secondaryWindow || widget.isDisposed)) {
            // 소속을 지우기 전에 닫힌 화면의 묶음과 사이드바 순서를 기억해 둔다. 다음에 보여 줄 터미널을 그 순서로 고른다.
            this.scheduleCloseFocus(widget);
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
        } else if (!this.isSwitchingRoot && widget.secondaryWindow && !widget.isDisposed) {
            this.keepMainReferenceAfterDetach(widget);
        }
    }

    /**
     * 다른 창으로 분리한 탭이 본문의 현재 화면이었으면, 본문에 남은 화면을 새 기준으로 삼는다.
     * 기준이 비면 currentWidget이 없어 탭 줄·사이드바가 마지막으로 고른 작업 폴더로 돌아가, 본문과 다른 묶음을 보인다.
     * 분리 창으로 초점이 가므로 입력 초점은 옮기지 않고 현재 표시만 바꾼다.
     */
    keepMainReferenceAfterDetach(
        widget,
    ) {
        if (widget === this.lastMainWidget || this.shell.mainPanel.currentTitle?.owner === widget) {
            const next = [...this.shell.mainPanel.tabBars()].map(bar => bar.currentTitle?.owner)
                .find(item => item && item !== widget && !item.isDisposed && !item.secondaryWindow);
            if (next) {
                this.lastMainWidget = next;
                this.shell.mainPanel.markAsCurrent(next.title);
            }
        }
        this.refreshSoon();
    }

    /**
     * 터미널 묶음을 사이드바 순서로 돌려준다: 작업 폴더 차례, 그다음 Unassigned 묶음.
     * `key`는 rootOf와 같은 꼴(`folder:<폴더>`, `top:<시작 터미널 id>`)이고 `ids`는 그 묶음의 터미널 id다.
     */
    terminalGroups() {
        return [
            ...this.state.folders.map(folder => ({ key: `folder:${folder.key}`, ids: model.terminalsOf(this.state, folder.key) })),
            ...this.topTerminals().map(terminal => ({ key: `top:${terminal.id}`, ids: this.innerTabIds(terminal.id) })),
        ];
    }

    /**
     * 화면이 닫힌 뒤 다음에 보여 줄 터미널 고르기를 예약한다. 닫기 확인 창이 이전 초점을 되돌린 다음에 실행되도록 한 박자 늦춘다.
     * 분리 창으로 옮겨 본문에서 빠진 위젯은 닫힌 것이 아니므로 호출하지 않는다(forgetClosedWidget의 조건).
     * 코드가 닫고 다음 화면을 직접 정하는 구간(n_closesByCode > 0)에는 예약하지 않는다.
     */
    scheduleCloseFocus(
        widget,
    ) {
        // 코드가 닫는 구간은 그 코드가 다음 화면을 직접 정한다. 한 박자 뒤 다른 묶음 터미널을 여는 예약이 끼어들지 않게 한다.
        if (widget.id !== PreferencesWidget.ID && !(this.n_closesByCode > 0)) {
            if (!this.closeFocus) {
                const folder = this.isTerminal(widget) ? model.folderOf(this.state, widget.id) : null;
                const key = folder ? `folder:${folder}` : this.isTerminal(widget) ? `top:${this.innerTerminalRoots.get(widget.id) || widget.id}` : this.rootSwitcher.rootOf(widget);
                this.closeFocus = {
                    key,
                    groups: this.terminalGroups(),
                    closed: new Set(),
                    // 보이던 묶음의 화면을 닫았을 때만 초점을 옮긴다. 다른 묶음에 가려진 터미널이 끝났다고 보던 화면을 바꾸지 않는다.
                    wasShown: widget === this.lastMainWidget || Boolean(key && key === this.displayedRoot),
                };
            }
            this.closeFocus.closed.add(widget);
            clearTimeout(this.closeFocusTimer);
            this.closeFocusTimer = setTimeout(() => this.run(() => this.focusAfterClose()));
        }
    }

    /**
     * 닫은 뒤 남은 화면에 초점을 준다. 본문이 비었으면 같은 묶음의 다음 터미널, 없으면 사이드바에서 가장 가까운 묶음의 터미널을 연다.
     * 터미널이 하나도 남지 않으면 본문의 빈 상태 안내(renderMainEmpty)가 보인다.
     */
    async focusAfterClose() {
        const context = this.closeFocus;
        this.closeFocus = null;
        const shown = this.shell.mainPanel.currentTitle?.owner;
        const isMainEmpty = ![...this.shell.mainPanel.widgets()].length;
        let next = null;
        if (shown && !shown.isDisposed && !context.closed.has(shown)) {
            // 같은 칸의 다른 탭이 선택만 되고 입력 초점은 사라진 상태다. 남은 화면을 활성화해 바로 입력할 수 있게 한다.
            next = context.wasShown ? shown : null;
        } else if (context.wasShown || isMainEmpty) {
            const isOpen = id => {
                const widget = this.shell.getWidgetById(id);
                return this.isTerminal(widget) && !widget.isDisposed && !widget.secondaryWindow && !context.closed.has(widget);
            };
            const groups = context.groups.map(group => ({ key: group.key, ids: group.ids.filter(isOpen) }));
            // 닫는 동안 새로 생긴 묶음(첫 내부 터미널이 이어받은 묶음 등)도 후보에 넣는다.
            for (const group of this.terminalGroups()) {
                const ids = group.ids.filter(isOpen);
                if (ids.length && !groups.some(item => item.ids.some(id => ids.includes(id)))) groups.push({ key: group.key, ids });
            }
            const id = model.nearestTerminal(groups, context.key);
            // Unassigned 묶음으로 옮겨 가면 사이드바 행을 누를 때처럼 그 묶음에서 마지막으로 쓴 터미널을 연다.
            const root = id ? this.topTerminalOf(this.shell.getWidgetById(id)) : null;
            next = id ? this.shell.getWidgetById(root && isOpen(this.groupTerminal(root)) ? this.groupTerminal(root) : id) : null;
        }
        if (next) await this.activate(next.id);
        this.refreshSoon();
    }

    /** 본문에 화면이 없고 열린 터미널도 없으면 새 터미널을 여는 방법을 안내한다. 분리 창에 터미널이 있으면 보이지 않는다. */
    renderMainEmpty() {
        const panel = this.shell.mainPanel.node;
        let notice = panel.querySelector(':scope > .main-empty');
        if (!notice) {
            notice = element('div', 'main-empty');
            notice.append(element('p', 'work-empty-title', 'No open terminals'), element('p', 'work-empty', 'Use + in Work to open a terminal.'));
            panel.append(notice);
        }
        const isMainEmpty = ![...this.shell.mainPanel.widgets()].length;
        notice.hidden = !isMainEmpty || this.terminals.all.some(terminal => !terminal.isDisposed);
    }

    /** 전체 설정을 닫으면 이전 작업으로 돌아간다. 그 작업이 모두 끝났으면 남아 있는 다른 작업을 보여 준다. */
    async restoreWorkAfterPreferences() {
        if (this.displayedRoot === PreferencesWidget.ID) {
            const previous = this.preferencesReturnWidget;
            const previousRoot = previous && !previous.isDisposed ? this.rootSwitcher.rootOf(previous) : this.preferencesReturnRoot;
            const candidates = [previousRoot, ...this.rootLayouts.keys()].filter(root => root && root !== PreferencesWidget.ID);
            const root = candidates.find(name => layoutModel.widgetsOf(layoutModel.withoutDisposed(this.rootLayouts.get(name))).length);
            const area = root ? layoutModel.withoutDisposed(this.rootLayouts.get(root)) : null;
            const widget = previous && layoutModel.includes(area, previous) ? previous : layoutModel.widgetsOf(area)[0];
            if (widget) {
                this.rootSwitcher.showRoot(root, widget);
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
            if (this.lastGroupTerminals.has(id)) this.lastGroupTerminals.set(nextRoot, this.lastGroupTerminals.get(id));
        }
        this.lastGroupTerminals.delete(id);
        return nextRoot;
    }

    /**
     * Unassigned 묶음에서 마지막으로 쓴 터미널. 그 터미널이 닫혔거나 기록이 없으면 시작 터미널이다.
     * 마지막 터미널이 다른 창에 분리돼 있으면 시작 터미널이다. 행을 눌렀을 때 분리 창이 아니라 본문에 그 묶음을 보이려는 것이다.
     */
    groupTerminal(
        root,
    ) {
        const last = this.lastGroupTerminals.get(root);
        const isDetached = Boolean(last && this.shell.getWidgetById(last)?.secondaryWindow);
        return last && !isDetached && this.innerTabIds(root).includes(last) ? last : root;
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
        const isWslFolder = ['wsl.localhost', 'wsl$'].includes(new URI(cwd).authority.toLowerCase());
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
                const distribution = distributionOf(selected.options, cwd) || this.defaultWslDistribution;
                const args = Array.isArray(selected.options.shellArgs) ? selected.options.shellArgs : [];
                const shellArgs = distribution ? withDistribution(args, distribution) : args;
                selected = selected.modify({
                    id,
                    env: { ...selected.options.env, [wsl.MARKER]: id, WSLENV: wsl.forwardedWslEnv(this.windowsWslEnv) },
                    shellArgs: isWindowsHome ? ['--cd', '~', ...shellArgs] : shellArgs,
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

    /** Opens another view of the same backend shell without creating or owning a process. */
    async openRemoteTerminal(
        row,
        remote,
    ) {
        const existing = this.terminals.all.find(terminal => terminal.terminalId === remote.terminalId);
        if (existing) return this.activate(existing.id);
        const terminal = await this.terminals.newTerminal({ title: row.name, cwd: row.folder, paddockShared: true });
        try {
            await this.terminals.open(terminal, { widgetOptions: this.widgetOptions(), mode: 'activate' });
            await terminal.start(remote.terminalId);
            this.state = model.assignTerminal(this.state, terminal.id, row.folder, row.name);
            this.cwdCache.set(terminal.id, row.folder);
            this.selectedFolder = row.folder;
            await this.save();
            await this.refresh();
        } catch (error) {
            terminal.dispose();
            throw error;
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

    /** Refit the layout and terminal canvases after moving to a display with a different DPI. */
    watchDisplayScale() {
        let query;
        let timer;
        const refit = () => {
            clearTimeout(timer);
            timer = setTimeout(() => {
                this.shell.fit();
                this.shell.mainPanel.fit();
                // Layout messages must settle before measuring the terminal's new bounds.
                requestAnimationFrame(() => requestAnimationFrame(() => {
                    for (const terminal of this.terminals.all) {
                        if (!terminal.isVisible || terminal.isDisposed || !terminal.term?.element) continue;
                        terminal.term.clearTextureAtlas?.();
                        terminal.resizeTerminal();
                        terminal.term.refresh(0, terminal.term.rows - 1);
                    }
                }));
            }, 150);
        };
        const watch = () => {
            query?.removeEventListener('change', changed);
            query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
            query.addEventListener('change', changed);
        };
        const changed = () => {
            watch();
            refit();
        };
        watch();
        window.addEventListener('resize', refit);
        this.shell.disposed.connect(() => {
            clearTimeout(timer);
            query.removeEventListener('change', changed);
            window.removeEventListener('resize', refit);
        });
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
     * 내부 터미널이 없어도 시작 터미널을 돌려준다.
     */
    innerTabIds(
        root,
    ) {
        const children = [...this.innerTerminalRoots].filter(([, owner]) => owner === root).map(([id]) => id);
        const ids = [root, ...children].filter(id => this.terminals.all.some(terminal => terminal.id === id));
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
        else if (inner.length > 1) ids = inner;
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
     * 지금 보는 곳에서 새 터미널을 연다(나누기 버튼·단축키·본문 위 ＋·Theia의 New Terminal·Split Terminal).
     * 작업 폴더가 보이면 그 폴더의 작업 터미널, 아니면 현재 Unassigned 묶음의 내부 터미널이다.
     * 아무 폴더·묶음도 보이지 않으면 사이드바 Work ＋처럼 홈에서 새 Unassigned 묶음을 연다. 자리 판정은 work-model의 newTerminalPlace다.
     * 폴더 판정은 본문 위 탭 줄과 같은 shownFolder를 쓴다. 마지막 터미널을 닫아 빈 폴더를 보고 있어도 버튼마다 같은 폴더에 연다.
     * `split`은 열 자리다: `split-bottom`·`split-right`는 칸을 나누고, `tab-after`는 현재 칸의 탭으로 둔다. 없으면 현재 칸의 새 탭이다.
     */
    async newTerminalHere(
        { split } = {},
    ) {
        const current = this.currentWidget();
        const folderKey = this.shownFolder();
        const place = model.newTerminalPlace({
            folderKey,
            groupRoot: this.shownTopTerminal(),
            fileRoot: this.isFileOnlyGroup(current) ? this.fileRoots.get(current.id) : null,
        });
        let terminal;
        if (place === 'folder') terminal = await this.newWorkTerminal({ folderKey, split });
        else if (place === 'group') terminal = await this.newInnerTerminal({ split });
        else terminal = await this.startWork();
        return terminal;
    }

    /**
     * 본문 위 탭 줄 ＋ 메뉴의 Terminal: 작업 폴더가 보이면 그 안에, 아니면 현재 터미널의 묶음에 추가 터미널을 현재 칸의 탭으로 연다. 나누기 버튼과 같은 판정을 쓴다.
     * Theia의 New Terminal(메뉴·우클릭·Ctrl+`·명령 팔레트)도 이 동작을 따른다(routeTerminalCommands).
     */
    async newTerminalFromFolderBar() {
        const terminal = await this.newTerminalHere();
        return terminal;
    }

    /** 사이드바 Work ＋: 홈에서 새 Unassigned 묶음을 연다. 새 묶음을 시작하는 유일한 입구이며, 보이는 폴더·묶음이 없을 때 새 터미널도 이리로 온다. */
    async startWork() {
        this.workExpanded = true;
        const terminal = await this.newExtraTerminal();
        return terminal;
    }

    /** 아래 ＋로 위쪽 터미널 묶음 안에 터미널을 연다. 같은 묶음의 터미널과 파일 탭 옆에 나타난다. */
    async newInnerTerminal(
        { split } = {},
    ) {
        const current = this.currentWidget();
        const root = this.shownTopTerminal();
        const fileRoot = this.isFileOnlyGroup(current) ? this.fileRoots.get(current.id) : null;
        const cwd = this.isTerminal(current) ? await this.readCwd(current) : current?.getResourceUri?.()?.parent?.toString();
        const source = this.isTerminal(current) ? current : this.shell.getWidgetById(root);
        const terminal = await this.createTerminal(cwd || `file://${this.homePath}`, this.profileForTerminal(source));
        if (root) this.innerTerminalRoots.set(terminal.id, root);
        else if (fileRoot) this.moveFileGroup(fileRoot, terminal.id);
        await this.saveInnerTabs();
        // 파일만 남아 있으면 그 왼쪽에 새 시작 터미널을 두어 파일과 실행 화면을 함께 유지한다.
        const widgetOptions = fileRoot ? { area: 'main', mode: split || 'split-left', ref: current } : this.widgetOptions(split);
        await this.openTerminal(terminal, { widgetOptions, mode: 'activate' });
        await this.refresh();
        return terminal;
    }

    /** 나눠 열 때 기준 칸. 나누지 않으면 현재 칸의 새 탭으로 연다(기준이 없으면 Theia가 현재 탭 줄에 넣는다). */
    widgetOptions(
        split,
    ) {
        const current = this.currentWidget();
        return split && current ? { area: 'main', mode: split, ref: current } : { area: 'main' };
    }

    /**
     * Work의 Unassigned 목록에 새 터미널 묶음을 연다.
     * Work ＋와 환경 선택에서 열면 홈에서 시작한다. 보이는 폴더·묶음이 없을 때의 새 터미널도 Work ＋(startWork)를 거쳐 여기서 연다. 분할 옵션이 있으면 현재 터미널의 폴더를 따른다.
     * 연 터미널을 돌려준다.
     */
    async newExtraTerminal(
        { profile, split, cwd } = {},
    ) {
        const current = this.currentWidget();
        const nearby = cwd || (split && this.isTerminal(current) ? await this.readCwd(current) : null);
        const terminal = await this.createTerminal(nearby || `file://${this.homePath}`, profile);
        await this.openTerminal(terminal, { widgetOptions: this.widgetOptions(split), mode: 'activate' });
        if (this.view !== 'work') await this.workSidebar.showView('work');
        await this.refresh();
        return terminal;
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
        const current = this.currentWidget();
        const source = this.isTerminal(current) && this.workFolderOf(current) === folderKey ? current
            : model.terminalsOf(this.state, folderKey).map(id => this.shell.getWidgetById(id)).find(Boolean);
        const terminal = await this.createTerminal(folderKey, this.profileForTerminal(source));
        // 폴더의 첫 터미널도 Unassigned와 같은 `terminal N` 규칙이다(폴더마다 1부터). 번호는 지금 정해 이름에 저장하므로 다른 터미널을 닫아도 바뀌지 않는다.
        this.state = model.assignTerminal(this.state, terminal.id, folderKey, model.folderTerminalName(this.state, this.unassignedNumbers()));
        this.cwdCache.set(terminal.id, folderKey);
        this.selectedFolder = model.folderOf(this.state, terminal.id);
        await this.save();
        await this.openTerminal(terminal, { widgetOptions: this.widgetOptions(split), mode: 'activate' });
        this.view = 'work';
        await this.refresh();
        return terminal;
    }

    /**
     * 바뀐 파일을 품은 저장소마다 Git 확장에 다시 읽기를 요청한다.
     * Git 확장은 워크스페이스 밖 작업 폴더의 파일 변화를 스스로 듣지 못해서, 에이전트가 고친 파일이 변경 수·파일 목록 표시에 늦게 나타난다.
     * Git이 스스로 쓰는 색인·잠금 파일 변화는 무시한다(다시 읽기가 그 파일을 고쳐 끝없이 반복되지 않게). 같은 저장소의 요청은 0.5초 동안 모은다.
     */
    refreshRepositoriesOf(
        resources,
    ) {
        this.pendingRepositoryRefresh ??= new Map();
        for (const resource of resources) {
            const path = resource.path.toString();
            const isGitInternal = /\/\.git\/(index|.*\.lock$)/.test(path);
            const repository = isGitInternal ? null : this.scm.repositories
                // 저장소 안에 다른 저장소가 있으면 더 깊은(경로가 긴) 쪽이 그 파일의 저장소다.
                .filter(item => item.provider.rootUri && new URI(item.provider.rootUri).isEqualOrParent(resource))
                .sort((left, right) => right.provider.rootUri.length - left.provider.rootUri.length)[0];
            const root = repository ? new URI(repository.provider.rootUri).path.fsPath() : null;
            if (root && !this.pendingRepositoryRefresh.has(root)) {
                this.pendingRepositoryRefresh.set(root, setTimeout(() => {
                    this.pendingRepositoryRefresh.delete(root);
                    // 경로가 열린 저장소에 속하므로 Git 확장이 저장소 고르기 창을 띄우지 않는다.
                    this.commands.executeCommand('git.refresh', root).catch(() => undefined);
                }, 500));
            }
        }
    }

    openRepository(
        key,
    ) {
        // Git 확장은 워크스페이스 루트만 찾으므로 작업 폴더의 저장소를 직접 알려 준다. 저장소가 아니면 조용히 넘긴다.
        const path = new URI(key).path.fsPath();
        this.plugins.willStart.then(() => this.commands.executeCommand('git.openRepository', path)).catch(() => undefined);
        // 작업 폴더 바로 아래에 따로 clone한 저장소도 연다(VS Code의 기본 탐색 깊이 1과 같다).
        // 바깥 저장소가 그 폴더를 무시하면, 열지 않은 저장소의 변경은 파일 목록 표시와 Git 보기 어디에도 나타나지 않는다.
        this.files.resolve(new URI(key)).then(async (folder) => {
            const children = (folder.children ?? []).filter(child => child.isDirectory && child.name !== 'node_modules');
            const nested = await Promise.all(children.map(child => this.files.exists(child.resource.resolve('.git')).then(isRepository => (isRepository ? child : null))));
            await this.plugins.willStart;
            for (const child of nested.filter(Boolean)) {
                await this.commands.executeCommand('git.openRepository', child.resource.path.fsPath()).catch(() => undefined);
            }
        }).catch(() => undefined);
        // 파일 감시도 워크스페이스 루트에만 걸려 있어서, 그대로 두면 에이전트가 고친 파일을 Git 확장이 모르고
        // 변경 수·파일 목록 표시가 멈춘다. 작업 폴더마다 한 번 감시를 건다(내려받은 의존성 폴더는 뺀다).
        this.watchedFolders ??= new Map();
        if (!this.watchedFolders.has(key)) {
            this.watchedFolders.set(key, this.files.watch(new URI(key), { recursive: true, excludes: ['**/node_modules/**', '**/.venv/**'] }));
        }
    }

    async closeWidget(
        widget,
    ) {
        // 문서의 변경 여부와 저장 확인, 터미널의 종료 확인은 각 위젯의 닫기 계약을 따른다.
        // 닫힌 뒤 남은 화면의 선택과 입력 초점은 forgetClosedWidget이 예약한 focusAfterClose가 맞춘다.
        await this.shell.closeWidget(widget.id);
        await this.refresh();
    }

    /** Unassigned 묶음의 시작 터미널과 내부 터미널을 한 번의 확인으로 모두 닫는다. 파일 탭은 파일만 남은 묶음으로 남는다. */
    async closeTerminalGroup(
        root,
    ) {
        const terminals = this.innerTabIds(root).map(id => this.shell.getWidgetById(id)).filter(widget => widget && !widget.isDisposed);
        await this.shell.closeMany(terminals);
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
            // 남은 폴더 터미널이 닫히는 사이 잠시 Unassigned로 보이므로, 닫힘마다 다음 화면을 고르면 곧 닫힐 터미널이 깜빡인다.
            // 다음 화면은 아래 result.next가 정한다.
            this.n_closesByCode += 1;
            try {
                for (const terminal of terminals) {
                    await terminal.closeWithoutSaving();
                }
            } finally {
                this.n_closesByCode -= 1;
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

    /**
     * 터미널 이름을 바꾼다. 작업 터미널은 작업 목록에 저장한 이름을, Unassigned 터미널은 터미널 제목을 바꾼다.
     * Unassigned 이름은 사용자가 바꾼 제목을 먼저 쓰므로(unassignedName) 사이드바·탭 줄·칸 탭에 같이 반영되고 재시작 뒤에도 남는다.
     */
    async renameTerminal(
        id,
    ) {
        const entry = this.state.terminals[id];
        const terminal = this.shell.getWidgetById(id);
        const initialValue = entry ? entry.name : this.isTerminal(terminal) ? this.unassignedName(terminal) : '';
        const name = await new SingleTextInputDialog({ title: 'Rename terminal', initialValue }).open();
        if (name && this.state.terminals[id]) {
            this.state = model.renameTerminal(this.state, id, name);
            await this.save();
        } else if (name && this.isTerminal(terminal) && !terminal.isDisposed) {
            terminal.setTitle(name);
        }
        // 대화상자를 닫으면 초점이 사이드바·탭으로 돌아간다. 이름을 바꿨든 취소했든 지금 보는 터미널에 바로 입력할 수 있게 다시 선택한다.
        // 행을 두 번 누르면 첫 누름이 그 묶음의 마지막 터미널을 열므로, 이름을 바꾼 터미널과 지금 보는 터미널이 다를 수 있다.
        const current = this.currentWidget();
        if (this.isTerminal(current) && !current.isDisposed) await this.activate(current.id);
        await this.refresh();
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

    /** 전용 편집 기능이 없으면 이유를 알리고 필요한 확장의 검색 결과로 연결한다. */
    async offerExtensionSearch(
        message,
        query,
    ) {
        const action = await this.messages.warn(message, 'Find extension');
        if (action === 'Find extension') {
            await this.workSidebar.showView('extensions');
            this.container.get(VSXExtensionsSearchModel).query = query;
        }
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
        // 일반 편집기를 닫고 전용 편집기로 다시 여는 사이에는 닫힘 뒤 다른 묶음 터미널로 옮기지 않는다. 새 편집기가 같은 묶음에 열려야 한다.
        this.n_closesByCode += 1;
        let widget;
        try {
            if (opener.id.startsWith('custom-editor-')) {
                // 같은 파일의 일반 편집기가 열려 있으면 저장 여부를 먼저 결정한다.
                for (const item of this.shell.mainPanel.widgets()) {
                    if (!(item instanceof CustomEditorWidget) && item.getResourceUri?.()?.isEqual(uri)) {
                        await this.shell.closeWidget(item.id);
                        canOpen = this.shell.getAreaFor(item) !== 'main';
                    }
                }
            }
            if (canOpen) widget = await opener.open(uri, options);
        } finally {
            this.n_closesByCode -= 1;
        }
        if (canOpen) {
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

    // -- 상태 줄 --

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

    /** 상태 줄 항목이 켜져 있는지. 설정이 없으면 켜짐이다. */
    isStatusItemOn(
        preferenceName,
    ) {
        return this.preferences.get(preferenceName, true) !== false;
    }
}
module.exports = { PaddockWorkspace, PADDOCK_COMMANDS };
