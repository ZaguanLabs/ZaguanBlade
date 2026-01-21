/**
 * Diagnostics Extension for CodeMirror
 * 
 * Displays LSP diagnostics (errors, warnings, info) as underline decorations
 * in the editor. Listens for DiagnosticsUpdated events from the backend.
 */

import { StateField, StateEffect, RangeSet } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate, WidgetType } from "@codemirror/view";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BladeEventEnvelope, LanguageDiagnostic, LanguageRange } from "../../../types/blade";

// Effect to set diagnostics
export const setDiagnostics = StateEffect.define<LanguageDiagnostic[]>();

// Clear all diagnostics
export const clearDiagnostics = StateEffect.define<null>();

// State field to store current diagnostics
const diagnosticsState = StateField.define<LanguageDiagnostic[]>({
    create: () => [],
    update(diagnostics, tr) {
        for (const effect of tr.effects) {
            if (effect.is(setDiagnostics)) {
                return effect.value;
            }
            if (effect.is(clearDiagnostics)) {
                return [];
            }
        }
        return diagnostics;
    }
});

// Create decoration marks for diagnostics
const diagnosticDecorations = EditorView.decorations.compute(
    [diagnosticsState],
    (state) => {
        const diagnostics = state.field(diagnosticsState);
        const decorations: { from: number; to: number; decoration: Decoration }[] = [];

        for (const diag of diagnostics) {
            try {
                // Convert 0-based LSP lines to 1-based CodeMirror lines
                const startLineNum = diag.range.start.line + 1;
                const endLineNum = diag.range.end.line + 1;

                // Bounds check
                if (startLineNum < 1 || startLineNum > state.doc.lines) continue;
                if (endLineNum < 1 || endLineNum > state.doc.lines) continue;

                const startLine = state.doc.line(startLineNum);
                const endLine = state.doc.line(endLineNum);

                const from = startLine.from + Math.min(diag.range.start.character, startLine.length);
                const to = endLine.from + Math.min(diag.range.end.character, endLine.length);

                // Ensure from < to
                if (from >= to) continue;

                const severityClass = getSeverityClass(diag.severity);

                decorations.push({
                    from,
                    to,
                    decoration: Decoration.mark({
                        class: severityClass,
                        attributes: {
                            title: diag.message,
                            "data-diagnostic": "true"
                        }
                    })
                });
            } catch (e) {
                console.warn("[Diagnostics] Failed to create decoration:", e);
            }
        }

        // Sort by position
        decorations.sort((a, b) => a.from - b.from || a.to - b.to);

        return Decoration.set(decorations.map(d => d.decoration.range(d.from, d.to)));
    }
);

function getSeverityClass(severity: string): string {
    const lowerSeverity = severity.toLowerCase();
    if (lowerSeverity.includes("error") || severity === "1") {
        return "cm-diagnostic-error";
    } else if (lowerSeverity.includes("warning") || severity === "2") {
        return "cm-diagnostic-warning";
    } else if (lowerSeverity.includes("info") || severity === "3") {
        return "cm-diagnostic-info";
    } else {
        return "cm-diagnostic-hint";
    }
}

// Styles for diagnostic underlines
const diagnosticStyles = EditorView.baseTheme({
    ".cm-diagnostic-error": {
        textDecoration: "underline wavy #ef4444",
        textDecorationSkipInk: "none"
    },
    ".cm-diagnostic-warning": {
        textDecoration: "underline wavy #f59e0b",
        textDecorationSkipInk: "none"
    },
    ".cm-diagnostic-info": {
        textDecoration: "underline wavy #3b82f6",
        textDecorationSkipInk: "none"
    },
    ".cm-diagnostic-hint": {
        textDecoration: "underline dotted #6b7280",
        textDecorationSkipInk: "none"
    }
});

/**
 * Creates a diagnostics extension that listens for backend events.
 * 
 * @param filePath - The file path to filter events for
 */
export function diagnosticsExtension(filePath: string) {
    // Track the listener
    let unlistenFn: UnlistenFn | null = null;

    const plugin = ViewPlugin.define(view => {
        // Start listening for diagnostic events
        listen<BladeEventEnvelope>("blade-event", (event) => {
            const envelope = event.payload;

            // Check if it's a Language event
            if (envelope.event.type !== "Language") return;

            const langEvent = envelope.event.payload;
            if (langEvent.type !== "DiagnosticsUpdated") return;

            // Check if it's for our file
            if (langEvent.payload.file_path !== filePath) return;

            // Update diagnostics
            view.dispatch({
                effects: setDiagnostics.of(langEvent.payload.diagnostics)
            });
        }).then(fn => {
            unlistenFn = fn;
        });

        return {
            update(update: ViewUpdate) {
                // Could track document changes here if needed
            },
            destroy() {
                if (unlistenFn) {
                    unlistenFn();
                    unlistenFn = null;
                }
            }
        };
    });

    return [
        diagnosticsState,
        diagnosticDecorations,
        diagnosticStyles,
        plugin
    ];
}

/**
 * Get current diagnostics from editor state
 */
export function getDiagnostics(view: EditorView): LanguageDiagnostic[] {
    return view.state.field(diagnosticsState);
}
