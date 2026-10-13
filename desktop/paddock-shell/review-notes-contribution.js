const { QuickInputService } = require('@theia/core/lib/common/quick-pick-service');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { DiffUris } = require('@theia/core/lib/browser/diff-uris');
const { EditorManager } = require('@theia/editor/lib/browser/editor-manager');
const { EDITOR_CONTEXT_MENU } = require('@theia/editor/lib/browser/editor-menu');
const { WorkspaceService } = require('@theia/workspace/lib/browser/workspace-service');
const { PaddockWorkspace } = require('./workspace');
const agent = require('./agent-model');
const { NOTES, relativePath, formatNotes } = require('./review-notes');

const COMMAND = {
    ADD: { id: 'paddock.reviewNotes.add', label: 'Paddock: Add Review Note on Selected Lines' },
    SEND: { id: 'paddock.reviewNotes.send', label: 'Paddock: Send Review Notes to Agent' },
    CLEAR: { id: 'paddock.reviewNotes.clear', label: 'Paddock: Clear Review Notes' },
};

/**
 * 편집기·변경 비교 화면에서 고른 줄에 검토 메모를 남기고, 모은 메모를 에이전트 터미널에 한 번에 붙여 넣는 명령.
 *
 * 변경 내용을 보며 "여기 고쳐 줘"를 여러 곳에 남긴 뒤 에이전트에게 한 번에 넘기려고 존재한다. 메모는 이 창 안에만 모으고(파일로 남기지 않음),
 * 붙여 넣기만 하고 Enter는 누르지 않아 사용자가 확인·보충한 뒤 보낼 수 있다. 보낸 뒤에는 비운다.
 */
class PaddockReviewNotes {
    constructor(
        container,
    ) {
        this.container = container;
        this.notes = [];
    }

    registerCommands(
        registry,
    ) {
        registry.registerCommand(COMMAND.ADD, { execute: () => this.add() });
        registry.registerCommand(COMMAND.SEND, { execute: () => this.send(), isEnabled: () => this.notes.length > 0 });
        registry.registerCommand(COMMAND.CLEAR, { execute: () => this.clear(), isEnabled: () => this.notes.length > 0 });
    }

    registerMenus(
        menus,
    ) {
        menus.registerMenuAction([...EDITOR_CONTEXT_MENU, 'paddock_review'], { commandId: COMMAND.ADD.id, label: 'Add Review Note…' });
        menus.registerMenuAction([...EDITOR_CONTEXT_MENU, 'paddock_review'], { commandId: COMMAND.SEND.id, label: 'Send Review Notes to Agent' });
    }

    registerKeybindings(
        keybindings,
    ) {
        keybindings.registerKeybinding({ command: COMMAND.ADD.id, keybinding: 'ctrlcmd+alt+m', when: 'editorTextFocus' });
    }

    /** 고른 줄에 대한 메모를 입력받아 목록에 더한다. 변경 비교 화면이면 오른쪽(바뀐 쪽) 파일의 줄을 가리킨다. */
    async add() {
        const messages = this.container.get(MessageService);
        const editor = this.container.get(EditorManager).currentEditor?.editor;
        if (!editor) {
            messages.info('Open a file or a diff and select the lines first.');
        } else if (this.notes.length >= NOTES.MAX_NOTES) {
            messages.warn(`Send the ${NOTES.MAX_NOTES} notes first, then add more.`);
        } else {
            const selection = editor.selection;
            const startLine = selection.start.line + 1;
            // 끝 위치가 줄의 맨 앞이면(줄 전체를 끌어 고른 경우) 그 줄은 고른 범위에 넣지 않는다.
            const endLine = Math.max(startLine, selection.end.character === 0 && selection.end.line > selection.start.line ? selection.end.line : selection.end.line + 1);
            const text = await this.container.get(QuickInputService).input({
                prompt: `Note for ${startLine === endLine ? `line ${startLine}` : `lines ${startLine}-${endLine}`}`,
                placeHolder: 'What should the agent change here?',
            });
            if (text && text.trim()) {
                const uri = DiffUris.isDiffUri(editor.uri) ? DiffUris.decode(editor.uri)[1] : editor.uri;
                const roots = this.container.get(WorkspaceService).tryGetRoots().map(root => root.resource.path.toString());
                this.notes.push({
                    file: relativePath(uri.path.toString(), roots),
                    startLine,
                    endLine,
                    lineText: editor.document.getLineContent(startLine),
                    text,
                });
                messages.info(`Review note added (${this.notes.length}).`);
            }
        }
    }

    /**
     * 에이전트 터미널에 메모 글을 붙여 넣고 목록을 비운다. 지금 보는 터미널이 에이전트면 그곳으로, 아니면 에이전트가 하나일 때 그곳으로,
     * 여럿이면 고르게 한다. 에이전트 터미널이 없으면 메모를 그대로 둔다.
     */
    async send() {
        const messages = this.container.get(MessageService);
        const workspace = this.container.get(PaddockWorkspace);
        const agents = workspace.terminals.all.filter(terminal => agent.isAgent(workspace.programOf(terminal)));
        const current = workspace.currentWidget();
        let target = agents.find(terminal => terminal === current);
        if (!target && agents.length === 1) target = agents[0];
        if (!target && agents.length > 1) {
            const picked = await this.container.get(QuickInputService).pick(
                agents.map(terminal => ({ label: terminal.title.label, description: workspace.programOf(terminal), terminal })),
                { placeHolder: 'Send review notes to which agent?' },
            );
            target = picked?.terminal;
        }
        if (!agents.length) {
            messages.info('No agent is running in a terminal. Start Claude or Codex in a terminal first; your notes are kept.');
        } else if (target) {
            target.paste(formatNotes(this.notes));
            await workspace.activate(target.id);
            this.notes = [];
            messages.info('Review notes pasted into the agent terminal. Press Enter there to send.');
        }
    }

    clear() {
        this.notes = [];
        this.container.get(MessageService).info('Review notes cleared.');
    }
}

module.exports = { PaddockReviewNotes, COMMAND };
