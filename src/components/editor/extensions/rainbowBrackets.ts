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
            const visibleRanges = [...view.visibleRanges].sort(
                (a, b) => a.from - b.from || a.to - b.to
            );

            // Collect all brackets across visible ranges, then sort before adding.
            // Visible ranges expanded to line boundaries can overlap, so we deduplicate
            // by tracking the furthest position processed.
            const allBrackets: { from: number; to: number; depth: number }[] = [];
            let maxProcessed = -1;
            
            for (const { from, to } of visibleRanges) {
                const lineStart = Math.max(doc.lineAt(from).from, maxProcessed + 1);
                const lineEnd = doc.lineAt(to).to;
                if (lineStart > lineEnd) continue;
                maxProcessed = lineEnd;
                
                const text = doc.sliceString(lineStart, lineEnd);
                let depth = 0;
                
                for (let i = 0; i < text.length; i++) {
                    const char = text[i];
                    const pos = lineStart + i;
                    
                    if (openBrackets.has(char) || closeBrackets.has(char)) {
                        const nodeAt = tree.resolveInner(pos, 1);
                        const nodeType = nodeAt.type.name;
                        
                        const lowerType = nodeType.toLowerCase();
                        if (lowerType.includes("string") || lowerType.includes("comment")) {
                            continue;
                        }

                        if (openBrackets.has(char)) {
                            allBrackets.push({ from: pos, to: pos + 1, depth });
                            depth++;
                        } else if (closeBrackets.has(char)) {
                            depth = Math.max(0, depth - 1);
                            allBrackets.push({ from: pos, to: pos + 1, depth });
                        }
                    }
                }
            }

            // Sort by position (required by RangeSetBuilder)
            allBrackets.sort((a, b) => a.from - b.from || a.to - b.to);

            for (const bracket of allBrackets) {
                const colorIndex = bracket.depth % bracketColors.length;
                builder.add(bracket.from, bracket.to, bracketDecorations[colorIndex]);
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
