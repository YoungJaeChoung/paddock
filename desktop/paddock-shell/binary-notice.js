const { BaseWidget } = require('@theia/core/lib/browser/widgets/widget');
const { ApplicationShell } = require('@theia/core/lib/browser/shell/application-shell');
const { OpenerService } = require('@theia/core/lib/browser/opener-service');
const { CommandRegistry } = require('@theia/core/lib/common/command');
const { FileService } = require('@theia/filesystem/lib/browser/file-service');

// 텍스트로 읽을 수 없는 파일의 확장자. 이 파일을 텍스트 편집기로 열면 "이진 파일이라 열면 느려질 수 있다"는 경고창이 뜨므로,
// 대신 볼 수 없다는 사실과 할 수 있는 일(폴더에서 보기·그래도 텍스트로 열기)을 파일 탭에 보여 준다.
// 이미지·오디오·영상·PDF는 각자의 뷰어가 열기 때문에 여기에 넣지 않는다.
const BINARY_EXTENSIONS = new Set([
    '.zip', '.gz', '.tgz', '.bz2', '.xz', '.7z', '.rar', '.tar', '.dmg', '.iso', '.jar', '.war', '.apk',
    '.exe', '.dll', '.so', '.dylib', '.o', '.a', '.lib', '.bin', '.dat', '.class', '.pyc', '.wasm',
    '.ttf', '.otf', '.woff', '.woff2', '.eot',
    '.db', '.sqlite', '.sqlite3',
    '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.psd', '.ai', '.sketch', '.fig',
]);

function formatSize(
    bytes,
) {
    let text = `${bytes} B`;
    if (bytes >= 1024 ** 3) text = `${(bytes / 1024 ** 3).toFixed(1)} GB`;
    else if (bytes >= 1024 ** 2) text = `${(bytes / 1024 ** 2).toFixed(1)} MB`;
    else if (bytes >= 1024) text = `${(bytes / 1024).toFixed(1)} KB`;
    return text;
}

/** 볼 수 없는 파일을 알리는 파일 탭. 파일 이름·크기와 두 가지 동작(폴더에서 보기, 그래도 텍스트로 열기)을 둔다. */
class PaddockBinaryNoticeWidget extends BaseWidget {
    constructor(
        uri,
        container,
    ) {
        super();
        this.uri = uri;
        this.container = container;
        this.id = `paddock-binary:${uri.toString()}`;
        this.addClass('paddock-binary-notice');
        this.title.label = uri.path.base;
        this.title.caption = uri.path.fsPath();
        this.title.closable = true;
        this.node.tabIndex = -1;
        this.body = document.createElement('div');
        this.body.className = 'binary-notice-body';
        this.node.append(this.body);
    }

    getResourceUri() {
        return this.uri;
    }

    /** 파일 크기를 읽어 안내 문구와 버튼을 그린다. 크기를 못 읽으면 크기 줄만 뺀다. */
    async load() {
        const stat = await this.container.get(FileService).resolve(this.uri, { resolveMetadata: true }).catch(() => undefined);
        const name = document.createElement('p');
        name.className = 'binary-notice-name';
        name.textContent = this.uri.path.base;
        const message = document.createElement('p');
        message.className = 'binary-notice-message';
        message.textContent = "This file can't be previewed here. It is a binary file, not text.";
        this.body.replaceChildren(name);
        if (typeof stat?.size === 'number') {
            const size = document.createElement('p');
            size.className = 'binary-notice-size';
            size.textContent = formatSize(stat.size);
            this.body.append(size);
        }
        const actions = document.createElement('div');
        actions.className = 'binary-notice-actions';
        actions.append(
            this.action('Show in file manager', () => this.container.get(CommandRegistry).executeCommand('revealFileInOS', this.uri)),
            this.action('Open as text anyway', () => this.openAsText()),
        );
        this.body.append(message, actions);
    }

    action(
        label,
        run,
    ) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'binary-notice-action';
        button.textContent = label;
        button.addEventListener('click', () => {
            Promise.resolve(run()).catch(() => undefined);
        });
        return button;
    }

    /** 사용자가 직접 고른 경우에만 텍스트 편집기로 연다. 편집기가 자체 경고창으로 한 번 더 확인한다. */
    async openAsText() {
        const openers = await this.container.get(OpenerService).getOpeners(this.uri);
        // 이 안내 탭을 제외한 우선순위 가장 높은 처리기가 텍스트 편집기다(Theia 편집기 미리보기 관리자).
        const text = openers.find(opener => opener.id !== 'paddock-binary-notice');
        if (text) {
            const widget = await text.open(this.uri, { mode: 'activate', preview: false, widgetOptions: { area: 'main', ref: this, mode: 'tab-after' } });
            if (widget) this.close();
        }
    }
}

/** 알려진 이진 확장자를 텍스트 편집기 대신 안내 탭으로 여는 열기 처리기. 같은 파일은 탭 하나를 재사용한다. */
class PaddockBinaryNoticeOpener {
    constructor(
        container,
    ) {
        this.container = container;
        this.id = 'paddock-binary-notice';
        this.label = 'File information';
        this.widgets = new Map();
    }

    canHandle(
        uri,
    ) {
        return BINARY_EXTENSIONS.has(uri.path.ext.toLowerCase()) ? 400 : 0;
    }

    async open(
        uri,
        options = {},
    ) {
        const shell = this.container.get(ApplicationShell);
        const key = uri.toString();
        let widget = this.widgets.get(key);
        if (!widget || widget.isDisposed) {
            widget = new PaddockBinaryNoticeWidget(uri, this.container);
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

module.exports = { PaddockBinaryNoticeWidget, PaddockBinaryNoticeOpener, BINARY_EXTENSIONS };
