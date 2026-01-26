import { EditorView, Decoration, DecorationSet, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { Extension, RangeSetBuilder } from "@codemirror/state";

// Indent guide decoration
const indentGuide = Decoration.mark({
    class: "cm-indent-guide",
});

// Plugin to render indent guides
const indentGuidePlugin = ViewPlugin.fromClass(
    class {
        decorations: DecorationSet;

        constructor(view: EditorView) {
            this.decorations = this.buildDecorations(view);
        }

        update(update: ViewUpdate) {
            if (update.docChanged || update.viewportChanged) {
                this.decorations = this.buildDecorations(update.view);
            }
        }

        buildDecorations(view: EditorView): DecorationSet {
            const builder = new RangeSetBuilder<Decoration>();
            const tabSize = view.state.tabSize;

            for (const { from, to } of view.visibleRanges) {
                for (let pos = from; pos < to; ) {
                    const line = view.state.doc.lineAt(pos);
                    const text = line.text;
                    
                    // Quick check: skip empty lines
                    if (text.length === 0) {
                        pos = line.to + 1;
                        continue;
                    }
                    
                    // Count leading whitespace using regex for speed
                    const match = text.match(/^[\t ]*/);
                    if (!match || match[0].length === 0) {
                        pos = line.to + 1;
                        continue;
                    }
                    
                    const whitespace = match[0];
                    let indent = 0;
                    
                    // Calculate total indent
                    for (const char of whitespace) {
                        if (char === " ") {
                            indent++;
                        } else if (char === "\t") {
                            indent += tabSize - (indent % tabSize);
                        }
                    }

                    // Add guide marks at each indent level
                    if (indent >= tabSize) {
                        for (let level = tabSize; level < indent; level += tabSize) {
                            // Find character position for this indent level
                            let charIndent = 0;
                            for (let i = 0; i < whitespace.length; i++) {
                                const char = whitespace[i];
                                if (char === " ") {
                                    charIndent++;
                                } else if (char === "\t") {
                                    charIndent += tabSize - (charIndent % tabSize);
                                }
                                
                                if (charIndent === level) {
                                    builder.add(
                                        line.from + i,
                                        line.from + i + 1,
                                        indentGuide
                                    );
                                    break;
                                }
                            }
                        }
                    }

                    pos = line.to + 1;
                }
            }

            return builder.finish();
        }
    },
    {
        decorations: (v) => v.decorations,
    }
);

// Theme for indent guides
const indentGuideTheme = EditorView.theme({
    ".cm-indent-guide": {
        borderLeft: "1px solid rgba(255, 255, 255, 0.06)",
        marginLeft: "-1px",
    },
});

// Combined extension
export const indentGuides: Extension = [indentGuidePlugin, indentGuideTheme];
