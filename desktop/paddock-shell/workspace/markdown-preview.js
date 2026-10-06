const { CustomEditorWidget } = require('@theia/plugin-ext/lib/main/browser/custom-editors/custom-editor-widget');
const { WebviewWidget } = require('@theia/plugin-ext/lib/main/browser/webview/webview');
const { MARKDOWN_PREVIEW } = require('./shared');

/**
 * 마크다운 파일의 원문·미리보기·나란히 보기를 전환하고, 원문과 미리보기 화면의 짝을 기억한다.
 */
class MarkdownPreview {
    constructor(
        workspace,
    ) {
        this.workspace = workspace;
    }

    /** 현재 Markdown 파일의 원문 편집기. 미리보기 탭을 보고 있어도 연결된 원문을 찾는다. */
    markdownSourceWidget() {
        const current = this.workspace.currentWidget();
        const uri = current?.getResourceUri?.();
        const sourceUri = current instanceof WebviewWidget ? this.workspace.markdownPreviewSources.get(current.id) : uri?.toString();
        let source = null;
        if (sourceUri) {
            source = [...this.workspace.shell.mainPanel.widgets()].find(widget => widget.getResourceUri?.()?.toString() === sourceUri && !(widget instanceof CustomEditorWidget)) || null;
            if (source && !MARKDOWN_PREVIEW.EXTENSIONS.has(source.getResourceUri().path.ext.toLowerCase())) source = null;
        }
        return source;
    }

    /** 연결된 Markdown 미리보기 웹 화면을 찾는다. */
    markdownPreviewWidget(
        uri,
    ) {
        const sourceUri = uri.toString();
        const widgets = [...this.workspace.shell.mainPanel.widgets()];
        let preview = widgets.find(widget => widget instanceof WebviewWidget && this.workspace.markdownPreviewSources.get(widget.id) === sourceUri) || null;
        if (!preview) {
            const matching = widgets.filter(widget => widget instanceof WebviewWidget
                && widget.viewType === MARKDOWN_PREVIEW.VIEW_TYPE
                && !this.workspace.markdownPreviewSources.has(widget.id)
                && widget.title.label === `Preview ${uri.path.base}`);
            if (matching.length === 1) {
                preview = matching[0];
                this.workspace.markdownPreviewSources.set(preview.id, sourceUri);
            }
        }
        return preview;
    }

    /** 확장이 같은 칸의 미리보기 명령을 끝내기 전에 웹 화면을 추가하는 경우를 기다린다. */
    async waitForMarkdownPreview(
        uri,
    ) {
        let preview = this.markdownPreviewWidget(uri);
        if (!preview) {
            preview = await new Promise(resolve => {
                const listener = this.workspace.shell.onDidAddWidget(widget => {
                    if (widget instanceof WebviewWidget && widget.viewType === MARKDOWN_PREVIEW.VIEW_TYPE && widget.title.label === `Preview ${uri.path.base}`) {
                        clearTimeout(timer);
                        listener.dispose();
                        this.workspace.markdownPreviewSources.set(widget.id, uri.toString());
                        resolve(widget);
                    }
                });
                const timer = setTimeout(() => {
                    listener.dispose();
                    resolve(null);
                }, 3000);
            });
        }
        return preview;
    }

    /** 파일 보기·미리 보기·같이 보기 버튼이 원문과 렌더링 화면의 배치를 바꾼다. */
    async setMarkdownView(
        mode,
    ) {
        const source = this.markdownSourceWidget();
        if (source && ['file', 'preview', 'both'].includes(mode)) {
            const uri = source.getResourceUri();
            let preview = this.markdownPreviewWidget(uri);
            const wasSplit = Boolean(preview && this.workspace.shell.getTabBarFor(source) !== this.workspace.shell.getTabBarFor(preview));
            if (mode === 'file') {
                if (preview) await this.workspace.shell.closeWidget(preview.id);
                await this.workspace.activate(source.id);
            } else if (mode === 'both' && wasSplit) {
                // 기존 칸을 다시 추가하면 원문 칸을 반으로 나눈다. 이미 나란히 열려 있으면 선택만 바꿔 사용자가 끈 너비를 유지한다.
                await this.workspace.activate(source.id);
            } else {
                await this.workspace.plugins.willStart;
                const command = MARKDOWN_PREVIEW.COMMANDS[mode];
                if (!this.workspace.commands.getCommand(command)) {
                    await this.workspace.offerExtensionSearch('Install Markdown Preview Enhanced to preview this file.', 'Markdown Preview Enhanced');
                } else {
                    this.workspace.pendingMarkdownSource = uri.toString();
                    try {
                        await this.workspace.activate(source.id);
                        await this.workspace.commands.executeCommand(command);
                        preview = await this.waitForMarkdownPreview(uri);
                    } finally {
                        this.workspace.pendingMarkdownSource = null;
                    }
                    if (preview) {
                        const options = mode === 'both'
                            ? { area: 'main', mode: 'split-right', ref: source }
                            : { area: 'main', ref: source };
                        this.workspace.pendingMarkdownSource = uri.toString();
                        try {
                            await this.workspace.shell.addWidget(preview, options);
                        } finally {
                            this.workspace.pendingMarkdownSource = null;
                        }
                        // 처음 나란히 볼 때만 공간을 배분한다.
                        if (mode === 'both') await this.workspace.balanceSplit(preview, 'split-right');
                        await this.workspace.activate(mode === 'both' ? source.id : preview.id);
                    }
                }
            }
            this.workspace.refreshSoon();
        }
    }
}

module.exports = { MarkdownPreview };
