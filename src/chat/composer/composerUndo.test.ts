import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { classifyChange, ComposerUndoModel, type UndoSnapshot } from './composerUndo';

/** Simulates typing text one character at a time into the model. */
function typeText(model: ComposerUndoModel, start: string, typed: string): string {
    let value = start;
    for (const ch of typed) {
        const next = value + ch;
        model.recordChange(value, next, { start: value.length, end: value.length }, null, false);
        value = next;
    }
    return value;
}

function backspace(model: ComposerUndoModel, value: string, count: number): string {
    let current = value;
    for (let i = 0; i < count; i += 1) {
        const next = current.slice(0, -1);
        model.recordChange(current, next, { start: current.length, end: current.length }, 'backward', false);
        current = next;
    }
    return current;
}

function snap(value: string): UndoSnapshot {
    return { value, start: value.length, end: value.length };
}

describe('classifyChange', () => {
    test('classifies single-character insert', () => {
        assert.deepEqual(classifyChange('abc', 'abcd', null), {
            kind: 'insert',
            changeStart: 3,
            inserted: 'd',
            deletedLength: 0,
        });
    });

    test('classifies mid-text insert', () => {
        const change = classifyChange('ac', 'abc', null);
        assert.equal(change.kind, 'insert');
        assert.equal(change.inserted, 'b');
        assert.equal(change.changeStart, 1);
    });

    test('classifies backward delete', () => {
        const change = classifyChange('abc', 'ab', 'backward');
        assert.equal(change.kind, 'delete-backward');
        assert.equal(change.deletedLength, 1);
    });

    test('classifies forward delete', () => {
        const change = classifyChange('abc', 'bc', 'forward');
        assert.equal(change.kind, 'delete-forward');
        assert.equal(change.changeStart, 0);
    });

    test('classifies replacement as other', () => {
        assert.equal(classifyChange('hello world', 'hello there', null).kind, 'other');
    });
});

describe('word-at-most grouping (Monaco rules)', () => {
    test('undo after "one two" reverts to "one", then to empty', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'one two');

        const first = model.undo(snap(value));
        assert.equal(first?.value, 'one');

        const second = model.undo(snap(first!.value));
        assert.equal(second?.value, '');

        assert.equal(model.undo(snap('')), null);
    });

    test('a fast-typed whole line still undoes word by word (no timers)', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'the quick brown fox');
        const entries: string[] = [];
        let current = value;
        for (;;) {
            const entry = model.undo(snap(current));
            if (!entry) break;
            entries.push(entry.value);
            current = entry.value;
        }
        assert.deepEqual(entries, ['the quick brown', 'the quick', 'the', '']);
    });

    test('consecutive spaces stay in one group', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'a   b');
        // Groups: "a" | "   b" — the first space opens a new group, consecutive
        // spaces join it, and "b" continues that same insert group.
        const first = model.undo(snap(value));
        assert.equal(first?.value, 'a');
    });

    test('Enter gets its own undo step', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'hi');
        const next = `${value}\n`;
        model.recordChange(value, next, { start: value.length, end: value.length }, null, false);
        value = typeText(model, next, 'there');

        const u1 = model.undo(snap(value));
        assert.equal(u1?.value, 'hi\n');
        const u2 = model.undo(snap(u1!.value));
        assert.equal(u2?.value, 'hi');
    });
});

describe('edit-kind and adjacency boundaries', () => {
    test('switching from insert to backspace starts a new group', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'abcd');
        value = backspace(model, value, 2);
        assert.equal(value, 'ab');

        // Undo the deletion burst -> back to "abcd".
        const entry = model.undo(snap(value));
        assert.equal(entry?.value, 'abcd');
    });

    test('consecutive backspaces coalesce into one entry', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'word');
        value = backspace(model, value, 4);
        assert.equal(value, '');
        const entry = model.undo(snap(value));
        assert.equal(entry?.value, 'word');
        // And that entry was a single step: the next undo is the typing group.
        const prev = model.undo(snap(entry!.value));
        assert.equal(prev?.value, '');
    });

    test('cursor jump (non-adjacent edit) starts a new group', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'foobar');
        // Jump to position 0 and insert there: "Xfoobar".
        model.recordChange(value, `X${value}`, { start: 0, end: 0 }, null, false);
        const entry = model.undo(snap(`X${value}`));
        assert.equal(entry?.value, 'foobar');
    });
});

