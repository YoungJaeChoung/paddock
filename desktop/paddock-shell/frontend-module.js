Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { AboutDialog } = require('@theia/core/lib/browser/about-dialog');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { KeybindingContribution } = require('@theia/core/lib/browser/keybinding');
const { MenuContribution } = require('@theia/core/lib/common/menu');
const { CommandContribution } = require('@theia/core/lib/common/command');
const { PreferenceContribution } = require('@theia/core/lib/common/preferences/preference-schema');
const { PaddockShell } = require('./shell');
const { PaddockAboutDialog } = require('./about-dialog');
const { PaddockWorkspace } = require('./workspace');
const { PaddockExtensionOpen } = require('./extension-open');
const { withLocalReadme } = require('./extension-readme');
const { holdEnterWhileComposing } = require('./dialog-ime');
const { VSXExtensionsModel } = require('@theia/vsx-registry/lib/browser/vsx-extensions-model');
const { Endpoint } = require('@theia/core/lib/browser/endpoint');
const { PaddockBackendStop } = require('./backend-stop');
const { PaddockQuitPrompt } = require('./quit-prompt');
const { PaddockSetupNotices } = require('./setup-notices');
const { PaddockLayoutAutosave } = require('./layout-autosave');
const { PaddockReviewNotes } = require('./review-notes-contribution');
const { PaddockPdfOpener } = require('./pdf-viewer');
const { PaddockBinaryNoticeOpener } = require('./binary-notice');
const { OpenHandler } = require('@theia/core/lib/browser/opener-service');
const { KeyboardLayoutService } = require('@theia/core/lib/browser/keyboard/keyboard-layout-service');
const { PaddockKeyboardLayoutService } = require('./keyboard-layout');
const { TerminalCopyOnSelectionHandler } = require('@theia/terminal/lib/browser/terminal-copy-on-selection-handler');
const { withQueuedCopy } = require('./terminal-copy');
const { LocalFileLinkProvider } = require('@theia/terminal/lib/browser/terminal-file-link-provider');
const { withoutBarePathLinks } = require('./terminal-links');
const { LocalStorageService } = require('@theia/core/lib/browser/storage-service');
const { SearchInWorkspaceService } = require('@theia/search-in-workspace/lib/browser/search-in-workspace-service');
const { SearchInWorkspaceWidget } = require('@theia/search-in-workspace/lib/browser/search-in-workspace-widget');
const { SearchInWorkspaceFrontendContribution } = require('@theia/search-in-workspace/lib/browser/search-in-workspace-frontend-contribution');
const { withFolderSearchService, withScopeNotice, withFolderSearchView } = require('./search-in-folder');
const { WINDOW_ROLE, windowRole } = require('./window-role');
const { MainPluginApiProvider } = require('@theia/plugin-ext/lib/common/plugin-ext-api-contribution');
const { PaddockPluginApiOverrides } = require('./plugin-api-overrides');
const { PaddockReconnectGuard } = require('./reconnect-guard');
const { PaddockGitShim } = require('./git-shim');
require('./controls.css');

