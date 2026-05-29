export type Point = { x: number; y: number };
export type Bounds = { x: number; y: number; width: number; height: number };
export type ResizeHandle = 'nw' | 'ne' | 'sw' | 'se';

export type Annotation =
    | { id: string; type: 'arrow'; start: Point; end: Point; color: string; strokeWidth: number }
    | { id: string; type: 'rect'; start: Point; end: Point; color: string; strokeWidth: number; filled: boolean }
    | { id: string; type: 'ellipse'; start: Point; end: Point; color: string; strokeWidth: number; filled: boolean }
    | { id: string; type: 'pen'; points: Point[]; color: string; strokeWidth: number }
    | { id: string; type: 'text'; point: Point; text: string; color: string; fontSize: number };

export function normalizeRect(start: Point, end: Point): Bounds {
    return {
        x: Math.min(start.x, end.x),
        y: Math.min(start.y, end.y),
        width: Math.abs(end.x - start.x),
        height: Math.abs(end.y - start.y),
    };
}

export function paddedBounds(bounds: Bounds, padding: number): Bounds {
    return {
        x: bounds.x - padding,
        y: bounds.y - padding,
        width: bounds.width + padding * 2,
        height: bounds.height + padding * 2,
    };
}

export function textBounds(annotation: Extract<Annotation, { type: 'text' }>): Bounds {
    const lines = annotation.text.split('\n');
    const longestLine = Math.max(1, ...lines.map((line) => line.length));
    return {
        x: annotation.point.x,
        y: annotation.point.y,
        width: Math.max(annotation.fontSize, longestLine * annotation.fontSize * 0.62),
        height: Math.max(annotation.fontSize, lines.length * annotation.fontSize * 1.25),
    };
}

export function annotationBounds(annotation: Annotation): Bounds {
    if (annotation.type === 'arrow') {
        return normalizeRect(annotation.start, annotation.end);
    }
    if (annotation.type === 'rect' || annotation.type === 'ellipse') {
        return normalizeRect(annotation.start, annotation.end);
    }
    if (annotation.type === 'text') {
        return textBounds(annotation);
    }
    const xs = annotation.points.map((point) => point.x);
    const ys = annotation.points.map((point) => point.y);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    return {
        x: minX,
        y: minY,
        width: Math.max(1, Math.max(...xs) - minX),
        height: Math.max(1, Math.max(...ys) - minY),
    };
}

export function pointInBounds(point: Point, bounds: Bounds): boolean {
    return point.x >= bounds.x
        && point.x <= bounds.x + bounds.width
        && point.y >= bounds.y
        && point.y <= bounds.y + bounds.height;
}

export function distanceToSegment(point: Point, start: Point, end: Point): number {
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    const lengthSquared = dx * dx + dy * dy;
    if (lengthSquared === 0) {
        return Math.hypot(point.x - start.x, point.y - start.y);
    }
    const t = Math.max(0, Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / lengthSquared));
    return Math.hypot(point.x - (start.x + t * dx), point.y - (start.y + t * dy));
}

export function hitAnnotation(annotation: Annotation, point: Point): boolean {
    const tolerance = Math.max(8, annotation.type === 'text' ? 8 : annotation.strokeWidth + 4);
    if (annotation.type === 'arrow') {
        return distanceToSegment(point, annotation.start, annotation.end) <= tolerance
            || pointInBounds(point, paddedBounds(annotationBounds(annotation), tolerance));
    }
    if (annotation.type === 'pen') {
        if (annotation.points.length < 2) {
            return false;
        }
        for (let index = 1; index < annotation.points.length; index += 1) {
            if (distanceToSegment(point, annotation.points[index - 1], annotation.points[index]) <= tolerance) {
                return true;
            }
        }
        return false;
    }
    return pointInBounds(point, paddedBounds(annotationBounds(annotation), tolerance));
}

export function translatePoint(point: Point, dx: number, dy: number): Point {
    return { x: point.x + dx, y: point.y + dy };
}

