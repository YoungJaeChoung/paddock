const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { Widget, Panel, SplitPanel } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { isOSX, isWindows } = require('@theia/core/lib/common/os');
const markup = require('./shell-markup');

// 사이드바 폭(px). 세로 아이콘 레일은 두지 않고, 보기 전환은 사이드바 위 가로 줄에서 한다.
// 사용자가 경계선을 끌어 바꾼 폭은 이 창의 브라우저 저장소에 남아 다음 실행에 되살린다.
const SIDEBAR = {
    DEFAULT_WIDTH: 272,
    STORAGE_KEY: 'paddock.sidebar-width',
};

/** 저장된 사이드바 폭. 없거나 읽을 수 없으면 기본 폭이다. */
function savedSidebarWidth() {
    let width = SIDEBAR.DEFAULT_WIDTH;
    try {
        const saved = Number(window.localStorage.getItem(SIDEBAR.STORAGE_KEY));
        if (saved > 0) width = saved;
    } catch {
        // 저장소를 못 쓰는 환경이면 기본 폭으로 시작한다.
    }
    return width;
}

/**
 * Paddock가 창 틀(추가 터미널 줄·사이드바·작업 폴더 탭 줄·상태 줄)을 그리고 본문은 Theia 편집 영역을 쓴다.
 *
 * 내부 터미널·파일 탭 줄은 창 맨 위가 아니라 본문 바로 위에 둔다. 맨 위 줄은 새 터미널 묶음용이라,
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
        this.toggleClass('is-linux', !isOSX && !isWindows);
        this.mainPanel.mode = 'multiple-document';
        this.header = new Widget({ node: markup.header() });
        this.sidebar = new Widget({ node: markup.sidebar() });
        this.tabs = { node: this.header.node.querySelector('#tab-strip') };
        this.footer = new Widget({ node: markup.footer() });
        this.folderBar = new Widget({ node: markup.folderBar() });
        // 작업 폴더가 없어도 빈 탭 줄은 남긴다. workspace.js가 선택한 폴더의 탭과 버튼을 채운다.
        const body = new Panel({ layout: this.createBoxLayout(
            [this.folderBar, this.mainPanel], [0, 1], { direction: 'top-to-bottom', spacing: 0 },
        ) });
        // 사이드바와 본문 사이 경계선을 끌어 사이드바 폭을 바꾼다. 창 크기가 바뀌면 본문만 늘고 준다.
        const workspace = new SplitPanel({ orientation: 'horizontal', spacing: 1 });
        workspace.addClass('paddock-workspace-split');
        workspace.addWidget(this.sidebar);
        workspace.addWidget(body);
        SplitPanel.setStretch(this.sidebar, 0);
        SplitPanel.setStretch(body, 1);
        const width = savedSidebarWidth();
        workspace.setRelativeSizes([width, Math.max(window.innerWidth - width, width)]);
        workspace.handleMoved.connect(() => {
            try {
                window.localStorage.setItem(SIDEBAR.STORAGE_KEY, String(Math.round(this.sidebar.node.getBoundingClientRect().width)));
            } catch {
                // 저장하지 못해도 이번 실행에서는 끈 폭 그대로 쓴다.
            }
        });
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

    /**
     * 위젯을 선택하기 전에 그 위젯의 묶음 배치를 본문에 되살린다(`beforeActivate`는 workspace.js가 건다).
     * Theia 명령이나 다른 확장이 직접 선택해도, 화면에서 빠져 있던 다른 묶음의 터미널이 본문에 나타난다.
     */
    async activateWidget(
        id,
    ) {
        this.beforeActivate?.(id);
        return super.activateWidget(id);
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
