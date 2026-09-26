const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { Widget, Panel, BoxLayout } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { isOSX } = require('@theia/core/lib/common/os');
const markup = require('./shell-markup');

/** Herdr가 창과 탭을 관리하고 파일 편집 위젯만 본문에 배치한다. */
class HerdrShell extends ApplicationShell {
    createLayout() {
        this.addClass('herdr-shell');
        // macOS 창 버튼이 탭 줄 왼쪽에 겹치므로 그 자리를 비울지 CSS가 판단한다.
        this.toggleClass('is-macos', isOSX);
        this.mainPanel.mode = 'single-document';
        this.header = new Widget({ node: markup.header() });
        this.rail = new Widget({ node: markup.rail() });
        this.sidebar = new Widget({ node: markup.sidebar() });
        this.tabs = { node: this.header.node.querySelector('#tab-strip') };
        this.footer = new Widget({ node: markup.footer() });
        const workspace = new Panel({ layout: this.createBoxLayout(
            [this.rail, this.sidebar, this.mainPanel], [0, 0, 1], { direction: 'left-to-right', spacing: 0 },
        ) });
        BoxLayout.setSizeBasis(this.rail, 52);
        BoxLayout.setSizeBasis(this.sidebar, 246);
        const layout = this.createBoxLayout(
            [this.header, workspace, this.footer], [0, 1, 0], { direction: 'top-to-bottom', spacing: 0 },
        );
        return layout;
    }

    async setLayoutData(
        layoutData,
    ) {
        await super.setLayoutData(layoutData);
        // 저장된 DockPanel 배치 복원은 기본 탭 모드도 되살리므로 제품의 탭 정책을 재적용한다.
        this.mainPanel.mode = 'single-document';
    }

    async addWidget(
        widget,
        options,
    ) {
        // 터미널은 파일과 같은 탭 영역에서 전환한다.
        const target = widget instanceof TerminalWidget ? { ...options, area: 'main' } : options;
        await super.addWidget(widget, target);
    }
}
decorate(injectable(), HerdrShell);
module.exports = { HerdrShell };
