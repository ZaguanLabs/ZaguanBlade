import { StateField, StateEffect, RangeSetBuilder } from "@codemirror/state";
import { EditorView, Decoration, WidgetType } from "@codemirror/view";
import { parsePatch } from "diff";

// ─── Types ───────────────────────────────────────────────────────────────────

export interface DiffLine {
    newLineNum: number | null;  // 1-based line number in new (current) content
    oldLineNum: number | null;  // 1-based line number in old (original) content
    type: 'added' | 'removed' | 'context' | 'gap';
    content: string;
    hiddenCount?: number;       // For gap lines: how many lines are hidden
}

export interface DiffState {
    lines: DiffLine[];
    originalContent: string;
}

// ─── State Effects ───────────────────────────────────────────────────────────

export const setDiffState = StateEffect.define<DiffState | null>();
export const clearDiff = StateEffect.define<void>();

// ─── Diff Parsing (using `diff` library) ─────────────────────────────────────

export function parseUnifiedDiff(source: string): DiffLine[] {
    if (!source) return [];

    try {
        const patches = parsePatch(source);
        if (!patches || patches.length === 0) return [];

        const patch = patches[0];
        if (!patch) return [];

        const lines: DiffLine[] = [];
        let prevHunk: { oldStart: number; oldLines: number; newStart: number; newLines: number } | null = null;

        for (const hunk of (patch as any).hunks || []) {
            // Insert a gap separator between hunks
            if (prevHunk) {
                const gapNew = hunk.newStart - (prevHunk.newStart + prevHunk.newLines);
                const gapOld = hunk.oldStart - (prevHunk.oldStart + prevHunk.oldLines);
                const hidden = Math.max(gapNew, gapOld);
                if (hidden > 0) {
                    lines.push({
                        oldLineNum: null,
                        newLineNum: null,
                        type: 'gap',
                        content: '',
                        hiddenCount: hidden,
                    });
                }
            }

            let oldLine = hunk.oldStart;
            let newLine = hunk.newStart;

            for (const raw of hunk.lines || []) {
                const firstChar = (raw as string)[0];
                const content = (raw as string).slice(1);

                if (firstChar === '-') {
                    lines.push({
                        oldLineNum: oldLine,
                        newLineNum: null,
                        type: 'removed',
                        content,
                    });
                    oldLine++;
                } else if (firstChar === '+') {
                    lines.push({
                        oldLineNum: null,
                        newLineNum: newLine,
                        type: 'added',
                        content,
                    });
                    newLine++;
                } else {
                    lines.push({
                        oldLineNum: oldLine,
                        newLineNum: newLine,
                        type: 'context',
                        content,
                    });
                    oldLine++;
                    newLine++;
                }
            }

            prevHunk = hunk;
        }

        return lines;
    } catch {
        return [];
    }
}

// ─── Character-Level Diff ────────────────────────────────────────────────────

interface CharDiff {
    offset: number;
    length: number;
}

// Simple word-boundary-aware character diff between two strings.
// Returns arrays of changed spans for old and new text.
function computeCharDiffs(oldStr: string, newStr: string): { oldSpans: CharDiff[]; newSpans: CharDiff[] } {
    const oldSpans: CharDiff[] = [];
    const newSpans: CharDiff[] = [];

    // Find common prefix
    let prefixLen = 0;
    const minLen = Math.min(oldStr.length, newStr.length);
    while (prefixLen < minLen && oldStr[prefixLen] === newStr[prefixLen]) {
        prefixLen++;
    }

    // Find common suffix (not overlapping with prefix)
    let suffixLen = 0;
    while (
        suffixLen < (minLen - prefixLen) &&
        oldStr[oldStr.length - 1 - suffixLen] === newStr[newStr.length - 1 - suffixLen]
    ) {
        suffixLen++;
    }

    const oldMiddleLen = oldStr.length - prefixLen - suffixLen;
    const newMiddleLen = newStr.length - prefixLen - suffixLen;

    if (oldMiddleLen > 0) {
        oldSpans.push({ offset: prefixLen, length: oldMiddleLen });
    }
    if (newMiddleLen > 0) {
        newSpans.push({ offset: prefixLen, length: newMiddleLen });
    }

    return { oldSpans, newSpans };
}

