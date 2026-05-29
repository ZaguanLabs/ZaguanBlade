import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { annotationEditorReducer, initialAnnotationEditorState, type AnnotationEditorAction } from './annotationState';
import type { Annotation } from './annotationGeometry';

const arrow: Annotation = {
    id: 'arrow',
    type: 'arrow',
    start: { x: 10, y: 10 },
    end: { x: 30, y: 30 },
    color: '#ff0000',
    strokeWidth: 4,
};

const text: Annotation = {
    id: 'text',
    type: 'text',
    point: { x: 40, y: 50 },
    text: 'Note',
    color: '#ffffff',
    fontSize: 20,
};

function reduce(actions: AnnotationEditorAction[]) {
    return actions.reduce(annotationEditorReducer, initialAnnotationEditorState);
}

describe('annotation editor reducer', () => {
    test('push selects the new annotation and supports undo/redo', () => {
        const pushed = reduce([{ type: 'push', annotation: arrow }]);

        assert.deepEqual(pushed.annotations, [arrow]);
        assert.equal(pushed.selectedAnnotationId, 'arrow');
        assert.equal(pushed.undoStack.length, 1);
        assert.equal(pushed.redoStack.length, 0);

        const undone = annotationEditorReducer(pushed, { type: 'undo' });
        assert.deepEqual(undone.annotations, []);
        assert.equal(undone.selectedAnnotationId, null);
        assert.deepEqual(undone.redoStack, [[arrow]]);

        const redone = annotationEditorReducer(undone, { type: 'redo' });
        assert.deepEqual(redone.annotations, [arrow]);
        assert.equal(redone.selectedAnnotationId, null);
    });

    test('delete selected annotation records history and clears selection', () => {
        const deleted = reduce([
            { type: 'push', annotation: arrow },
            { type: 'push', annotation: text },
            { type: 'select', id: 'arrow' },
            { type: 'deleteSelected' },
        ]);

        assert.deepEqual(deleted.annotations, [text]);
        assert.equal(deleted.selectedAnnotationId, null);
        assert.equal(deleted.undoStack.length, 3);

        const restored = annotationEditorReducer(deleted, { type: 'undo' });
        assert.deepEqual(restored.annotations, [arrow, text]);
    });

    test('nudge, color, and size edits are committed changes', () => {
        const edited = reduce([
            { type: 'push', annotation: arrow },
            { type: 'nudgeSelected', dx: 5, dy: -3 },
            { type: 'applyColor', color: '#00ff00' },
            { type: 'applySize', size: 8 },
        ]);
        const editedArrow = edited.annotations[0] as Extract<Annotation, { type: 'arrow' }>;

        assert.deepEqual(editedArrow.start, { x: 15, y: 7 });
        assert.deepEqual(editedArrow.end, { x: 35, y: 27 });
        assert.equal(editedArrow.color, '#00ff00');
        assert.equal(editedArrow.strokeWidth, 8);
        assert.equal(edited.undoStack.length, 4);
    });

    test('preview plus recordHistory models drag/resize without duplicate live history', () => {
        const pushed = reduce([{ type: 'push', annotation: arrow }]);
        const draggedArrow: Annotation = {
            ...arrow,
            start: { x: 20, y: 20 },
            end: { x: 40, y: 40 },
        };
        const previewed = annotationEditorReducer(pushed, { type: 'preview', annotations: [draggedArrow] });

        assert.deepEqual(previewed.annotations, [draggedArrow]);
        assert.equal(previewed.undoStack.length, 1);

        const committed = annotationEditorReducer(previewed, { type: 'recordHistory', previousAnnotations: pushed.annotations });
        assert.equal(committed.undoStack.length, 2);
        assert.deepEqual(annotationEditorReducer(committed, { type: 'undo' }).annotations, [arrow]);
    });

    test('cycles selection and selectLast follow annotation order', () => {
        const selected = reduce([
            { type: 'push', annotation: arrow },
            { type: 'push', annotation: text },
            { type: 'select', id: null },
            { type: 'cycleSelection', direction: 1 },
        ]);

        assert.equal(selected.selectedAnnotationId, 'arrow');
        assert.equal(annotationEditorReducer(selected, { type: 'cycleSelection', direction: 1 }).selectedAnnotationId, 'text');
        assert.equal(annotationEditorReducer(selected, { type: 'selectLast' }).selectedAnnotationId, 'text');
    });

    test('clear records history only when annotations exist', () => {
        const emptyClear = annotationEditorReducer(initialAnnotationEditorState, { type: 'clear' });
        assert.equal(emptyClear, initialAnnotationEditorState);

        const cleared = reduce([
            { type: 'push', annotation: arrow },
            { type: 'clear' },
        ]);

        assert.deepEqual(cleared.annotations, []);
        assert.equal(cleared.selectedAnnotationId, null);
        assert.equal(cleared.undoStack.length, 2);
    });
});
