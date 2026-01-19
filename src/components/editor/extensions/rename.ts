
import { EditorView } from "@codemirror/view";
import { keymap } from "@codemirror/view";
import { Extension } from "@codemirror/state";
import { LanguageService } from "../../../services/language";
import { showPanel, Panel } from "@codemirror/view";

// Rename panel component
function createRenamePanel(view: EditorView, initialName: string, onRename: (newName: string) => void, onClose: () => void): Panel {
    const dom = document.createElement("div");
    dom.className = "cm-rename-panel";
    dom.style.cssText = `
        padding: 5px 10px;
        background: var(--bg-secondary);
        border-bottom: 1px solid var(--border-color);
        display: flex;
        align-items: center;
        gap: 8px;
        font-family: var(--font-sans);
        font-size: 13px;
    `;

    const label = document.createElement("label");
    label.textContent = "Rename to:";
    dom.appendChild(label);

    const input = document.createElement("input");
    input.type = "text";
    input.value = initialName;
    input.className = "cm-rename-input";
    input.style.cssText = `
        background: var(--bg-tertiary);
        border: 1px solid var(--border-color);
        color: var(--text-primary);
        padding: 2px 6px;
        border-radius: 4px;
        flex-grow: 1;
        outline: none;
    `;
    // Select all text on focus
    setTimeout(() => {
        input.focus();
        input.select();
    }, 10);

    input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
            e.preventDefault();
            onRename(input.value);
            onClose();
        } else if (e.key === "Escape") {
            e.preventDefault();
            onClose();
        }
    });

    dom.appendChild(input);

    const applyBtn = document.createElement("button");
    applyBtn.textContent = "Apply";
    applyBtn.style.cssText = `
        background: var(--accent-primary);
        color: white;
        border: none;
        padding: 3px 8px;
        border-radius: 4px;
        cursor: pointer;
    `;
    applyBtn.onclick = () => {
        onRename(input.value);
        onClose();
    };
    dom.appendChild(applyBtn);

    const cancelBtn = document.createElement("button");
    cancelBtn.textContent = "Cancel";
    cancelBtn.style.cssText = `
        background: transparent;
        color: var(--text-secondary);
        border: 1px solid var(--border-color);
        padding: 3px 8px;
        border-radius: 4px;
        cursor: pointer;
    `;
    cancelBtn.onclick = onClose;
    dom.appendChild(cancelBtn);

    return {
        dom,
        top: true
    };
}

let activeRenamePanel: ((view: EditorView) => Panel) | null = null;

export function renameExtension(
    filePath: string,
    onApplyEdit?: (changes: Record<string, any[]>) => void
): Extension {
    return [
        keymap.of([{
            key: "F2",
            run: (view) => {
                const pos = view.state.selection.main.head;
                const line = view.state.doc.lineAt(pos);
                // Get word at cursor as initial value
                const word = view.state.wordAt(pos);
                const initialName = word ? view.state.sliceDoc(word.from, word.to) : "";

                // Show panel
                const closePanel = () => {
                    activeRenamePanel = null;
                    view.dispatch({
                        effects: renamePanelEffect.of(null)
                    });
                    view.focus();
                };

                const performRename = (newName: string) => {
                    const lineNum = line.number - 1;
                    const char = pos - line.from;

                    LanguageService.renameSymbol(filePath, lineNum, char, newName)
                        .then(edit => {
                            if (edit && edit.changes) {
                                // Apply changes
                                // Since workspace edits can affect multiple files, this often needs
                                // to be handled by the parent editor or workspace manager.
                                // For now, if we have a callback, use it.
                                if (onApplyEdit) {
                                    // Cast to any to bypass strict type checking for now since we know the structure
                                    onApplyEdit(edit.changes as any);
                                } else {
                                    console.log("Rename edi received but no handler:", edit);
                                }
                            }
                        })
                        .catch(err => {
                            console.error("Rename failed", err);
                        });
                };

                activeRenamePanel = (v) => createRenamePanel(v, initialName, performRename, closePanel);

                view.dispatch({
                    effects: renamePanelEffect.of(activeRenamePanel)
                });

                return true;
            }
        }]),
        renamePanelState
    ];
}

import { StateField, StateEffect } from "@codemirror/state";

const renamePanelEffect = StateEffect.define<((view: EditorView) => Panel) | null>();

const renamePanelState = StateField.define<((view: EditorView) => Panel) | null>({
    create: () => null,
    update(value, tr) {
        for (let e of tr.effects) if (e.is(renamePanelEffect)) value = e.value;
        return value;
    },
    provide: f => showPanel.from(f, v => v)
});
