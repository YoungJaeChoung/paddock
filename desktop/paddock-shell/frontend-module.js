Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { KeybindingContribution } = require('@theia/core/lib/browser/keybinding');
const { CommandContribution } = require('@theia/core/lib/common/command');
const { PreferenceContribution } = require('@theia/core/lib/common/preferences/preference-schema');
const { PaddockShell } = require('./shell');
const { PaddockWorkspace } = require('./workspace');
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

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ApplicationShell).to(PaddockShell).inSingletonScope();
    bind(PaddockWorkspace).toDynamicValue(({ container }) => new PaddockWorkspace(container)).inSingletonScope();
    bind(FrontendApplicationContribution).toService(PaddockWorkspace);
    bind(CommandContribution).toService(PaddockWorkspace);
    bind(KeybindingContribution).toService(PaddockWorkspace);
    bind(PreferenceContribution).toConstantValue({ schema: PaddockPreferenceSchema });
});