// ─── Removed Line Widget ─────────────────────────────────────────────────────

class RemovedLineWidget extends WidgetType {
    constructor(
        private readonly content: string,
        private readonly oldLineNum: number | null,
        private readonly charHighlights: CharDiff[],
    ) {
        super();
    }

    toDOM(): HTMLElement {
        const wrapper = document.createElement("div");
        wrapper.className = "cm-diff-removed-widget";

        // Line number gutter
        const gutter = document.createElement("span");
        gutter.className = "cm-diff-removed-gutter";
        gutter.textContent = this.oldLineNum != null ? String(this.oldLineNum) : "";
        wrapper.appendChild(gutter);

        // Minus sign
        const sign = document.createElement("span");
        sign.className = "cm-diff-removed-sign";
        sign.textContent = "−";
        wrapper.appendChild(sign);

        // Code content with optional character highlights
        const code = document.createElement("span");
        code.className = "cm-diff-removed-code";

        if (this.charHighlights.length > 0) {
            let lastEnd = 0;
            for (const span of this.charHighlights) {
                // Text before the highlight
                if (span.offset > lastEnd) {
                    code.appendChild(document.createTextNode(this.content.slice(lastEnd, span.offset)));
                }
                // Highlighted (changed) text
                const mark = document.createElement("span");
                mark.className = "cm-diff-char-removed";
                mark.textContent = this.content.slice(span.offset, span.offset + span.length);
                code.appendChild(mark);
                lastEnd = span.offset + span.length;
            }
            // Remaining text after last highlight
            if (lastEnd < this.content.length) {
                code.appendChild(document.createTextNode(this.content.slice(lastEnd)));
            }
        } else {
            code.textContent = this.content;
        }

        wrapper.appendChild(code);
        return wrapper;
    }

    eq(other: RemovedLineWidget): boolean {
        return this.content === other.content
            && this.oldLineNum === other.oldLineNum
            && JSON.stringify(this.charHighlights) === JSON.stringify(other.charHighlights);
    }

    get estimatedHeight(): number {
        return 20;
    }

    ignoreEvent(): boolean {
        return true;
    }
}

// ─── Gap (Collapsed Unchanged Region) Widget ─────────────────────────────────

class GapWidget extends WidgetType {
    constructor(private readonly hiddenCount: number) {
        super();
    }

    toDOM(): HTMLElement {
        const wrapper = document.createElement("div");
        wrapper.className = "cm-diff-gap-widget";

        const line = document.createElement("span");
        line.className = "cm-diff-gap-line";
        wrapper.appendChild(line);

        const label = document.createElement("span");
        label.className = "cm-diff-gap-label";
        label.textContent = `${this.hiddenCount} unchanged line${this.hiddenCount === 1 ? '' : 's'}`;
        wrapper.appendChild(label);

        const line2 = document.createElement("span");
        line2.className = "cm-diff-gap-line";
        wrapper.appendChild(line2);

        return wrapper;
    }

    eq(other: GapWidget): boolean {
        return this.hiddenCount === other.hiddenCount;
    }

    get estimatedHeight(): number {
        return 24;
    }

    ignoreEvent(): boolean {
        return true;
    }
}

// ─── State Field ─────────────────────────────────────────────────────────────

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

// ─── Line Decorations (added lines) ─────────────────────────────────────────

const addedLineDecoration = Decoration.line({ class: "cm-diff-added-line" });

// ─── Decoration Provider ─────────────────────────────────────────────────────

