import { StateField, StateEffect, RangeSet } from "@codemirror/state";
import { EditorView, Decoration, DecorationSet } from "@codemirror/view";

/**
 * Inline Diff Extension - Windsurf/Antigravity Style
 * 
 * Instead of replacing code with a block widget, this extension:
 * 1. Highlights removed lines with a red background
 * 2. Highlights added lines with a green background
 * 3. Shows the proposed new text inline (as ghost/addition lines)
 * 
 * The accept/reject actions are handled by a separate bottom bar component.
 */

// Data Model
export interface InlineDiffHunk {
    id: string;
    // Where in the document the old text starts
    fromLine: number;
    toLine: number;
    // The actual content
    oldText: string;
    newText: string;
}

export interface PendingInlineDiff {
    id: string;
    hunks: InlineDiffHunk[];
    path: string;
}

// Effects
export const setInlineDiff = StateEffect.define<PendingInlineDiff | null>();
export const clearInlineDiff = StateEffect.define<null>();

// Decoration styles
const removedLineDecoration = Decoration.line({
    class: "cm-diff-removed-line",
});

const addedLineDecoration = Decoration.line({
    class: "cm-diff-added-line",
});

const contextLineDecoration = Decoration.line({
    class: "cm-diff-context-line",
});

// State Field - tracks the current inline diff being previewed
export const inlineDiffField = StateField.define<{
    diff: PendingInlineDiff | null;
    decorations: DecorationSet;
}>({
    create() {
        return { diff: null, decorations: Decoration.none };
    },

    update(state, tr) {
        let diff = state.diff;
        let decorations = state.decorations;

        for (const e of tr.effects) {
            if (e.is(setInlineDiff)) {
                diff = e.value;

                if (diff) {
                    // Build decorations for all hunks
                    const decos: any[] = [];

                    for (const hunk of diff.hunks) {
                        // Mark removed lines (original code that will be replaced)
                        for (let line = hunk.fromLine; line <= hunk.toLine; line++) {
                            try {
                                const lineInfo = tr.state.doc.line(line);
                                decos.push(removedLineDecoration.range(lineInfo.from));
                            } catch {
                                // Line doesn't exist yet
                            }
                        }
                    }

                    // Sort decorations by position
                    decos.sort((a, b) => a.from - b.from);
                    decorations = Decoration.set(decos);
                } else {
                    decorations = Decoration.none;
                }
            } else if (e.is(clearInlineDiff)) {
                diff = null;
                decorations = Decoration.none;
            }
        }

        // Map decorations through document changes
        if (tr.docChanged && decorations !== Decoration.none) {
            decorations = decorations.map(tr.changes);
        }

        return { diff, decorations };
    },

    provide: f => EditorView.decorations.from(f, state => state.decorations)
});

// Theme for inline diff styling
export const inlineDiffTheme = EditorView.theme({
    ".cm-diff-removed-line": {
        backgroundColor: "rgba(239, 68, 68, 0.15)",
        borderLeft: "3px solid rgba(239, 68, 68, 0.6)",
    },
    ".cm-diff-added-line": {
        backgroundColor: "rgba(34, 197, 94, 0.15)",
        borderLeft: "3px solid rgba(34, 197, 94, 0.6)",
    },
    ".cm-diff-context-line": {
        backgroundColor: "rgba(161, 161, 170, 0.05)",
    },
    // Ghost text for additions (shown inline)
    ".cm-diff-ghost-text": {
        color: "rgba(34, 197, 94, 0.7)",
        fontStyle: "italic",
    },
});

// Helper to compute line ranges for diff highlighting
export function computeDiffLines(
    content: string,
    oldText: string,
    newText: string
): { removedLines: number[]; addedLines: number[] } | null {
    // Find where oldText appears in content
    const index = content.indexOf(oldText);
    if (index === -1) return null;

    // Count lines before the match
    const beforeMatch = content.substring(0, index);
    const startLine = (beforeMatch.match(/\n/g) || []).length + 1;

    // Count lines in the old text
    const oldLineCount = (oldText.match(/\n/g) || []).length + 1;
    const endLine = startLine + oldLineCount - 1;

    // Lines to mark as removed
    const removedLines: number[] = [];
    for (let i = startLine; i <= endLine; i++) {
        removedLines.push(i);
    }

    // For added lines, we'd need to show them somehow (ghost text or after-apply)
    // For now, just return the removal range
    const addedLines: number[] = [];

    return { removedLines, addedLines };
}
