Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { AboutDialog } = require('@theia/core/lib/browser/about-dialog');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { KeybindingContribution } = require('@theia/core/lib/browser/keybinding');
const { CommandContribution } = require('@theia/core/lib/common/command');
const { PreferenceContribution } = require('@theia/core/lib/common/preferences/preference-schema');
const { PaddockShell } = require('./shell');
const { PaddockAboutDialog } = require('./about-dialog');
const { PaddockWorkspace } = require('./workspace');
const { PaddockPdfOpener } = require('./pdf-viewer');
const { PaddockBinaryNoticeOpener } = require('./binary-notice');
const { OpenHandler } = require('@theia/core/lib/browser/opener-service');
const { KeyboardLayoutService } = require('@theia/core/lib/browser/keyboard/keyboard-layout-service');
const { PaddockKeyboardLayoutService } = require('./keyboard-layout');
const { TerminalCopyOnSelectionHandler } = require('@theia/terminal/lib/browser/terminal-copy-on-selection-handler');
const { withQueuedCopy } = require('./terminal-copy');
const { LocalFileLinkProvider } = require('@theia/terminal/lib/browser/terminal-file-link-provider');
const { withoutBarePathLinks } = require('./terminal-links');
require('./controls.css');

// Paddock 설정. 설정 화면(Preferences)에서 "paddock"로 찾을 수 있다.
const PaddockPreferenceSchema = {
    properties: {
        'paddock.agentDoneSound': {
            type: 'boolean',
            default: true,
            description: 'Play a sound when an AI agent (Claude, Codex, …) finishes in a terminal you are not looking at.',
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
    bind(PaddockWorkspace).toDynamicValue(({ container }) => new PaddockWorkspace(container)).inSingletonScope();
    bind(FrontendApplicationContribution).toService(PaddockWorkspace);
    bind(CommandContribution).toService(PaddockWorkspace);
    bind(KeybindingContribution).toService(PaddockWorkspace);
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockPdfOpener(container)).inSingletonScope();
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockBinaryNoticeOpener(container)).inSingletonScope();
    bind(PreferenceContribution).toConstantValue({ schema: PaddockPreferenceSchema });
});
