/**
 * Signature Help Extension for CodeMirror
 * 
 * Shows parameter hints while typing function calls.
 * Triggered by typing '(' or ',' inside function arguments.
 */

import { Extension, StateEffect, StateField } from "@codemirror/state";
import { showTooltip, Tooltip, EditorView, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { LanguageService, SignatureHelpResult } from "../../../services/language";

// State for current signature help
interface SignatureHelpState {
    result: SignatureHelpResult | null;
    pos: number;
}

const setSignatureHelp = StateEffect.define<SignatureHelpState | null>();

const signatureHelpState = StateField.define<SignatureHelpState | null>({
    create: () => null,
    update(state, tr) {
        for (const effect of tr.effects) {
            if (effect.is(setSignatureHelp)) {
                return effect.value;
            }
        }
        // Clear on document changes that might invalidate the help
        if (tr.docChanged && state) {
            return null;
        }
        return state;
    }
});

// Create tooltip from signature help state
function signatureHelpTooltip(state: SignatureHelpState): Tooltip {
    const { result, pos } = state;
    if (!result || result.signatures.length === 0) {
        return { pos, create: () => ({ dom: document.createElement("div") }) };
    }

    return {
        pos,
        above: true,
        arrow: true,
        create(view: EditorView) {
            const dom = document.createElement("div");
            dom.className = "cm-signature-help";

            const activeIdx = result.activeSignature ?? 0;
            const signature = result.signatures[activeIdx];
            if (!signature) {
                return { dom };
            }

            // Create signature label with highlighted parameter
            const labelDiv = document.createElement("div");
            labelDiv.className = "cm-signature-label";

            const activeParamIdx = result.activeParameter ?? 0;
            const params = signature.parameters || [];

            if (params.length > 0 && activeParamIdx < params.length) {
                // Try to highlight the active parameter in the label
                const label = signature.label;
                const activeParam = params[activeParamIdx];

                // Simple approach: just display label with parameter highlighted
                const paramStart = label.indexOf(activeParam.label);
                if (paramStart !== -1) {
                    const before = label.substring(0, paramStart);
                    const param = activeParam.label;
                    const after = label.substring(paramStart + param.length);

                    labelDiv.appendChild(document.createTextNode(before));
                    const highlight = document.createElement("b");
                    highlight.className = "cm-signature-active-param";
                    highlight.textContent = param;
                    labelDiv.appendChild(highlight);
                    labelDiv.appendChild(document.createTextNode(after));
                } else {
                    labelDiv.textContent = label;
                }
            } else {
                labelDiv.textContent = signature.label;
            }

            dom.appendChild(labelDiv);

            // Add documentation if available
            if (signature.documentation) {
                const docDiv = document.createElement("div");
                docDiv.className = "cm-signature-doc";
                docDiv.textContent = signature.documentation;
                dom.appendChild(docDiv);
            }

            // Add active parameter documentation
            if (params.length > 0 && activeParamIdx < params.length) {
                const activeParam = params[activeParamIdx];
                if (activeParam.documentation) {
                    const paramDoc = document.createElement("div");
                    paramDoc.className = "cm-signature-param-doc";
                    paramDoc.textContent = `${activeParam.label}: ${activeParam.documentation}`;
                    dom.appendChild(paramDoc);
                }
            }

            // Show signature index if multiple
            if (result.signatures.length > 1) {
                const indexDiv = document.createElement("div");
                indexDiv.className = "cm-signature-index";
                indexDiv.textContent = `${activeIdx + 1}/${result.signatures.length}`;
                dom.appendChild(indexDiv);
            }

            return { dom };
        }
    };
}

// Compute tooltip from state
const signatureHelpTooltipField = showTooltip.compute([signatureHelpState], (state) => {
    const help = state.field(signatureHelpState);
    if (!help || !help.result) return null;
    return signatureHelpTooltip(help);
});

// Theme for signature help
const signatureHelpTheme = EditorView.baseTheme({
    ".cm-signature-help": {
        backgroundColor: "var(--bg-secondary, #1e1e1e)",
        border: "1px solid var(--border-neutral, #444)",
        borderRadius: "4px",
        padding: "8px 12px",
        fontFamily: "var(--font-mono, monospace)",
        fontSize: "13px",
        maxWidth: "500px",
        zIndex: "100"
    },
    ".cm-signature-label": {
        color: "var(--text-primary, #e0e0e0)",
        marginBottom: "4px"
    },
    ".cm-signature-active-param": {
        color: "var(--accent-primary, #569cd6)",
        fontWeight: "bold"
    },
    ".cm-signature-doc": {
        color: "var(--text-secondary, #9e9e9e)",
        fontSize: "12px",
        marginTop: "4px",
        borderTop: "1px solid var(--border-neutral, #333)",
        paddingTop: "4px"
    },
    ".cm-signature-param-doc": {
        color: "var(--text-tertiary, #888)",
        fontSize: "11px",
        fontStyle: "italic",
        marginTop: "4px"
    },
    ".cm-signature-index": {
        color: "var(--text-tertiary, #666)",
        fontSize: "10px",
        marginTop: "4px",
        textAlign: "right"
    }
});

// Trigger characters for signature help
const TRIGGER_CHARS = ["(", ","];
const CLOSE_CHARS = [")"];

/**
 * Creates a signature help extension.
 * 
 * @param filePath - The file path for LSP requests
 */
export function signatureHelpExtension(filePath: string): Extension {
    let lastTriggerPos = -1;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    const plugin = ViewPlugin.define(view => {
        return {
            update(update: ViewUpdate) {
                // Check for trigger characters
                if (!update.docChanged) return;

                update.changes.iterChanges((fromA, toA, fromB, toB, inserted) => {
                    const text = inserted.toString();
                    const lastChar = text[text.length - 1];

                    if (TRIGGER_CHARS.includes(lastChar)) {
                        // Trigger signature help
                        lastTriggerPos = toB;
                        requestSignatureHelp(view, filePath, toB);
                    } else if (CLOSE_CHARS.includes(lastChar)) {
                        // Close signature help
                        view.dispatch({ effects: setSignatureHelp.of(null) });
                    }
                });
            },
            destroy() {
                if (debounceTimer) {
                    clearTimeout(debounceTimer);
                }
            }
        };
    });

    async function requestSignatureHelp(view: EditorView, path: string, pos: number) {
        // Debounce
        if (debounceTimer) {
            clearTimeout(debounceTimer);
        }

        debounceTimer = setTimeout(async () => {
            try {
                const line = view.state.doc.lineAt(pos);
                const lineNum = line.number - 1; // 0-based
                const char = pos - line.from;

                const result = await LanguageService.getSignatureHelp(path, lineNum, char);

                if (result && result.signatures.length > 0) {
                    view.dispatch({
                        effects: setSignatureHelp.of({ result, pos: lastTriggerPos })
                    });
                } else {
                    view.dispatch({ effects: setSignatureHelp.of(null) });
                }
            } catch (e) {
                console.warn("[SignatureHelp] Failed:", e);
            }
        }, 50);
    }

    return [
        signatureHelpState,
        signatureHelpTooltipField,
        signatureHelpTheme,
        plugin
    ];
}

/**
 * Manually trigger signature help at current cursor position
 */
export function triggerSignatureHelp(view: EditorView, filePath: string): void {
    const pos = view.state.selection.main.head;
    const line = view.state.doc.lineAt(pos);
    const lineNum = line.number - 1;
    const char = pos - line.from;

    LanguageService.getSignatureHelp(filePath, lineNum, char)
        .then(result => {
            if (result && result.signatures.length > 0) {
                view.dispatch({
                    effects: setSignatureHelp.of({ result, pos })
                });
            }
        })
        .catch(e => console.warn("[SignatureHelp] Manual trigger failed:", e));
}