// Pair up consecutive removed+added lines for character-level diffing
function pairRemovedAdded(diffLines: DiffLine[]): Map<number, { removed: DiffLine; added: DiffLine }> {
    const pairs = new Map<number, { removed: DiffLine; added: DiffLine }>();
    for (let i = 0; i < diffLines.length - 1; i++) {
        if (diffLines[i].type === 'removed' && diffLines[i + 1].type === 'added') {
            pairs.set(i, { removed: diffLines[i], added: diffLines[i + 1] });
        }
    }
    return pairs;
}

const diffDecorationsPlugin = EditorView.decorations.compute(
    [diffStateField],
    (state) => {
        const diffState = state.field(diffStateField);
        if (!diffState) {
            return Decoration.none;
        }

        const builder = new RangeSetBuilder<Decoration>();
        const doc = state.doc;
        const diffLines = diffState.lines;

        // Pre-compute removed↔added pairs for character-level diffs
        const pairs = pairRemovedAdded(diffLines);
        const pairedAddedIndices = new Set<number>();
        for (const [idx] of pairs) {
            pairedAddedIndices.add(idx + 1);
        }

        // We need to collect all decorations with their positions, then sort by position
        // because CM6 requires decorations to be added in document order.
        const decos: { from: number; to: number; deco: Decoration }[] = [];

        for (let i = 0; i < diffLines.length; i++) {
            const dl = diffLines[i];

            if (dl.type === 'added' && dl.newLineNum != null && dl.newLineNum <= doc.lines) {
                const line = doc.line(dl.newLineNum);

                // Line-level decoration (green background)
                decos.push({ from: line.from, to: line.from, deco: addedLineDecoration });

                // Character-level highlights for paired lines
                if (pairedAddedIndices.has(i)) {
                    const pair = pairs.get(i - 1)!;
                    const { newSpans } = computeCharDiffs(pair.removed.content, pair.added.content);
                    for (const span of newSpans) {
                        const from = line.from + span.offset;
                        const to = Math.min(line.from + span.offset + span.length, line.to);
                        if (from < to && from >= line.from && to <= line.to) {
                            decos.push({
                                from,
                                to,
                                deco: Decoration.mark({ class: "cm-diff-char-added" }),
                            });
                        }
                    }
                }
            } else if (dl.type === 'removed') {
                // Find the position in the document where this removed line should appear.
                // It should appear before the next new-side line number.
                let insertBeforeLine = dl.newLineNum;

                // If newLineNum is null (removed-only line), find the next context/added line's newLineNum
                if (insertBeforeLine == null) {
                    for (let j = i + 1; j < diffLines.length; j++) {
                        if (diffLines[j].newLineNum != null) {
                            insertBeforeLine = diffLines[j].newLineNum;
                            break;
                        }
                    }
                }

                // Fallback: place at end of document
                if (insertBeforeLine == null) {
                    insertBeforeLine = doc.lines + 1;
                }

                const pos = insertBeforeLine <= doc.lines
                    ? doc.line(insertBeforeLine).from
                    : doc.length;

                // Compute character highlights if this removed line is paired with an added line
                let charHighlights: CharDiff[] = [];
                if (pairs.has(i)) {
                    const pair = pairs.get(i)!;
                    const { oldSpans } = computeCharDiffs(pair.removed.content, pair.added.content);
                    charHighlights = oldSpans;
                }

                decos.push({
                    from: pos,
                    to: pos,
                    deco: Decoration.widget({
                        widget: new RemovedLineWidget(dl.content, dl.oldLineNum, charHighlights),
                        block: true,
                        side: -1, // Place before the line
                    }),
                });
            } else if (dl.type === 'gap') {
                // Find the position for the gap widget
                let gapBeforeLine: number | null = null;
                for (let j = i + 1; j < diffLines.length; j++) {
                    if (diffLines[j].newLineNum != null) {
                        gapBeforeLine = diffLines[j].newLineNum;
                        break;
                    }
                }

                const pos = gapBeforeLine != null && gapBeforeLine <= doc.lines
                    ? doc.line(gapBeforeLine).from
                    : doc.length;

                decos.push({
                    from: pos,
                    to: pos,
                    deco: Decoration.widget({
                        widget: new GapWidget(dl.hiddenCount || 0),
                        block: true,
                        side: -1,
                    }),
                });
            }
        }

        // Sort by position (required by CM6)
        decos.sort((a, b) => a.from - b.from || a.to - b.to);

        for (const d of decos) {
            builder.add(d.from, d.to, d.deco);
        }

        return builder.finish();
    }
);

