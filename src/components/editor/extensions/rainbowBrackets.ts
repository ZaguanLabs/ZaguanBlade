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
            
            // Track bracket depth
            const stack: { char: string; pos: number; depth: number }[] = [];
            const brackets: { from: number; to: number; depth: number }[] = [];

            // Iterate through visible ranges
            for (const { from, to } of view.visibleRanges) {
                // We need to start from the beginning to get correct depth
                // But only decorate visible range
                const text = doc.sliceString(0, to);
                let depth = 0;
                
                for (let i = 0; i < text.length; i++) {
                    const char = text[i];
                    
                    // Check if we're inside a string or comment using syntax tree
                    const nodeAt = tree.resolveInner(i, 1);
                    const nodeType = nodeAt.type.name.toLowerCase();
                    
                    // Skip if inside string or comment
                    if (
                        nodeType.includes("string") ||
                        nodeType.includes("comment") ||
                        nodeType.includes("template")
                    ) {
                        continue;
                    }

                    if (openBrackets.has(char)) {
                        stack.push({ char, pos: i, depth });
                        if (i >= from) {
                            brackets.push({ from: i, to: i + 1, depth });
                        }
                        depth++;
                    } else if (closeBrackets.has(char)) {
                        depth = Math.max(0, depth - 1);
                        
                        // Find matching open bracket
                        for (let j = stack.length - 1; j >= 0; j--) {
                            if (bracketPairs[stack[j].char] === char) {
                                if (i >= from) {
                                    brackets.push({ from: i, to: i + 1, depth: stack[j].depth });
                                }
                                stack.splice(j, 1);
                                break;
                            }
                        }
                    }
                }
            }

            // Sort by position and add decorations
            brackets.sort((a, b) => a.from - b.from);
            for (const bracket of brackets) {
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
