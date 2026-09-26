Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { HerdrShell } = require('./shell');
const { HerdrWorkspace } = require('./workspace');
require('./controls.css');
require('./style.css');

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ApplicationShell).to(HerdrShell).inSingletonScope();
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => new HerdrWorkspace(container)).inSingletonScope();
});