// ─── Theme ───────────────────────────────────────────────────────────────────

export const diffTheme = EditorView.baseTheme({
    // Added line (full line background)
    ".cm-diff-added-line": {
        backgroundColor: "rgba(46, 160, 67, 0.15)",
        borderLeft: "3px solid rgba(46, 160, 67, 0.7)",
    },

    // Character-level highlight within added lines
    ".cm-diff-char-added": {
        backgroundColor: "rgba(46, 160, 67, 0.35)",
        borderRadius: "2px",
    },

    // Removed line widget (block widget inserted above)
    ".cm-diff-removed-widget": {
        display: "flex",
        alignItems: "center",
        backgroundColor: "rgba(248, 81, 73, 0.10)",
        borderLeft: "3px solid rgba(248, 81, 73, 0.7)",
        fontFamily: "inherit",
        fontSize: "inherit",
        lineHeight: "inherit",
        minHeight: "1.4em",
        padding: "0",
        color: "rgba(200, 200, 200, 0.6)",
        userSelect: "none",
    },

    // Gutter area in removed line widget
    ".cm-diff-removed-gutter": {
        display: "inline-block",
        width: "3.5em",
        textAlign: "right",
        paddingRight: "8px",
        color: "rgba(248, 81, 73, 0.4)",
        fontSize: "0.85em",
        flexShrink: "0",
        userSelect: "none",
    },

    // Minus sign in removed line widget
    ".cm-diff-removed-sign": {
        display: "inline-block",
        width: "1.5em",
        textAlign: "center",
        color: "rgba(248, 81, 73, 0.6)",
        flexShrink: "0",
        userSelect: "none",
    },

    // Code content in removed line widget
    ".cm-diff-removed-code": {
        flex: "1",
        textDecoration: "line-through",
        textDecorationColor: "rgba(248, 81, 73, 0.4)",
        whiteSpace: "pre",
        overflow: "hidden",
        paddingRight: "8px",
    },

    // Character-level highlight within removed lines
    ".cm-diff-char-removed": {
        backgroundColor: "rgba(248, 81, 73, 0.30)",
        borderRadius: "2px",
        textDecoration: "line-through",
        textDecorationColor: "rgba(248, 81, 73, 0.6)",
    },

    // Gap widget (collapsed unchanged region)
    ".cm-diff-gap-widget": {
        display: "flex",
        alignItems: "center",
        gap: "8px",
        padding: "2px 12px",
        backgroundColor: "rgba(128, 128, 128, 0.05)",
        borderTop: "1px solid rgba(128, 128, 128, 0.15)",
        borderBottom: "1px solid rgba(128, 128, 128, 0.15)",
        userSelect: "none",
        minHeight: "24px",
    },

    ".cm-diff-gap-line": {
        flex: "1",
        height: "1px",
        backgroundColor: "rgba(128, 128, 128, 0.2)",
    },

    ".cm-diff-gap-label": {
        fontSize: "0.8em",
        color: "rgba(128, 128, 128, 0.6)",
        whiteSpace: "nowrap",
        fontStyle: "italic",
    },
});

// ─── Extension Export ────────────────────────────────────────────────────────

export function diffDecorations() {
    return [
        diffStateField,
        diffDecorationsPlugin,
        diffTheme
    ];
}
