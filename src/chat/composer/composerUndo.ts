// Undo/redo model for the chat composer's controlled <textarea>.
//
// Native browser undo is broken for React controlled inputs (facebook/react
// #17494, #5807, #28369), so the composer keeps its own history. Grouping is
// Monaco/VS Code style — explicit boundaries, NO timers (verified: VS Code's
// editStack.ts has zero time-based coalescing; word-at-most granularity comes
// from undo stops the typing layer pushes at whitespace-after-non-whitespace
// transitions and before Enter/paste). Companion rules follow Lexical (edit-
// kind switch breaks the group) and ProseMirror/CodeMirror 6 (non-adjacent
// edits break the group). See docs/internal/2026-07-04-composer-undo-research.md.

export interface UndoSnapshot {
    value: string;
    /** Selection in `value` at the moment the group opened. */
    start: number;
    end: number;
}

export type EditKind = 'insert' | 'delete-backward' | 'delete-forward' | 'other';

export interface ClassifiedChange {
    kind: EditKind;
    /** Offset where the change begins (common-prefix length). */
    changeStart: number;
    inserted: string;
    deletedLength: number;
}

const MAX_UNDO_DEPTH = 100;

/**
 * Diff prev → next into a single replace-span and classify it.
 * `deleteDirection` disambiguates Backspace vs Delete (both leave the cursor
 * at the change start); callers derive it from the last keydown.
 */
export function classifyChange(
    prev: string,
    next: string,
    deleteDirection: 'backward' | 'forward' | null,
): ClassifiedChange {
    let prefix = 0;
    const minLen = Math.min(prev.length, next.length);
    while (prefix < minLen && prev[prefix] === next[prefix]) {
        prefix += 1;
    }
    let suffix = 0;
    while (
        suffix < minLen - prefix &&
        prev[prev.length - 1 - suffix] === next[next.length - 1 - suffix]
    ) {
        suffix += 1;
    }
    const deletedLength = prev.length - prefix - suffix;
    const inserted = next.slice(prefix, next.length - suffix);

    let kind: EditKind;
    if (deletedLength === 0 && inserted.length > 0) {
        kind = 'insert';
    } else if (inserted.length === 0 && deletedLength > 0) {
        kind = deleteDirection === 'forward' ? 'delete-forward' : 'delete-backward';
    } else {
        // Replacement (typing over a selection), or a no-op.
        kind = 'other';
    }
    return { kind, changeStart: prefix, inserted, deletedLength };
}

function isWhitespaceOnly(s: string): boolean {
    return s.length > 0 && /^\s+$/.test(s);
}

export class ComposerUndoModel {
    private undoStack: UndoSnapshot[] = [];
    private redoStack: UndoSnapshot[] = [];
    // Open-group state. `groupKind === null` means no group is open, so the
    // next change always snapshots.
    private groupKind: EditKind | null = null;
    /** Position just after the group's most recent edit (adjacency anchor). */
    private groupLastPos = 0;
    /** Whether the group's last inserted character was whitespace. */
    private lastInsertWasWhitespace = false;

    /** Close the open group; the next change starts a new undo entry. */
    closeGroup(): void {
        this.groupKind = null;
    }

    private pushSnapshot(snapshot: UndoSnapshot): void {
        this.undoStack.push(snapshot);
        if (this.undoStack.length > MAX_UNDO_DEPTH) {
            this.undoStack.shift();
        }
    }

