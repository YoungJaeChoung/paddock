Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { KeybindingContribution } = require('@theia/core/lib/browser/keybinding');
const { CommandContribution } = require('@theia/core/lib/common/command');
const { PreferenceContribution } = require('@theia/core/lib/common/preferences/preference-schema');
const { HerdrShell } = require('./shell');
const { HerdrWorkspace } = require('./workspace');
require('./controls.css');

// Herdr 설정. 설정 화면(Preferences)에서 "herdr"로 찾을 수 있다.
const HerdrPreferenceSchema = {
    properties: {
        'herdr.agentDoneSound': {
            type: 'boolean',
            default: true,
            description: 'Play a sound when an AI agent (Claude, Codex, …) finishes in a terminal you are not looking at.',
        },
    },
};
require('./style.css');

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ApplicationShell).to(HerdrShell).inSingletonScope();
    bind(HerdrWorkspace).toDynamicValue(({ container }) => new HerdrWorkspace(container)).inSingletonScope();
    bind(FrontendApplicationContribution).toService(HerdrWorkspace);
    bind(CommandContribution).toService(HerdrWorkspace);
    bind(KeybindingContribution).toService(HerdrWorkspace);
    bind(PreferenceContribution).toConstantValue({ schema: HerdrPreferenceSchema });
});