describe('isolation of paste/programmatic edits', () => {
    test('replacement (paste over selection) is its own undo step', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'draft ');
        // Paste "PASTED" at the end (multi-char insert joins as insert... but a
        // replacement of selected text classifies as other):
        model.recordChange(`${value}old`, `${value}PASTED`, { start: value.length, end: value.length + 3 }, null, false);
        const entry = model.undo(snap(`${value}PASTED`));
        assert.equal(entry?.value, `${value}old`);
    });

    test('paste at cursor (multi-char insert) is its own undo step', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'cmd: ');
        // Paste "ls -la" at the cursor — no selection replaced, pure insert.
        const pasted = `${value}ls -la`;
        model.recordChange(value, pasted, { start: value.length, end: value.length }, null, false);
        // Keep typing after the paste — must NOT join the paste's entry.
        value = typeText(model, pasted, 'x');

        const u1 = model.undo(snap(value));
        assert.equal(u1?.value, pasted); // removes "x"
        const u2 = model.undo(snap(u1!.value));
        assert.equal(u2?.value, 'cmd: '); // removes exactly the paste
    });

    test('programmatic change (mention insert) is isolated on both sides', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'see ');
        const withMention = `${value}@src/main.rs `;
        model.recordProgrammaticChange(value, withMention, { start: value.length, end: value.length });
        value = typeText(model, withMention, 'ok');

        const u1 = model.undo(snap(value));
        assert.equal(u1?.value, withMention); // removes "ok"
        const u2 = model.undo(snap(u1!.value));
        assert.equal(u2?.value, 'see '); // removes exactly the mention
    });
});

describe('redo and stack behavior', () => {
    test('undo then redo round-trips', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'one two');
        const entry = model.undo(snap(value));
        assert.equal(entry?.value, 'one');
        const redone = model.redo(snap(entry!.value));
        assert.equal(redone?.value, 'one two');
    });

    test('a new change clears the redo stack', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'one two');
        const entry = model.undo(snap(value));
        value = typeText(model, entry!.value, '!');
        assert.equal(model.redoDepth, 0);
        assert.equal(model.redo(snap(value)), null);
    });

    test('IME composition never splits mid-composition', () => {
        const model = new ComposerUndoModel();
        let value = typeText(model, '', 'x');
        // Composition updates including whitespace-like segments join freely.
        model.recordChange(value, `${value} `, { start: value.length, end: value.length }, null, true);
        value = `${value} `;
        model.recordChange(value, `${value}あ`, { start: value.length, end: value.length }, null, true);
        value = `${value}あ`;
        const entry = model.undo(snap(value));
        assert.equal(entry?.value, '');
    });

    test('undo restores the pre-group cursor position', () => {
        const model = new ComposerUndoModel();
        const value = typeText(model, '', 'one two');
        const entry = model.undo(snap(value));
        // Group opened right before the space was typed, cursor was at 3.
        assert.equal(entry?.start, 3);
        assert.equal(entry?.end, 3);
    });

    test('depth is capped at 100 entries (FIFO eviction)', () => {
        const model = new ComposerUndoModel();
        let value = '';
        // 150 isolated programmatic changes -> 150 would-be entries.
        for (let i = 0; i < 150; i += 1) {
            const next = `v${i}`;
            model.recordProgrammaticChange(value, next, { start: 0, end: 0 });
            value = next;
        }
        assert.equal(model.undoDepth, 100);
    });
});
