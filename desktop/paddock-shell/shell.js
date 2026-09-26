const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { Widget, Panel, BoxLayout } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { isOSX } = require('@theia/core/lib/common/os');
const markup = require('./shell-markup');

// 사이드바 폭. 세로 아이콘 레일은 두지 않고, 보기 전환은 사이드바 위 가로 줄에서 한다.
const SIDEBAR_WIDTH = 246;

/**
 * Paddock가 창 틀(추가 터미널 줄·사이드바·작업 폴더 탭 줄·상태 줄)을 그리고 본문은 Theia 편집 영역을 쓴다.
 *
 * 작업 폴더 탭 줄은 창 맨 위가 아니라 본문 바로 위에 둔다. 맨 위 줄은 폴더와 무관한 추가 터미널용이라,
 * 두 줄이 같은 높이에 있으면 어느 ＋가 어느 폴더에 여는지 헷갈린다.
 *
 * 본문은 여러 칸으로 나눌 수 있게 multiple-document 모드로 둔다. 칸마다 생기는 탭 줄은
 * 현재 항목 하나만 보이는 경로 줄로 꾸민다(workspace.js가 내용을 채운다).
 */
class PaddockShell extends ApplicationShell {
    createLayout() {
        this.addClass('paddock-shell');
        // macOS 창 버튼이 위쪽 줄 왼쪽에 겹치므로 그 자리를 비울지 CSS가 판단한다.
        this.toggleClass('is-macos', isOSX);
        this.mainPanel.mode = 'multiple-document';
        this.header = new Widget({ node: markup.header() });
        this.sidebar = new Widget({ node: markup.sidebar() });
        this.tabs = { node: this.header.node.querySelector('#tab-strip') };
        this.footer = new Widget({ node: markup.footer() });
        this.folderBar = new Widget({ node: markup.folderBar() });
        // 선택한 작업 폴더가 생기기 전(첫 실행)에는 줄을 숨긴다. workspace.js가 보이고 숨긴다.
        this.folderBar.hide();
        const body = new Panel({ layout: this.createBoxLayout(
            [this.folderBar, this.mainPanel], [0, 1], { direction: 'top-to-bottom', spacing: 0 },
        ) });
        const workspace = new Panel({ layout: this.createBoxLayout(
            [this.sidebar, body], [0, 1], { direction: 'left-to-right', spacing: 0 },
        ) });
        BoxLayout.setSizeBasis(this.sidebar, SIDEBAR_WIDTH);
        const layout = this.createBoxLayout(
            [this.header, workspace, this.footer], [0, 1, 0], { direction: 'top-to-bottom', spacing: 0 },
        );
        return layout;
    }

    async setLayoutData(
        layoutData,
    ) {
        await super.setLayoutData(layoutData);
        // 저장된 배치를 되살리면 모드도 되살아나므로 나눌 수 있는 모드를 다시 적용한다.
        this.mainPanel.mode = 'multiple-document';
    }

    async addWidget(
        widget,
        options,
    ) {
        // 터미널은 아래 패널이 아니라 본문 칸에 연다. 본문이 이미 터미널이라 자리를 하나로 둔다.
        const target = widget instanceof TerminalWidget ? { ...options, area: 'main' } : options;
        await super.addWidget(widget, target);
    }
}
decorate(injectable(), PaddockShell);
module.exports = { PaddockShell };
