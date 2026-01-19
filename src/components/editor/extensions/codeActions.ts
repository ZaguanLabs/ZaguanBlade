/**
 * Code Actions Extension for CodeMirror
 * 
 * Shows a lightbulb gutter marker when code actions (quick fixes) are available.
 * Clicking the lightbulb opens a menu to apply the fix.
 */

import { Extension, StateField, StateEffect, RangeSet } from "@codemirror/state";
import {
    EditorView,
    ViewUpdate,
    ViewPlugin,
    Decoration,
    DecorationSet,
    gutter,
    GutterMarker,
    WidgetType
} from "@codemirror/view";
import { LanguageService } from "../../../services/language";
import type { CodeActionInfo, LanguageDiagnostic } from "../../../types/blade";
import { applyWorkspaceEdit } from "../utils/applyEdit";

// Store current code actions per line
interface CodeActionsState {
    actions: Map<number, CodeActionInfo[]>; // line number -> actions
    filePath: string;
}

const setCodeActions = StateEffect.define<CodeActionsState>();

const codeActionsState = StateField.define<CodeActionsState>({
    create: () => ({ actions: new Map(), filePath: "" }),
    update(state, tr) {
        for (const effect of tr.effects) {
            if (effect.is(setCodeActions)) {
                return effect.value;
            }
        }
        // Clear on significant document changes but keep filePath if possible or reset
        // Actually we should prob just keep filePath if we had it, but for simplicity:
        if (tr.docChanged) {
            return { actions: new Map(), filePath: state.filePath };
        }
        return state;
    }
});

class LightbulbMarker extends GutterMarker {
    constructor(readonly actions: CodeActionInfo[]) {
        super();
    }

    toDOM(view: EditorView): Node {
        const span = document.createElement("span");
        span.className = "cm-lightbulb";
        span.textContent = "💡";
        span.title = `${this.actions.length} code action${this.actions.length > 1 ? 's' : ''} available`;
        return span;
    }

    eq(other: GutterMarker): boolean {
        return other instanceof LightbulbMarker &&
            this.actions.length === other.actions.length;
    }
}

const lightbulbGutter = gutter({
    class: "cm-lightbulb-gutter",
    markers(view) {
        const state = view.state.field(codeActionsState, false);
        if (!state) return RangeSet.empty;

        const markers: { from: number, to: number, value: GutterMarker }[] = [];
        for (const [lineNum, actions] of state.actions) {
            if (actions.length > 0 && lineNum <= view.state.doc.lines) {
                const line = view.state.doc.line(lineNum);
                markers.push({ from: line.from, to: line.from, value: new LightbulbMarker(actions) });
            }
        }
        return RangeSet.of(markers.map(m => m.value.range(m.from)), true);
    },
    domEventHandlers: {
        click(view, line, event) {
            const state = view.state.field(codeActionsState, false);
            if (!state) return false;

            const lineNum = view.state.doc.lineAt(line.from).number;
            const actions = state.actions.get(lineNum);

            if (actions && actions.length > 0) {
                showCodeActionsMenu(view, line.from, actions, state.filePath);
                return true;
            }
            return false;
        }
    }
});

// Show code actions menu
function showCodeActionsMenu(view: EditorView, pos: number, actions: CodeActionInfo[], filePath: string) {
    // Remove any existing menu
    const existing = document.querySelector(".cm-code-actions-menu");
    if (existing) existing.remove();

    // Create menu
    const menu = document.createElement("div");
    menu.className = "cm-code-actions-menu";

    // Position near the lightbulb
    const coords = view.coordsAtPos(pos);
    if (coords) {
        menu.style.position = "fixed";
        menu.style.left = `${coords.left}px`;
        menu.style.top = `${coords.bottom + 4}px`;
    }

    // Add menu items
    for (const action of actions) {
        const item = document.createElement("div");
        item.className = "cm-code-action-item";
        if (action.is_preferred) {
            item.classList.add("cm-code-action-preferred");
        }

        const icon = document.createElement("span");
        icon.className = "cm-code-action-icon";
        icon.textContent = getActionIcon(action.kind);
        item.appendChild(icon);

        const title = document.createElement("span");
        title.className = "cm-code-action-title";
        title.textContent = action.title;
        item.appendChild(title);

        item.onclick = () => {
            menu.remove();
            applyCodeAction(view, action, filePath);
        };

        menu.appendChild(item);
    }

    // Add close handler
    const closeHandler = (e: MouseEvent) => {
        if (!menu.contains(e.target as Node)) {
            menu.remove();
            document.removeEventListener("click", closeHandler);
        }
    };
    setTimeout(() => document.addEventListener("click", closeHandler), 0);

    document.body.appendChild(menu);
}