// Paddock 설정. 설정 화면(Preferences)에서 "paddock"로 찾을 수 있다.
const PaddockPreferenceSchema = {
    properties: {
        'paddock.agentDoneSound': {
            type: 'boolean',
            default: true,
            description: 'Play a sound when an AI agent (Claude, Codex, …) finishes in a terminal you are not looking at.',
        },
        'paddock.backend.onQuit': {
            type: 'string',
            enum: ['ask', 'keep', 'stop'],
            enumDescriptions: [
                'Ask each time you quit while terminals are running.',
                'Keep terminals and their programs running in the background. Opening Paddock again picks them up.',
                'Close every terminal and its programs when you quit.',
            ],
            default: 'ask',
            description: 'What to do with running terminals when you quit Paddock. With no terminals open nothing is kept. To stop background terminals once, run "Paddock: Quit and Stop All Terminals" from the command palette.',
        },
        'paddock.terminal.restoreOnRestart': {
            type: 'string',
            enum: ['always', 'never'],
            enumDescriptions: [
                'Show the last screen of each terminal when Paddock opens again after its terminals ended (quit with "Stop all", an update, a crash), open a new shell in the same folder, and resume the Claude or Codex conversation that was open.',
                'Open a plain new shell instead.',
            ],
            default: 'always',
            description: 'What to do with terminals whose programs ended while Paddock was closed. Paddock saves each terminal\'s last screen to its settings folder while it runs; "never" stops saving and deletes it.',
        },
        'paddock.agents.permissions': {
            type: 'string',
            enum: ['ask', 'allowSkip', 'skip'],
            enumDescriptions: [
                'Ask before each tool use, as the CLI does by default.',
                'Start asking, but let Claude switch to skipping permission prompts inside the session (Shift+Tab). Codex is unchanged; use /approvals there.',
                'Skip all permission prompts. Claude runs with --dangerously-skip-permissions and Codex with --dangerously-bypass-approvals-and-sandbox, so agents can change or delete files and run commands without asking. Use only in folders you can restore.',
            ],
            default: 'ask',
            description: 'Permission prompts for Claude and Codex that Paddock starts from Accounts. Applies to terminals opened after the change; agents you start by typing claude or codex follow their own settings.',
        },
        'paddock.agents.memoryNotice': {
            type: 'boolean',
            default: false,
            description: 'Tell Claude Code agents when memory is low. Adds one PreToolUse hook to Paddock\'s own Claude Code settings file, which Paddock passes to Claude with --settings in its account terminals and in terminals where claude starts through Paddock\'s launcher; your own settings.json files are never changed. While available memory is below the limit, the hook adds a short note to the agent\'s context after a Bash, Agent or Task tool call so it postpones heavy work. It never blocks or stops anything. Turning it off removes only that hook and quiets agents that are already running. Turning it on reaches agents started afterwards. Works for WSL, Linux and accounts that run in WSL; macOS and Windows-native Claude are not supported.',
        },
        'paddock.agents.memoryNoticeMinimumMb': {
            type: 'number',
            default: 4096,
            minimum: 0,
            description: 'Available memory in MB below which the memory notice is added. 0 keeps the notice quiet.',
        },
        'paddock.wsl.claudeLauncher': {
            type: 'string',
            enum: ['ask', 'on', 'off'],
            enumDescriptions: [
                'Ask once when Paddock starts on Windows with WSL.',
                'Keep one line in the WSL ~/.bashrc that puts Paddock\'s claude launcher first in PATH, so a claude you type in a WSL terminal also gets Paddock\'s status line and memory notice.',
                'Do not change the WSL ~/.bashrc. Turning this off removes the line Paddock added.',
            ],
            default: 'ask',
            description: 'Whether Paddock adds one line to the WSL ~/.bashrc so the claude you type in a WSL terminal opened from Paddock uses Paddock\'s launcher. The line ends with the comment "# paddock-claude-wrapper". Only used on Windows with WSL; Paddock account terminals do not need it.',
        },
        'paddock.interfaceFontFamily': {
            type: 'string',
            default: '',
            description: 'Font for the Paddock interface, including the sidebar and tabs. Leave empty to use the system font.',
        },
        'paddock.statusBar.claude': {
            type: 'boolean',
            default: true,
            description: 'Show Claude account usage in the status bar. Turning it off restores the previous status line in the default environment and registered Claude accounts.',
        },
        'paddock.statusBar.codex': {
            type: 'boolean',
            default: true,
            description: 'Show Codex account usage in the status bar.',
        },
        'paddock.statusBar.memory': {
            type: 'boolean',
            default: true,
            description: 'Show system memory use in the status bar.',
        },
        'paddock.sidebarIndent': {
            type: 'number',
            default: 12,
            minimum: 6,
            maximum: 24,
            description: 'Indentation in pixels for each level of the Work and file lists in the sidebar.',
        },
    },
};
require('./style.css');
require('./usage-panel.css');

