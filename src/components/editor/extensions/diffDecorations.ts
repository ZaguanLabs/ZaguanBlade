import { StateField, StateEffect, RangeSetBuilder } from "@codemirror/state";
import { EditorView, Decoration, DecorationSet } from "@codemirror/view";

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

// Parse unified diff to extract line information
export function parseUnifiedDiff(unifiedDiff: string): DiffLine[] {
    const lines: DiffLine[] = [];
    const diffLines = unifiedDiff.split('\n');
    
    let currentLineInNew = 0;
    let inHunk = false;
    
    for (const line of diffLines) {
        // Skip diff metadata headers
        if (line.startsWith('diff --git') || line.startsWith('index ') || 
            line.startsWith('---') || line.startsWith('+++')) {
            continue;
        }
        
        // Parse hunk header to get starting line
        if (line.startsWith('@@')) {
            const hunkMatch = line.match(/@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
            if (hunkMatch) {
                currentLineInNew = parseInt(hunkMatch[1], 10);
                inHunk = true;
            }
            continue;
        }
        
        // Only process lines if we're inside a hunk
        if (!inHunk) {
            continue;
        }
        
        // Handle different line types
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
            // Context line with space prefix
            lines.push({
                lineNumber: currentLineInNew,
                type: 'context',
                content: line.slice(1)
            });
            currentLineInNew++;
        } else if (line.length === 0) {
            // Empty line - treat as context
            lines.push({
                lineNumber: currentLineInNew,
                type: 'context',
                content: ''
            });
            currentLineInNew++;
        } else {
            // Any other line in a hunk without +/- prefix is treated as context
            // This handles cases where git diff doesn't add space prefix
            lines.push({
                lineNumber: currentLineInNew,
                type: 'context',
                content: line
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
