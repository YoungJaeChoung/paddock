// 작업 화면 협력 클래스들이 함께 쓰는 DOM 도우미·저장 키·명령 정의.

// 작업 목록, 상단 터미널 묶음의 내부 탭, 창을 닫을 때 보던 위젯과 묶음별 칸 배치를 각각 저장한다.
class STORAGE {
    static WORK_FOLDERS = 'paddock.work-folders.v1';
    static INNER_TABS = 'paddock.inner-tabs.v1';
    static SHOWN_WIDGET = 'paddock.shown-widget.v1';
    static ROOT_LAYOUTS = 'paddock.root-layouts.v1';
}

// 상태 줄 오른쪽 항목별 표시 설정. 설정 화면·빠른 설정이 같은 값을 바꾸고, 사용량 항목은 전체 계정 목록을 연다. 기본은 모두 켜짐이다.
class STATUS_ITEMS {
    static CLAUDE = 'paddock.statusBar.claude';
    static CODEX = 'paddock.statusBar.codex';
    static MEMORY = 'paddock.statusBar.memory';
}

const MARKDOWN_PREVIEW = {
    COMMANDS: {
        preview: 'markdown-preview-enhanced.openPreview',
        both: 'markdown-preview-enhanced.openPreviewToTheSide',
    },
    VIEW_TYPE: 'markdown-preview-enhanced',
    EXTENSIONS: new Set(['.md', '.markdown', '.mdown', '.mkdn', '.mkd', '.rmd', '.qmd']),
};

function element(
    tag,
    className,
    text,
) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
}

function codicon(
    name,
) {
    return element('span', `codicon codicon-${name}`);
}

function button(
    label,
    className,
    action,
) {
    const node = element('button', className);
    node.type = 'button';
    if (typeof label === 'string') node.textContent = label;
    else if (label) node.append(...label);
    node.addEventListener('click', (event) => {
        event.stopPropagation();
        action(event);
    });
    return node;
}


module.exports = { STORAGE, STATUS_ITEMS, MARKDOWN_PREVIEW, element, codicon, button };
