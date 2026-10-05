Object.defineProperty(exports, "__esModule", { value: true });
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMenuContribution } = require('@theia/core/lib/electron-browser/menu/electron-menu-contribution');
const { PaddockMenuContribution } = require('./titlebar');
const { TerminalCopyOnSelectionHandler } = require('@theia/terminal/lib/browser/terminal-copy-on-selection-handler');
const { FrontendApplicationContribution } = require('@theia/core/lib/browser/frontend-application-contribution');
const { SecondaryWindowService } = require('@theia/core/lib/browser/window/secondary-window-service');
const { ElectronSecondaryWindowService } = require('@theia/core/lib/electron-browser/window/electron-secondary-window-service');
const { forgetRestoredWidgets } = require('./tab-detach');

/**
 * 메인 창으로 되돌린 위젯이 보조 창 닫기를 막지 않게 하는 보조 창 서비스.
 * OS 닫기 버튼은 main 프로세스가 `canClose`로 위젯을 먼저 되돌린 뒤 창을 닫는다. 이때 Theia의 위젯 목록에
 * 되돌린 저장 안 한 에디터가 남아 있으면 보조 창의 beforeunload 검사가 닫기를 막아 빈 창이 남는다.
 * 되돌린 위젯을 목록에서 지워 빈 창은 닫히게 하고, 저장 안 한 내용과 표시는 메인 창의 에디터에 그대로 둔다.
 */
class PaddockSecondaryWindowService extends ElectronSecondaryWindowService {
    async restoreWidgets(
        newWindow,
        extractableWidget,
        shell,
    ) {
        const allMovedOrDisposed = await super.restoreWidgets(newWindow, extractableWidget, shell);
        forgetRestoredWidgets(newWindow.rootWidget);
        return allMovedOrDisposed;
    }
}
decorate(injectable(), PaddockSecondaryWindowService);

// 기본 데스크톱 메뉴가 등록된 뒤 제품 제목줄로 바꾼다.
exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMenuContribution).to(PaddockMenuContribution).inSingletonScope();
    rebind(SecondaryWindowService).to(PaddockSecondaryWindowService).inSingletonScope();
    // The terminal module may load later; use its initialized singleton after all modules have loaded.
    bind(FrontendApplicationContribution).toDynamicValue(({ container }) => ({
        onStart: () => {
            const handler = container.get(TerminalCopyOnSelectionHandler);
            handler.copy = async text => {
                if (text) window.electronTheiaCore.writeClipboard(text);
            };
        },
    }));
});
