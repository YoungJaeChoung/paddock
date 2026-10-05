const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { TerminalWidget } = require('@theia/terminal/lib/browser/base/terminal-widget');
const { Widget, Panel, SplitPanel } = require('@lumino/widgets');
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { isOSX, isWindows } = require('@theia/core/lib/common/os');
const { Saveable } = require('@theia/core/lib/browser/saveable');
const { ConfirmDialog } = require('@theia/core/lib/browser/dialogs');
const markup = require('./shell-markup');

// 사이드바 폭(px). 세로 아이콘 레일은 두지 않고, 보기 전환은 사이드바 위 가로 줄에서 한다.
// 사용자가 경계선을 끌어 바꾼 폭과 접힘 상태는 이 창의 브라우저 저장소에 남아 다음 실행에 되살린다.
class SIDEBAR {
    static DEFAULT_WIDTH = 272;
    static STORAGE_KEY = 'paddock.sidebar-width';
    static COLLAPSED_KEY = 'paddock.sidebar-collapsed';
}

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
 * Work navigation lives in the sidebar; the body tabs switch terminals and files inside the selected work.
 *
 * 본문은 여러 칸으로 나눌 수 있게 multiple-document 모드로 둔다. 칸마다 생기는 탭 줄은
 * 분할 중에는 실제 탭을 쓰고 한 칸일 때는 경로 줄로 꾸민다(workspace.js가 내용을 채운다).
 */
class PaddockShell extends ApplicationShell {
    createLayout() {
        this.addClass('paddock-shell');
        // macOS 창 버튼이 위쪽 줄 왼쪽에 겹치므로 그 자리를 비울지 CSS가 판단한다.
        this.toggleClass('is-macos', isOSX);
        this.toggleClass('is-linux', !isOSX && !isWindows);
        // 데스크톱 메뉴가 통합 제목줄을 만들 때만 표시한다. macOS는 시스템 메뉴를 쓴다.
        this.topPanel.hide();
        this.mainPanel.mode = 'multiple-document';
        this.header = new Widget({ node: markup.header() });
        this.sidebar = new Widget({ node: markup.sidebar() });
        this.folderBar = new Widget({ node: markup.folderBar() });
        const sidebarToggle = this.folderBar.node.querySelector('.sidebar-toggle');
        sidebarToggle.setAttribute('aria-keyshortcuts', isOSX ? 'Meta+B' : 'Control+B');
        sidebarToggle.addEventListener('click', () => this.toggleSidebar());
        this.footer = new Widget({ node: markup.footer() });
        // Recalculate the workspace when zoom or a narrow window changes the footer's CSS height.
        const footerResize = new ResizeObserver(() => this.fit());
        footerResize.observe(this.footer.node);
        this.disposed.connect(() => footerResize.disconnect());
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
        let sidebarVisible = true;
        try {
            sidebarVisible = window.localStorage.getItem(SIDEBAR.COLLAPSED_KEY) !== 'true';
        } catch {
            // 저장소를 못 쓰는 환경이면 사이드바를 펼친 채 시작한다.
        }
        this.setSidebarVisible(sidebarVisible);
        const layout = this.createBoxLayout(
            [this.topPanel, this.header, workspace, this.footer], [0, 0, 1, 0], { direction: 'top-to-bottom', spacing: 0 },
        );
        return layout;
    }

    /** 상단 버튼·명령·단축키에서 같은 사이드바 접기와 펼치기를 실행한다. */
    toggleSidebar() {
        this.setSidebarVisible(this.sidebar.isHidden);
    }

    /** 사이드바 내용과 폭을 유지하며 본문 공간을 넓히고, 재열기 버튼과 다음 실행의 상태를 맞춘다. */
    setSidebarVisible(
        visible,
    ) {
        const toggle = this.folderBar.node.querySelector('.sidebar-toggle');
        if (!visible) {
            // 빠른 설정이 열린 채 본체만 사라지지 않도록 함께 닫는다.
            for (const popover of this.sidebar.node.querySelectorAll('[popover]:popover-open')) popover.hidePopover();
            if (this.sidebar.node.contains(document.activeElement)) toggle.focus({ preventScroll: true });
        }
        this.sidebar.setHidden(!visible);
        const label = visible ? 'Hide sidebar' : 'Show sidebar';
        toggle.setAttribute('aria-expanded', String(visible));
        toggle.setAttribute('aria-label', label);
        toggle.title = `${label} (${isOSX ? '⌘B' : 'Ctrl+B'})`;
        toggle.querySelector('.codicon').className = `codicon codicon-layout-sidebar-left${visible ? '' : '-off'}`;
        try {
            window.localStorage.setItem(SIDEBAR.COLLAPSED_KEY, String(!visible));
        } catch {
            // 저장하지 못해도 이번 실행에서는 바꾼 표시 상태를 유지한다.
        }
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
        const widget = this.getWidgetById(id);
        if (widget?.secondaryWindow) {
            // The main document's focus tracker cannot wait for focus in a
            // different native window. Select the existing view there directly.
            widget.secondaryWindow.focus();
            widget.activate();
            return widget;
        }
        this.beforeActivate?.(id);
        return super.activateWidget(id);
    }

    async addWidget(
        widget,
        options,
    ) {
        // 터미널은 아래 패널이 아니라 본문 칸에 연다. 본문이 이미 터미널이라 자리를 하나로 둔다.
        const target = widget instanceof TerminalWidget && options?.area !== 'secondaryWindow' ? { ...options, area: 'main' } : options;
        await super.addWidget(widget, target);
    }

    /** 실행 중인 셸과 명령을 멈추기 전에 대상 터미널 수를 보여 주고 확인한다. */
    async confirmCloseTerminals(
        terminals,
    ) {
        let confirmed = true;
        if (terminals.length) {
            const dialog = new ConfirmDialog({
                title: terminals.length > 1 ? `Close ${terminals.length} terminals` : 'Close terminal',
                msg: 'Running shells and commands in these terminals will stop.',
                ok: 'Close',
                cancel: 'Cancel',
            });
            // 실행 중인 명령을 멈추는 확인이라 폴더 메뉴의 "Remove from list"와 같은 빨간 색 언어를 쓴다.
            dialog.node.classList.add('is-destructive');
            confirmed = await dialog.open();
        }
        return Boolean(confirmed);
    }

    /** 터미널 종료와 파일 저장을 확인한 뒤 일괄 닫는다. Close All Editors도 같은 종료 확인을 거친다. */
    async closeMany(
        targets,
        options,
    ) {
        const terminals = targets.filter(widget => widget instanceof TerminalWidget);
        let closed = [];
        // 파일의 Don't Save는 즉시 변경을 되돌린다. 터미널 확인에서 취소했을 때 편집 내용도 남도록 먼저 묻는다.
        if (!terminals.length) {
            closed = await super.closeMany(targets, options);
        } else if (await this.confirmCloseTerminals(terminals)
            && (options?.save === false || await Saveable.confirmSaveBeforeClose(targets, this.widgets.filter(widget => !targets.includes(widget))))) {
            closed = (await Promise.all(targets.map(async widget => {
                let result;
                if (widget instanceof TerminalWidget) {
                    await widget.closeWithoutSaving();
                    result = widget;
                } else {
                    result = await super.closeWidget(widget.id, options);
                }
                return result;
            }))).filter(Boolean);
        }
        return closed;
    }
}
decorate(injectable(), PaddockShell);
module.exports = { PaddockShell };
