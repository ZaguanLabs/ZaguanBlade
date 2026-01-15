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
                    
                    // Count leading whitespace
                    let indent = 0;
                    let charPos = 0;
                    
                    while (charPos < text.length) {
                        const char = text[charPos];
                        if (char === " ") {
                            indent++;
                            charPos++;
                        } else if (char === "\t") {
                            indent += tabSize - (indent % tabSize);
                            charPos++;
                        } else {
                            break;
                        }
                    }

                    // Add guide marks at each indent level
                    if (indent >= tabSize && charPos > 0) {
                        let guideIndent = tabSize;
                        let guideCharPos = 0;
                        let currentIndent = 0;
                        
                        while (guideCharPos < charPos && currentIndent < indent) {
                            const char = text[guideCharPos];
                            if (char === " ") {
                                currentIndent++;
                            } else if (char === "\t") {
                                currentIndent += tabSize - (currentIndent % tabSize);
                            }
                            
                            if (currentIndent === guideIndent && guideIndent < indent) {
                                // Add a decoration at this position
                                builder.add(
                                    line.from + guideCharPos,
                                    line.from + guideCharPos + 1,
                                    indentGuide
                                );
                                guideIndent += tabSize;
                            }
                            guideCharPos++;
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
