Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule } = require('@theia/core/shared/inversify');
const { ElectronMenuContribution } = require('@theia/core/lib/electron-browser/menu/electron-menu-contribution');
const { PaddockMenuContribution } = require('./titlebar');

// 기본 데스크톱 메뉴가 등록된 뒤 제품 제목줄로 바꾼다.
exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMenuContribution).to(PaddockMenuContribution).inSingletonScope();
});