export function translateAnnotation(annotation: Annotation, dx: number, dy: number): Annotation {
    if (annotation.type === 'arrow') {
        return { ...annotation, start: translatePoint(annotation.start, dx, dy), end: translatePoint(annotation.end, dx, dy) };
    }
    if (annotation.type === 'rect' || annotation.type === 'ellipse') {
        return { ...annotation, start: translatePoint(annotation.start, dx, dy), end: translatePoint(annotation.end, dx, dy) };
    }
    if (annotation.type === 'pen') {
        return { ...annotation, points: annotation.points.map((point) => translatePoint(point, dx, dy)) };
    }
    return { ...annotation, point: translatePoint(annotation.point, dx, dy) };
}

export function resizePoint(point: Point, from: Bounds, to: Bounds): Point {
    const xRatio = from.width === 0 ? 0 : (point.x - from.x) / from.width;
    const yRatio = from.height === 0 ? 0 : (point.y - from.y) / from.height;
    return {
        x: to.x + xRatio * to.width,
        y: to.y + yRatio * to.height,
    };
}

export function resizeAnnotation(annotation: Annotation, from: Bounds, to: Bounds): Annotation {
    if (annotation.type === 'arrow') {
        return { ...annotation, start: resizePoint(annotation.start, from, to), end: resizePoint(annotation.end, from, to) };
    }
    if (annotation.type === 'rect' || annotation.type === 'ellipse') {
        return { ...annotation, start: resizePoint(annotation.start, from, to), end: resizePoint(annotation.end, from, to) };
    }
    if (annotation.type === 'pen') {
        return { ...annotation, points: annotation.points.map((point) => resizePoint(point, from, to)) };
    }
    const scale = Math.max(0.35, Math.min(to.width / Math.max(1, from.width), to.height / Math.max(1, from.height)));
    return {
        ...annotation,
        point: resizePoint(annotation.point, from, to),
        fontSize: Math.max(8, annotation.fontSize * scale),
    };
}

export function recolorAnnotation(annotation: Annotation, nextColor: string): Annotation {
    return { ...annotation, color: nextColor };
}

export function resizeStrokeAnnotation(annotation: Annotation, nextSize: number): Annotation {
    if (annotation.type === 'text') {
        return { ...annotation, fontSize: Math.max(8, nextSize * 5) };
    }
    return { ...annotation, strokeWidth: nextSize };
}

export function getResizeHandleAtPoint(annotation: Annotation, point: Point): ResizeHandle | null {
    const bounds = paddedBounds(annotationBounds(annotation), 6);
    const handles: Array<{ id: ResizeHandle; point: Point }> = [
        { id: 'nw', point: { x: bounds.x, y: bounds.y } },
        { id: 'ne', point: { x: bounds.x + bounds.width, y: bounds.y } },
        { id: 'sw', point: { x: bounds.x, y: bounds.y + bounds.height } },
        { id: 'se', point: { x: bounds.x + bounds.width, y: bounds.y + bounds.height } },
    ];
    for (const handle of handles) {
        if (Math.abs(point.x - handle.point.x) <= 8 && Math.abs(point.y - handle.point.y) <= 8) {
            return handle.id;
        }
    }
    return null;
}

export function resizeBoundsFromHandle(bounds: Bounds, handle: ResizeHandle, point: Point): Bounds {
    const opposite = {
        nw: { x: bounds.x + bounds.width, y: bounds.y + bounds.height },
        ne: { x: bounds.x, y: bounds.y + bounds.height },
        sw: { x: bounds.x + bounds.width, y: bounds.y },
        se: { x: bounds.x, y: bounds.y },
    }[handle];
    const next = normalizeRect(opposite, point);
    return {
        ...next,
        width: Math.max(4, next.width),
        height: Math.max(4, next.height),
    };
}

export function resizeHandleCursor(handle: ResizeHandle): string {
    return handle === 'nw' || handle === 'se' ? 'nwse-resize' : 'nesw-resize';
}
