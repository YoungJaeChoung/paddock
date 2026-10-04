const { TerminalWidgetImpl } = require('@theia/terminal/lib/browser/terminal-widget-impl');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { waitForClosed } = require('@theia/core/lib/browser/widgets');
const { injectable, inject, decorate } = require('@theia/core/shared/inversify');

/**
 * 실행 중인 터미널의 닫기 요청을 확인한 뒤 종료한다.
 * 탭 버튼·메뉴·단축키는 같은 닫기 계약을 쓰며, 셸 프로세스가 스스로 끝날 때의 dispose는 확인 없이 유지한다.
 */
class PaddockTerminal extends TerminalWidgetImpl {
    init() {
        super.init();
        // 밝은 테마에서도 명령과 출력의 색을 읽을 수 있도록 배경과의 최소 명암비를 유지한다.
        this.term.options.minimumContrastRatio = 4.5;
        // 많은 터미널을 열면 브라우저가 오래된 그래픽 자원을 회수할 수 있다.
        // 그래픽 연결이 끊기면 바로 기본 그리기로 돌아가 복구 대기 중 출력이 비지 않게 한다.
        const showText = () => queueMicrotask(() => {
            if (!this.isDisposed) this.webglAddon.dispose();
        });
        this.node.addEventListener('webglcontextlost', showText, true);
        this.toDispose.push({ dispose: () => this.node.removeEventListener('webglcontextlost', showText, true) });
        // 화면 밖에서 그래픽 복구 실패가 통지돼도 기존 출력과 계속 들어오는 내용을 보존한다.
        this.toDispose.push(this.webglAddon.onContextLoss(() => this.webglAddon.dispose()));
    }

    close() {
        return this.closeWithSaving();
    }

    /** 같은 터미널에 닫기를 반복 요청해도 확인 창은 하나만 연다. 취소하면 터미널은 그대로 남는다. */
    async closeWithSaving() {
        if (!this.isDisposed) {
            if (!this.pendingClose) {
                this.pendingClose = Promise.resolve().then(async () => {
                    if (await this.applicationShell.confirmCloseTerminals([this])) await this.closeWithoutSaving();
                }).finally(() => {
                    this.pendingClose = undefined;
                });
            }
            await this.pendingClose;
        }
    }

    /** 일괄 종료 확인을 이미 받은 터미널을 닫는다. Theia의 공개 닫기 계약과 같은 이름을 쓴다. */
    async closeWithoutSaving() {
        super.close();
        await waitForClosed(this);
    }
}
decorate(injectable(), PaddockTerminal);
decorate(inject(ApplicationShell), PaddockTerminal.prototype, 'applicationShell');

module.exports = { PaddockTerminal };
