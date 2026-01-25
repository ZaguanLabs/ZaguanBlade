import { StateField, StateEffect, RangeSetBuilder } from "@codemirror/state";
import { EditorView, Decoration, DecorationSet, WidgetType } from "@codemirror/view";

// Diff line types
export interface DiffLine {
    lineNumber: number;  // 1-based line number in current content
    type: 'added' | 'removed' | 'context';
    content: string;     // The actual line content (without +/- prefix)
}

export interface DiffState {
    lines: DiffLine[];
    originalContent: string;
}

// State effects
export const setDiffState = StateEffect.define<DiffState | null>();
export const clearDiff = StateEffect.define<void>();

// Decorations for diff highlighting
const addedLineDecoration = Decoration.line({ class: "cm-diff-added" });
const removedLineDecoration = Decoration.line({ class: "cm-diff-removed" });

// Widget for showing removed lines inline
class RemovedLineWidget extends WidgetType {
    constructor(readonly content: string) {
        super();
    }

    toDOM() {
        const div = document.createElement("div");
        div.className = "cm-diff-removed-widget";
        div.textContent = this.content;
        return div;
    }

    eq(other: RemovedLineWidget) {
        return this.content === other.content;
    }
}

// Parse unified diff to extract line information
export function parseUnifiedDiff(unifiedDiff: string): DiffLine[] {
    const lines: DiffLine[] = [];
    const diffLines = unifiedDiff.split('\n');
    
    let currentLineInNew = 0;
    
    for (const line of diffLines) {
        // Skip diff headers
        if (line.startsWith('---') || line.startsWith('+++') || line.startsWith('@@')) {
            // Parse hunk header to get starting line
            const hunkMatch = line.match(/@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
            if (hunkMatch) {
                currentLineInNew = parseInt(hunkMatch[1], 10);
            }
            continue;
        }
        
        if (line.startsWith('+')) {
            lines.push({
                lineNumber: currentLineInNew,
                type: 'added',
                content: line.slice(1)
            });
            currentLineInNew++;
        } else if (line.startsWith('-')) {
            // Removed lines don't increment the current line counter
            lines.push({
                lineNumber: currentLineInNew, // Position where it was removed
                type: 'removed',
                content: line.slice(1)
            });
        } else if (line.startsWith(' ')) {
            // Context line
            lines.push({
                lineNumber: currentLineInNew,
                type: 'context',
                content: line.slice(1)
            });
            currentLineInNew++;
        }
    }
    
    return lines;
}

// StateField to track diff state
export const diffStateField = StateField.define<DiffState | null>({
    create() {
        return null;
    },
    
    update(value, tr) {
        for (const effect of tr.effects) {
            if (effect.is(setDiffState)) {
                return effect.value;
            }
            if (effect.is(clearDiff)) {
                return null;
            }
        }
        return value;
    }
});

// Decoration builder
function buildDecorations(view: EditorView): DecorationSet {
    const diffState = view.state.field(diffStateField);
    if (!diffState) {
        return Decoration.none;
    }
    
    const builder = new RangeSetBuilder<Decoration>();
    const doc = view.state.doc;
    
    for (const diffLine of diffState.lines) {
        if (diffLine.type === 'added' && diffLine.lineNumber <= doc.lines) {
            const line = doc.line(diffLine.lineNumber);
            builder.add(line.from, line.from, addedLineDecoration);
        }
        // Note: removed lines would need widget decorations which is more complex
    }
    
    return builder.finish();
}

// ViewPlugin to manage decorations
const diffDecorationsPlugin = EditorView.decorations.compute(
    [diffStateField],
    (state) => {
        const diffState = state.field(diffStateField);
        if (!diffState) {
            return Decoration.none;
        }
        
        const builder = new RangeSetBuilder<Decoration>();
        const doc = state.doc;
        
        for (const diffLine of diffState.lines) {
            if (diffLine.type === 'added' && diffLine.lineNumber <= doc.lines) {
                const line = doc.line(diffLine.lineNumber);
                builder.add(line.from, line.from, addedLineDecoration);
            }
        }
        
        return builder.finish();
    }
);

// Theme for diff decorations
export const diffTheme = EditorView.baseTheme({
    ".cm-diff-added": {
        backgroundColor: "rgba(46, 160, 67, 0.15)",
        borderLeft: "3px solid #2ea043",
    },
    ".cm-diff-removed": {
        backgroundColor: "rgba(248, 81, 73, 0.15)",
        borderLeft: "3px solid #f85149",
    },
    ".cm-diff-removed-widget": {
        backgroundColor: "rgba(248, 81, 73, 0.1)",
        color: "#f85149",
        fontFamily: "inherit",
        fontSize: "inherit",
        padding: "0 4px",
        textDecoration: "line-through",
        opacity: "0.7",
    }
});

// Export the extension
export function diffDecorations() {
    return [
        diffStateField,
        diffDecorationsPlugin,
        diffTheme
    ];
}
