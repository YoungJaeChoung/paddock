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
require('./controls.css');

// Paddock 설정. 설정 화면(Preferences)에서 "paddock"로 찾을 수 있다.
const PaddockPreferenceSchema = {
    properties: {
        'paddock.agentDoneSound': {
            type: 'boolean',
            default: true,
            description: 'Play a sound when an AI agent (Claude, Codex, …) finishes in a terminal you are not looking at.',
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

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ApplicationShell).to(PaddockShell).inSingletonScope();
    rebind(AboutDialog).to(PaddockAboutDialog).inSingletonScope();
    bind(PaddockWorkspace).toDynamicValue(({ container }) => new PaddockWorkspace(container)).inSingletonScope();
    bind(FrontendApplicationContribution).toService(PaddockWorkspace);
    bind(CommandContribution).toService(PaddockWorkspace);
    bind(KeybindingContribution).toService(PaddockWorkspace);
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockPdfOpener(container)).inSingletonScope();
    bind(OpenHandler).toDynamicValue(({ container }) => new PaddockBinaryNoticeOpener(container)).inSingletonScope();
    bind(PreferenceContribution).toConstantValue({ schema: PaddockPreferenceSchema });
});
