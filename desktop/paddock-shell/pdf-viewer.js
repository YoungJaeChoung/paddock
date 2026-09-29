const { URI } = require('@theia/core');
const { BaseWidget, Widget } = require('@theia/core/lib/browser/widgets/widget');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');

// PDF 파일 탭. Electron에 들어 있는 PDF 뷰어(페이지 넘김·확대)를 iframe으로 그대로 쓴다.
// 파일 내용은 파일 서비스로 읽어 Blob 주소로 넘긴다 — 파일 경로를 직접 주소로 쓰면 창의 보안 설정에 막힐 수 있다.
class PaddockPdfWidget extends BaseWidget {
    constructor(
        uri,
        files,
    ) {
        super();
        this.uri = uri;
        this.files = files;
        this.id = `paddock-pdf:${uri.toString()}`;
        this.addClass('paddock-pdf');
        this.title.label = uri.path.base;
        this.title.caption = uri.path.fsPath();
        this.title.closable = true;
        this.iframe = document.createElement('iframe');
        this.iframe.className = 'paddock-pdf-frame';
        this.iframe.title = uri.path.base;
        this.node.append(this.iframe);
        this.node.tabIndex = -1;
    }

    /** 이 탭이 보여 주는 파일. 작업 폴더·파일 탭 줄이 파일 탭을 알아보는 데 쓴다. */
    getResourceUri() {
        return this.uri;
    }

    /** 파일을 다시 읽어 뷰어에 넘긴다. */
    async load() {
        const content = await this.files.readFile(this.uri);
        const previous = this.blobUrl;
        this.blobUrl = URL.createObjectURL(new Blob([content.value.buffer], { type: 'application/pdf' }));
        this.iframe.src = this.blobUrl;
        if (previous) URL.revokeObjectURL(previous);
    }

    onActivateRequest(
        message,
    ) {
        super.onActivateRequest(message);
        this.iframe.focus();
    }

    dispose() {
        if (this.blobUrl) URL.revokeObjectURL(this.blobUrl);
        super.dispose();
    }
}

/** `.pdf` 파일을 텍스트 편집기 대신 PDF 뷰어 탭으로 여는 열기 처리기. 같은 파일은 탭 하나를 재사용한다. */
class PaddockPdfOpener {
    constructor(
        container,
    ) {
        this.container = container;
        this.id = 'paddock-pdf';
        this.label = 'PDF viewer';
        this.widgets = new Map();
    }

    canHandle(
        uri,
    ) {
        return uri.path.ext.toLowerCase() === '.pdf' ? 500 : 0;
    }

    async open(
        uri,
        options = {},
    ) {
        const shell = this.container.get(ApplicationShell);
        const key = uri.toString();
        let widget = this.widgets.get(key);
        if (!widget || widget.isDisposed) {
            widget = new PaddockPdfWidget(uri, this.container.get(FileService));
            this.widgets.set(key, widget);
            widget.disposed.connect(() => this.widgets.delete(key));
            await widget.load();
        }
        if (!widget.isAttached) await shell.addWidget(widget, options.widgetOptions || { area: 'main' });
        if (options.mode !== 'reveal') await shell.activateWidget(widget.id);
        else await shell.revealWidget(widget.id);
        return widget;
    }
}

module.exports = { PaddockPdfWidget, PaddockPdfOpener };