function getActionIcon(kind: string | null): string {
    if (!kind) return "🔧";
    if (kind.includes("quickfix")) return "🔧";
    if (kind.includes("refactor")) return "♻️";
    if (kind.includes("source")) return "📄";
    return "💡";
}

async function applyCodeAction(view: EditorView, action: CodeActionInfo, filePath: string) {
    console.log("[CodeAction] Apply:", action.title);

    if (action.edit) {
        const applied = applyWorkspaceEdit(view, filePath, action.edit);
        if (applied) {
            console.log("[CodeAction] Successfully applied edits to current view");
        } else {
            console.log("[CodeAction] Edits were not applicable to current view or empty");
        }
    } else {
        console.warn("[CodeAction] No edit attached to action:", action);
    }
}

// Theme for code actions
const codeActionsTheme = EditorView.baseTheme({
    ".cm-lightbulb-gutter": {
        width: "20px",
        cursor: "pointer"
    },
    ".cm-lightbulb": {
        fontSize: "14px",
        cursor: "pointer",
        opacity: "0.8",
        "&:hover": {
            opacity: "1",
            transform: "scale(1.1)"
        }
    },
    ".cm-code-actions-menu": {
        backgroundColor: "var(--bg-secondary, #1e1e1e)",
        border: "1px solid var(--border-neutral, #444)",
        borderRadius: "6px",
        boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
        padding: "4px 0",
        minWidth: "200px",
        maxWidth: "400px",
        zIndex: "1000",
        fontFamily: "var(--font-sans, system-ui)"
    },
    ".cm-code-action-item": {
        padding: "8px 12px",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: "8px",
        fontSize: "13px",
        color: "var(--text-primary, #e0e0e0)",
        "&:hover": {
            backgroundColor: "var(--bg-tertiary, #2a2a2a)"
        }
    },
    ".cm-code-action-preferred": {
        fontWeight: "600",
        color: "var(--accent-primary, #569cd6)"
    },
    ".cm-code-action-icon": {
        fontSize: "14px",
        width: "20px",
        textAlign: "center"
    },
    ".cm-code-action-title": {
        flex: "1"
    }
});

/**
 * Creates a code actions extension.
 * 
 * @param filePath - The file path for LSP requests
 */
export function codeActionsExtension(filePath: string): Extension {
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    let lastRequestedLines: Set<number> = new Set();

    const plugin = ViewPlugin.define(view => {
        // Initial fetch
        fetchCodeActionsForVisibleDiagnostics(view, filePath);

        return {
            update(update: ViewUpdate) {
                // Refetch on document changes or selection changes
                if (update.docChanged || update.selectionSet) {
                    if (debounceTimer) clearTimeout(debounceTimer);
                    debounceTimer = setTimeout(() => {
                        fetchCodeActionsForVisibleDiagnostics(update.view, filePath);
                    }, 500);
                }
            },
            destroy() {
                if (debounceTimer) clearTimeout(debounceTimer);
            }
        };
    });

    async function fetchCodeActionsForVisibleDiagnostics(view: EditorView, path: string) {
        // Get current selection/cursor position
        const sel = view.state.selection.main;
        const line = view.state.doc.lineAt(sel.head);
        const lineNum = line.number;

        // Only fetch for current line to avoid too many requests
        if (lastRequestedLines.has(lineNum)) return;
        lastRequestedLines.add(lineNum);

        // Clear old requested lines (keep last 5)
        if (lastRequestedLines.size > 5) {
            const arr = Array.from(lastRequestedLines);
            lastRequestedLines = new Set(arr.slice(-5));
        }

        try {
            const actions = await LanguageService.getCodeActions(
                path,
                lineNum - 1, // 0-based
                0,
                lineNum - 1,
                line.length
            );

            if (actions.length > 0) {
                const state = view.state.field(codeActionsState);
                const newActions = new Map(state.actions);
                newActions.set(lineNum, actions);
                view.dispatch({
                    effects: setCodeActions.of({ actions: newActions, filePath: path })
                });
            }
        } catch (e) {
            // Ignore errors - code actions are optional
        }
    }

    return [
        codeActionsState,
        lightbulbGutter,
        codeActionsTheme,
        plugin
    ];
}

/**
 * Manually request code actions for current selection
 */
export function requestCodeActions(view: EditorView, filePath: string): void {
    const sel = view.state.selection.main;
    const startLine = view.state.doc.lineAt(sel.from);
    const endLine = view.state.doc.lineAt(sel.to);

    LanguageService.getCodeActions(
        filePath,
        startLine.number - 1,
        sel.from - startLine.from,
        endLine.number - 1,
        sel.to - endLine.from
    ).then(actions => {
        if (actions.length > 0) {
            showCodeActionsMenu(view, sel.from, actions, filePath);
        }
    }).catch(e => console.warn("[CodeActions] Request failed:", e));
}