    /**
     * Decide whether `change` continues the open group (Monaco-style rules,
     * no timers). IME compositions never break the group.
     */
    private joins(change: ClassifiedChange, isComposing: boolean): boolean {
        if (this.groupKind === null) {
            return false;
        }
        if (isComposing) {
            // Never split an IME composition (ProseMirror/CodeMirror rule).
            return true;
        }
        // 'other' edits (paste, replace, programmatic) are isolated on both
        // sides: they never join, and closeGroup() after recording them means
        // nothing joins them.
        if (change.kind === 'other' || this.groupKind === 'other') {
            return false;
        }
        // Edit-kind switch breaks the group (Lexical change types).
        if (change.kind !== this.groupKind) {
            return false;
        }
        // Non-adjacent edit (cursor jump) breaks the group (PM/CM6 isAdjacent).
        if (Math.abs(change.changeStart - this.groupLastPos) > 1) {
            return false;
        }
        if (change.kind === 'insert') {
            // Enter always gets its own step (Monaco pushes a stop before it).
            if (change.inserted.includes('\n')) {
                return false;
            }
            // Word boundary: first whitespace after non-whitespace closes the
            // group ("one two" -> undo -> "one"); consecutive whitespace stays
            // grouped (Monaco TypingFirstSpace / TypingConsecutiveSpace).
            if (isWhitespaceOnly(change.inserted) && !this.lastInsertWasWhitespace) {
                return false;
            }
        }
        return true;
    }

    /**
     * Record a user edit made in the textarea. `preSelection` is the selection
     * in `prev` just before the edit (used as the undo entry's cursor).
     */
    recordChange(
        prev: string,
        next: string,
        preSelection: { start: number; end: number },
        deleteDirection: 'backward' | 'forward' | null,
        isComposing: boolean,
    ): void {
        if (prev === next) {
            return;
        }
        const change = classifyChange(prev, next, deleteDirection);
        if (change.kind === 'insert' && !isComposing && change.inserted.length > 1) {
            // A multi-character insert outside composition is never keyboard
            // typing — it's a paste, drop, or autocomplete. Isolate it like
            // Lexical does for clipboard ops (paste/cut -> OTHER, own entry).
            change.kind = 'other';
        }
        if (!this.joins(change, isComposing)) {
            this.pushSnapshot({
                value: prev,
                start: Math.min(preSelection.start, prev.length),
                end: Math.min(preSelection.end, prev.length),
            });
            this.groupKind = change.kind;
        }
        this.redoStack = [];
        // Advance the adjacency anchor to just after this edit.
        this.groupLastPos = change.kind === 'insert'
            ? change.changeStart + change.inserted.length
            : change.changeStart;
        if (change.kind === 'insert') {
            this.lastInsertWasWhitespace = isWhitespaceOnly(change.inserted);
        }
        if (change.kind === 'other' || (change.kind === 'insert' && !isComposing && change.inserted.includes('\n'))) {
            // Isolate on the trailing side too: 'other' edits (CM6
            // isolateHistory 'full') and Enter (its own undo step — text typed
            // after a newline must not merge into the newline's group).
            this.closeGroup();
        }
    }

    /**
     * Record a programmatic text change (prefill, mention insert, history
     * navigation, clear-on-send). Isolated on both sides so undo removes
     * exactly this change.
     */
    recordProgrammaticChange(prev: string, next: string, preSelection: { start: number; end: number }): void {
        if (prev === next) {
            return;
        }
        this.pushSnapshot({
            value: prev,
            start: Math.min(preSelection.start, prev.length),
            end: Math.min(preSelection.end, prev.length),
        });
        this.redoStack = [];
        this.closeGroup();
    }

    /**
     * Undo: returns the snapshot to restore, or null if the stack is empty.
     * `current` is captured onto the redo stack.
     */
    undo(current: UndoSnapshot): UndoSnapshot | null {
        const entry = this.undoStack.pop();
        if (entry === undefined) {
            return null;
        }
        this.redoStack.push(current);
        this.closeGroup();
        return entry;
    }

    /** Redo: inverse of undo. */
    redo(current: UndoSnapshot): UndoSnapshot | null {
        const entry = this.redoStack.pop();
        if (entry === undefined) {
            return null;
        }
        this.undoStack.push(current);
        this.closeGroup();
        return entry;
    }

    get undoDepth(): number {
        return this.undoStack.length;
    }

    get redoDepth(): number {
        return this.redoStack.length;
    }
}
