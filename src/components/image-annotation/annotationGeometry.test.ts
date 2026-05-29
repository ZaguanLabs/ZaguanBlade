import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import {
    annotationBounds,
    getResizeHandleAtPoint,
    hitAnnotation,
    normalizeRect,
    resizeAnnotation,
    resizeBoundsFromHandle,
    textBounds,
    translateAnnotation,
    type Annotation,
} from './annotationGeometry';

describe('annotation geometry', () => {
    test('normalizes rectangles regardless of drag direction', () => {
        assert.deepEqual(normalizeRect({ x: 40, y: 80 }, { x: 10, y: 20 }), {
            x: 10,
            y: 20,
            width: 30,
            height: 60,
        });
    });

    test('computes text bounds from multiline content', () => {
        const text: Extract<Annotation, { type: 'text' }> = {
            id: 'text',
            type: 'text',
            point: { x: 12, y: 24 },
            text: 'short\nlonger line',
            color: '#fff',
            fontSize: 20,
        };

        assert.deepEqual(textBounds(text), {
            x: 12,
            y: 24,
            width: 11 * 20 * 0.62,
            height: 2 * 20 * 1.25,
        });
    });

    test('hit tests arrows and pen strokes by segment distance', () => {
        const arrow: Annotation = {
            id: 'arrow',
            type: 'arrow',
            start: { x: 10, y: 10 },
            end: { x: 110, y: 10 },
            color: '#f00',
            strokeWidth: 3,
        };
        const pen: Annotation = {
            id: 'pen',
            type: 'pen',
            points: [{ x: 10, y: 50 }, { x: 110, y: 50 }],
            color: '#f00',
            strokeWidth: 4,
        };

        assert.equal(hitAnnotation(arrow, { x: 60, y: 15 }), true);
        assert.equal(hitAnnotation(arrow, { x: 60, y: 40 }), false);
        assert.equal(hitAnnotation(pen, { x: 60, y: 56 }), true);
        assert.equal(hitAnnotation({ ...pen, points: [{ x: 10, y: 50 }] }, { x: 10, y: 50 }), false);
    });

    test('translates annotations without mutating the original', () => {
        const pen: Annotation = {
            id: 'pen',
            type: 'pen',
            points: [{ x: 1, y: 2 }, { x: 3, y: 4 }],
            color: '#fff',
            strokeWidth: 2,
        };

        const translated = translateAnnotation(pen, 10, -1);
        assert.deepEqual(annotationBounds(translated), { x: 11, y: 1, width: 2, height: 2 });
        assert.deepEqual(pen.points, [{ x: 1, y: 2 }, { x: 3, y: 4 }]);
    });

    test('resizes shapes and text relative to bounds', () => {
        const arrow: Annotation = {
            id: 'arrow',
            type: 'arrow',
            start: { x: 10, y: 10 },
            end: { x: 20, y: 20 },
            color: '#fff',
            strokeWidth: 2,
        };
        const text: Annotation = {
            id: 'text',
            type: 'text',
            point: { x: 10, y: 10 },
            text: 'Note',
            color: '#fff',
            fontSize: 20,
        };
        const from = { x: 10, y: 10, width: 10, height: 10 };
        const to = { x: 20, y: 30, width: 30, height: 20 };

        assert.deepEqual(resizeAnnotation(arrow, from, to), {
            ...arrow,
            start: { x: 20, y: 30 },
            end: { x: 50, y: 50 },
        });
        assert.deepEqual(resizeAnnotation(text, from, to), {
            ...text,
            point: { x: 20, y: 30 },
            fontSize: 40,
        });
    });

    test('finds resize handles and clamps tiny resize bounds', () => {
        const rect: Annotation = {
            id: 'rect',
            type: 'rect',
            start: { x: 10, y: 20 },
            end: { x: 50, y: 60 },
            color: '#fff',
            strokeWidth: 2,
            filled: false,
        };

        assert.equal(getResizeHandleAtPoint(rect, { x: 5, y: 15 }), 'nw');
        assert.equal(getResizeHandleAtPoint(rect, { x: 100, y: 100 }), null);
        assert.deepEqual(resizeBoundsFromHandle({ x: 10, y: 10, width: 20, height: 20 }, 'nw', { x: 29, y: 29 }), {
            x: 29,
            y: 29,
            width: 4,
            height: 4,
        });
    });
});