exports.default = new ContainerModule((bind, unbind, isBound, rebind, unbindAsync, onActivation) => {
    rebind(ApplicationShell).to(PaddockShell).inSingletonScope();
    rebind(AboutDialog).to(PaddockAboutDialog).inSingletonScope();
    // 한글 입력 중에도 Cmd·Ctrl 단축키가 키 자리대로 듣게 한다.
    rebind(KeyboardLayoutService).to(PaddockKeyboardLayoutService).inSingletonScope();
    // 터미널 선택 복사: 끌어 고르는 동안의 클립보드 쓰기 순서를 보장한다(terminal-copy.js).
    // Theia 터미널 모듈이 이 모듈보다 뒤에 처리기를 묶으므로 rebind할 대상이 아직 없다. 만들어지는 순간 감싼다.
    onActivation(TerminalCopyOnSelectionHandler, (context, handler) => withQueuedCopy(handler));
    // 터미널 파일 경로 링크: 문장 끝 마침표 같은 `.`·`..`·`~` 하나를 경로 링크로 보지 않는다(terminal-links.js). 위와 같은 까닭으로 만들어지는 순간 감싼다.
    onActivation(LocalFileLinkProvider, (context, provider) => withoutBarePathLinks(provider));
    // 다른 창이 떠 있을 때 연 창은 저장소를 메모리로 쓴다(window-role.js). 첫 창의 배치를 되살려 같은 이름의 빈 셸을
    // 복제하지 않고, 이 창의 배치·작업 목록이 첫 창의 저장 내용을 덮어쓰지 않는다. 설정 파일은 창과 무관하게 함께 쓴다.
    onActivation(LocalStorageService, (context, service) => {
        if (windowRole(window.location.search) === WINDOW_ROLE.SECONDARY) service.storage = {};
        return service;
    });
    bind(PaddockWorkspace).toDynamicValue(({ container }) => new PaddockWorkspace(container)).inSingletonScope();
    // 파일 검색(Ctrl+Shift+F)은 작업 공간 없이도 사이드바 파일 영역의 폴더(없으면 선택한 터미널의 폴더)를 훑는다(search-in-folder.js).
    // Theia 검색 모듈이 이 모듈보다 먼저 묶여 rebind할 대상이 없으므로, 만들어지는 순간 감싼다.
    onActivation(SearchInWorkspaceService, (context, service) => withFolderSearchService(service, () => context.container.get(PaddockWorkspace).searchRoots()));
    onActivation(SearchInWorkspaceWidget, (context, widget) => withScopeNotice(widget, () => context.container.get(PaddockWorkspace).searchScopeName()));
    onActivation(SearchInWorkspaceFrontendContribution, (context, contribution) => withFolderSearchView(
        contribution,
        () => context.container.get(PaddockWorkspace).searchRoots(),
        options => context.container.get(PaddockWorkspace).workSidebar.showFilesSearch(options),
    ));
    bind(FrontendApplicationContribution).toService(PaddockWorkspace);
    bind(CommandContribution).toService(PaddockWorkspace);
    bind(KeybindingContribution).toService(PaddockWorkspace);
    // 확장이 알림 버튼에서 부르는 VS Code의 `extension.open`(확장 페이지 열기·업데이트)을 제공한다.
    bind(CommandContribution).toDynamicValue(({ container }) => new PaddockExtensionOpen(container)).inSingletonScope();
    // 설치한 확장의 README를 앱 백엔드에서 화면이 직접 받아 확장 페이지 본문에 보인다(extension-readme.js).
    onActivation(VSXExtensionsModel, (context, model) => withLocalReadme(model, new Endpoint().getRestUrl().toString()));
    // 한글 조합 중 누른 Enter가 대화상자를 먼저 닫아 마지막 글자가 터미널로 새지 않게, 조합이 끝난 뒤 확인한다(dialog-ime.js).
    bind(FrontendApplicationContribution).toConstantValue({ onStart: () => { holdEnterWhileComposing(window); } });
    bind(CommandContribution).toDynamicValue(({ container }) => new PaddockBackendStop(container)).inSingletonScope();
    // 마지막 창을 닫을 때 남은 터미널을 계속 실행할지 창 안에서 묻는다(quit-prompt.js).
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => new PaddockQuitPrompt(container)).inSingletonScope();
    // 시작한 뒤 한 번 알리거나 묻는 일: 예전 판의 Paddock 항목을 설정 파일에서 걷어 낸 결과 알림, WSL ~/.bashrc에 claude 실행 래퍼 PATH 줄을 넣어도 되는지 묻기(setup-notices.js).
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => new PaddockSetupNotices(container)).inSingletonScope();
    // 작업 중에도 화면 배치를 주기적으로 저장해, 앱이 강제로 끝나도 그 터미널 탭이 되살아나게 한다(layout-autosave.js).
    bind(PaddockLayoutAutosave).toSelf().inSingletonScope();
    bind(FrontendApplicationContribution).toService(PaddockLayoutAutosave);
    // 코드 줄에 남긴 검토 메모를 모아 에이전트 터미널에 한 번에 붙여 넣는다(review-notes.js).
    bind(PaddockReviewNotes).toDynamicValue(({ container }) => new PaddockReviewNotes(container)).inSingletonScope();
    bind(CommandContribution).toService(PaddockReviewNotes);
    bind(MenuContribution).toService(PaddockReviewNotes);
    bind(KeybindingContribution).toService(PaddockReviewNotes);
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockPdfOpener(container)).inSingletonScope();
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockBinaryNoticeOpener(container)).inSingletonScope();
    bind(PreferenceContribution).toConstantValue({ schema: PaddockPreferenceSchema });
    // 확장의 오류 모달을 알림으로 바꾸고 Git 출력 채널의 오류를 로그에 남긴다(plugin-api-overrides.js).
    bind(MainPluginApiProvider).toConstantValue(new PaddockPluginApiOverrides());
    // Windows에서 WSL 폴더의 저장소는 WSL 안의 git으로 실행한다: Git 보기의 `git.path` 기본값을 분배기로 정한다(git-shim.js).
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => new PaddockGitShim(container)).inSingletonScope();
    // 백엔드 연결 복구에 실패하면 화면을 새로 고치되, 저장하지 않은 편집 내용이 있으면 막고 알린다(reconnect-guard.js).
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => new PaddockReconnectGuard(container)).inSingletonScope();
});
