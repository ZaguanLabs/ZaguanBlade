import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView } from "@codemirror/view";

// Effect to add line highlighting
export const addLineHighlight = StateEffect.define<{ startLine: number; endLine: number }>();

// Effect to clear line highlighting
export const clearLineHighlight = StateEffect.define();

// Line highlight decoration with a subtle background color
const lineHighlightMark = Decoration.line({
    attributes: { 
        class: "cm-highlighted-line",
        style: "background-color: rgba(255, 255, 0, 0.15); border-left: 3px solid rgba(255, 255, 0, 0.6);"
    }
});

// State field to manage line highlights
export const lineHighlightField = StateField.define<DecorationSet>({
    create() {
        return Decoration.none;
    },
    update(highlights, tr) {
        // Map existing highlights through document changes
        highlights = highlights.map(tr.changes);

        // Process effects
        for (const effect of tr.effects) {
            if (effect.is(addLineHighlight)) {
                const { startLine, endLine } = effect.value;
                const decorations = [];

                // Add decoration for each line in the range
                for (let lineNum = startLine; lineNum <= endLine; lineNum++) {
                    try {
                        const line = tr.state.doc.line(lineNum);
                        decorations.push(lineHighlightMark.range(line.from));
                    } catch {
                        // Line number out of range, skip
                        console.warn(`Line ${lineNum} out of range`);
                    }
                }

                highlights = Decoration.set(decorations);
            } else if (effect.is(clearLineHighlight)) {
                highlights = Decoration.none;
            }
        }

        return highlights;
    },
    provide: (field) => EditorView.decorations.from(field)
});
