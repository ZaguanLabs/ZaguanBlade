import { EditorView, Decoration, DecorationSet, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { Extension, RangeSetBuilder } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";

// Rainbow bracket colors - vibrant but not overwhelming
const bracketColors = [
    "#fbbf24", // Amber 400
    "#a78bfa", // Violet 400
    "#34d399", // Emerald 400
    "#60a5fa", // Blue 400
    "#f472b6", // Pink 400
    "#fb923c", // Orange 400
];

// Create decorations for each color level
const bracketDecorations = bracketColors.map((color) =>
    Decoration.mark({
        class: `cm-rainbow-bracket`,
        attributes: { style: `color: ${color}` },
    })
);

// Bracket pairs to match
const openBrackets = new Set(["(", "[", "{"]);
const closeBrackets = new Set([")", "]", "}"]);
const bracketPairs: Record<string, string> = {
    "(": ")",
    "[": "]",
    "{": "}",
};

// Plugin to colorize brackets
const rainbowBracketsPlugin = ViewPlugin.fromClass(
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
            const doc = view.state.doc;
            const tree = syntaxTree(view.state);
            
            // Process each visible range independently with local depth tracking
            for (const { from, to } of view.visibleRanges) {
                const lineStart = doc.lineAt(from).from;
                const lineEnd = doc.lineAt(to).to;
                
                // Get text for this range only
                const text = doc.sliceString(lineStart, lineEnd);
                const brackets: { from: number; to: number; depth: number }[] = [];
                let depth = 0;
                
                // Simple bracket matching without full document scan
                for (let i = 0; i < text.length; i++) {
                    const char = text[i];
                    const pos = lineStart + i;
                    
                    // Quick syntax check - only resolve if it's a bracket
                    if (openBrackets.has(char) || closeBrackets.has(char)) {
                        const nodeAt = tree.resolveInner(pos, 1);
                        const nodeType = nodeAt.type.name;
                        
                        // Skip if inside string or comment (case-insensitive check)
                        const lowerType = nodeType.toLowerCase();
                        if (lowerType.includes("string") || lowerType.includes("comment")) {
                            continue;
                        }

                        if (openBrackets.has(char)) {
                            brackets.push({ from: pos, to: pos + 1, depth });
                            depth++;
                        } else if (closeBrackets.has(char)) {
                            depth = Math.max(0, depth - 1);
                            brackets.push({ from: pos, to: pos + 1, depth });
                        }
                    }
                }
                
                // Add decorations for this range
                for (const bracket of brackets) {
                    const colorIndex = bracket.depth % bracketColors.length;
                    builder.add(bracket.from, bracket.to, bracketDecorations[colorIndex]);
                }
            }

            return builder.finish();
        }
    },
    {
        decorations: (v) => v.decorations,
    }
);

// Theme for rainbow brackets
const rainbowBracketsTheme = EditorView.theme({
    ".cm-rainbow-bracket": {
        fontWeight: "500",
    },
});

// Combined extension
export const rainbowBrackets: Extension = [rainbowBracketsPlugin, rainbowBracketsTheme];
