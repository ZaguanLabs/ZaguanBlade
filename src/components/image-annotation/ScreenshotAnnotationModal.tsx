import React, { useCallback, useEffect, useId, useMemo, useReducer, useRef, useState } from 'react';
import { ArrowUpRight, Check, Circle, Eraser, MousePointer2, Pencil, Redo2, Square, Trash2, Type, Undo2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
    annotationBounds,
    getResizeHandleAtPoint,
    hitAnnotation,
    normalizeRect,
    paddedBounds,
    resizeAnnotation,
    resizeBoundsFromHandle,
    resizeHandleCursor,
    translateAnnotation,
    type Annotation,
    type Bounds,
    type Point,
    type ResizeHandle,
} from './annotationGeometry';
import {
    annotationEditorReducer,
    initialAnnotationEditorState,
} from './annotationState';

type Tool = 'select' | 'arrow' | 'text' | 'rect' | 'filled-rect' | 'ellipse' | 'filled-ellipse' | 'pen';

type TextDraft = {
    id?: string;
    point: Point;
    value: string;
    left: number;
    top: number;
    scale: number;
    color: string;
    fontSize: number;
};

type MoveDraft = {
    id: string;
    origin: Point;
    previous: Annotation[];
    moved: boolean;
};

type ResizeDraft = {
    id: string;
    handle: ResizeHandle;
    bounds: Bounds;
    previous: Annotation[];
    moved: boolean;
};

const COLORS = ['#ff4d4f', '#f97316', '#facc15', '#22c55e', '#38bdf8', '#a855f7', '#ffffff', '#111827'];

function loadImage(dataUrl: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image);
        image.onerror = () => reject(new Error('Failed to load screenshot.'));
        image.src = dataUrl;
    });
}

function drawArrow(ctx: CanvasRenderingContext2D, start: Point, end: Point, color: string, strokeWidth: number) {
    const angle = Math.atan2(end.y - start.y, end.x - start.x);
    const headLength = Math.max(12, strokeWidth * 4);
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = strokeWidth;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(start.x, start.y);
    ctx.lineTo(end.x, end.y);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(end.x, end.y);
    ctx.lineTo(end.x - headLength * Math.cos(angle - Math.PI / 6), end.y - headLength * Math.sin(angle - Math.PI / 6));
    ctx.lineTo(end.x - headLength * Math.cos(angle + Math.PI / 6), end.y - headLength * Math.sin(angle + Math.PI / 6));
    ctx.closePath();
    ctx.fill();
}

function drawAnnotation(ctx: CanvasRenderingContext2D, annotation: Annotation) {
    ctx.save();
    if (annotation.type === 'arrow') {
        drawArrow(ctx, annotation.start, annotation.end, annotation.color, annotation.strokeWidth);
    } else if (annotation.type === 'rect') {
        const rect = normalizeRect(annotation.start, annotation.end);
        ctx.lineWidth = annotation.strokeWidth;
        ctx.strokeStyle = annotation.color;
        ctx.fillStyle = annotation.color;
        if (annotation.filled) {
            ctx.globalAlpha = 0.35;
            ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
            ctx.globalAlpha = 1;
        }
        ctx.strokeRect(rect.x, rect.y, rect.width, rect.height);
    } else if (annotation.type === 'ellipse') {
        const rect = normalizeRect(annotation.start, annotation.end);
        ctx.lineWidth = annotation.strokeWidth;
        ctx.strokeStyle = annotation.color;
        ctx.fillStyle = annotation.color;
        ctx.beginPath();
        ctx.ellipse(rect.x + rect.width / 2, rect.y + rect.height / 2, rect.width / 2, rect.height / 2, 0, 0, Math.PI * 2);
        if (annotation.filled) {
            ctx.globalAlpha = 0.35;
            ctx.fill();
            ctx.globalAlpha = 1;
        }
        ctx.stroke();
    } else if (annotation.type === 'pen') {
        if (annotation.points.length > 1) {
            ctx.lineWidth = annotation.strokeWidth;
            ctx.strokeStyle = annotation.color;
            ctx.lineCap = 'round';
            ctx.lineJoin = 'round';
            ctx.beginPath();
            ctx.moveTo(annotation.points[0].x, annotation.points[0].y);
            for (const point of annotation.points.slice(1)) {
                ctx.lineTo(point.x, point.y);
            }
            ctx.stroke();
        }
    } else {
        ctx.fillStyle = annotation.color;
        ctx.font = `600 ${annotation.fontSize}px ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
        ctx.textBaseline = 'top';
        const lines = annotation.text.split('\n');
        for (let index = 0; index < lines.length; index += 1) {
            ctx.fillText(lines[index], annotation.point.x, annotation.point.y + index * annotation.fontSize * 1.25);
        }
    }
    ctx.restore();
}

function drawSelection(ctx: CanvasRenderingContext2D, annotation: Annotation) {
    const bounds = paddedBounds(annotationBounds(annotation), 6);
    ctx.save();
    ctx.strokeStyle = '#38bdf8';
    ctx.lineWidth = 2;
    ctx.setLineDash([8, 5]);
    ctx.strokeRect(bounds.x, bounds.y, bounds.width, bounds.height);
    ctx.setLineDash([]);
    ctx.fillStyle = '#38bdf8';
    const handles = [
        { x: bounds.x, y: bounds.y },
        { x: bounds.x + bounds.width, y: bounds.y },
        { x: bounds.x, y: bounds.y + bounds.height },
        { x: bounds.x + bounds.width, y: bounds.y + bounds.height },
    ];
    for (const handle of handles) {
        ctx.fillRect(handle.x - 4, handle.y - 4, 8, 8);
    }
    ctx.restore();
}

export const ScreenshotAnnotationModal: React.FC<{
    dataUrl: string;
    name: string;
    onCancel: () => void;
    onDone: (dataUrl: string, name: string) => void;
}> = ({ dataUrl, name, onCancel, onDone }) => {
    const { t } = useTranslation();
    const titleId = useId();
    const descriptionId = useId();
    const colorInputId = useId();
    const sizeInputId = useId();
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const canvasWrapRef = useRef<HTMLDivElement>(null);
    const imageRef = useRef<HTMLImageElement | null>(null);
    const draftRef = useRef<Annotation | null>(null);
    const textInputRef = useRef<HTMLTextAreaElement>(null);
    const [annotationState, dispatchAnnotations] = useReducer(annotationEditorReducer, initialAnnotationEditorState);
    const [tool, setTool] = useState<Tool>('arrow');
    const [color, setColor] = useState('#ff4d4f');
    const [strokeWidth, setStrokeWidth] = useState(5);
    const [draft, setDraft] = useState<Annotation | null>(null);
    const [drawing, setDrawing] = useState(false);
    const [moveDraft, setMoveDraft] = useState<MoveDraft | null>(null);
    const [resizeDraft, setResizeDraft] = useState<ResizeDraft | null>(null);
    const [textDraft, setTextDraft] = useState<TextDraft | null>(null);
    const [imageSize, setImageSize] = useState<{ width: number; height: number } | null>(null);
    const [imageLoadFailed, setImageLoadFailed] = useState(false);
    const { annotations, undoStack, redoStack, selectedAnnotationId } = annotationState;

    const tools = useMemo(() => [
        { id: 'select' as const, label: t('screenshot.editor.tools.select'), icon: MousePointer2 },
        { id: 'arrow' as const, label: t('screenshot.editor.tools.arrow'), icon: ArrowUpRight },
        { id: 'text' as const, label: t('screenshot.editor.tools.text'), icon: Type },
        { id: 'rect' as const, label: t('screenshot.editor.tools.box'), icon: Square },
        { id: 'filled-rect' as const, label: t('screenshot.editor.tools.filledBox'), icon: Square },
        { id: 'ellipse' as const, label: t('screenshot.editor.tools.circle'), icon: Circle },
        { id: 'filled-ellipse' as const, label: t('screenshot.editor.tools.filledCircle'), icon: Circle },
        { id: 'pen' as const, label: t('screenshot.editor.tools.pencil'), icon: Pencil },
    ], [t]);

    const imageReady = Boolean(imageSize && imageRef.current && !imageLoadFailed);

    const selectedAnnotation = useMemo(
        () => annotations.find((annotation) => annotation.id === selectedAnnotationId) ?? null,
        [annotations, selectedAnnotationId]
    );

    const selectedResizeHandle = useMemo(() => {
        if (!selectedAnnotation || !resizeDraft) {
            return null;
        }
        return resizeDraft.id === selectedAnnotation.id ? resizeDraft.handle : null;
    }, [resizeDraft, selectedAnnotation]);

    const effectiveSize = selectedAnnotation?.type === 'text'
        ? Math.max(2, Math.min(14, Math.round(selectedAnnotation.fontSize / 5)))
        : selectedAnnotation
            ? selectedAnnotation.strokeWidth
            : strokeWidth;

    const canvasCursor = tool === 'text'
        ? 'text'
        : tool === 'select'
            ? selectedResizeHandle
                ? resizeHandleCursor(selectedResizeHandle)
                : selectedAnnotation
                    ? 'move'
                    : 'default'
            : 'crosshair';

    const render = useCallback((nextDraft: Annotation | null, nextAnnotations: Annotation[], nextSelectedId: string | null) => {
        const canvas = canvasRef.current;
        const image = imageRef.current;
        const ctx = canvas?.getContext('2d');
        if (!canvas || !image || !ctx) {
            return;
        }
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
        for (const annotation of nextAnnotations) {
            drawAnnotation(ctx, annotation);
        }
        if (nextDraft) {
            drawAnnotation(ctx, nextDraft);
        }
        const selected = nextAnnotations.find((annotation) => annotation.id === nextSelectedId);
        if (selected) {
            drawSelection(ctx, selected);
        }
    }, []);

    const pushAnnotation = useCallback((annotation: Annotation) => {
        dispatchAnnotations({ type: 'push', annotation });
        setTool('select');
    }, []);

    const getCanvasPoint = useCallback((clientX: number, clientY: number): Point | null => {
        const canvas = canvasRef.current;
        if (!canvas) {
            return null;
        }
        const rect = canvas.getBoundingClientRect();
        return {
            x: Math.max(0, Math.min(canvas.width, (clientX - rect.left) * (canvas.width / rect.width))),
            y: Math.max(0, Math.min(canvas.height, (clientY - rect.top) * (canvas.height / rect.height))),
        };
    }, []);

    const getPoint = useCallback((event: React.PointerEvent<HTMLCanvasElement>): Point | null => getCanvasPoint(event.clientX, event.clientY), [getCanvasPoint]);

    const getTextDraftPosition = useCallback((point: Point): Pick<TextDraft, 'left' | 'top' | 'scale'> | null => {
        const canvas = canvasRef.current;
        const wrap = canvasWrapRef.current;
        if (!canvas || !wrap) {
            return null;
        }
        const canvasRect = canvas.getBoundingClientRect();
        const wrapRect = wrap.getBoundingClientRect();
        const scale = canvasRect.width / canvas.width;
        return {
            left: canvasRect.left - wrapRect.left + point.x * scale,
            top: canvasRect.top - wrapRect.top + point.y * scale,
            scale,
        };
    }, []);

    const findAnnotationAtPoint = useCallback((point: Point): Annotation | null => {
        for (let index = annotations.length - 1; index >= 0; index -= 1) {
            if (hitAnnotation(annotations[index], point)) {
                return annotations[index];
            }
        }
        return null;
    }, [annotations]);

    const beginTextEdit = useCallback((annotation: Extract<Annotation, { type: 'text' }>) => {
        const position = getTextDraftPosition(annotation.point);
        if (!position) {
            return;
        }
        dispatchAnnotations({ type: 'select', id: annotation.id });
        setTextDraft({
            id: annotation.id,
            point: annotation.point,
            value: annotation.text,
            color: annotation.color,
            fontSize: annotation.fontSize,
            ...position,
        });
        setTool('select');
    }, [getTextDraftPosition]);

    const commitTextDraft = useCallback(() => {
        setTextDraft((current) => {
            if (!current) {
                return null;
            }
            const value = current.value.trim();
            if (current.id) {
                const existing = annotations.find((annotation): annotation is Extract<Annotation, { type: 'text' }> => annotation.id === current.id && annotation.type === 'text');
                if (!existing) {
                    return null;
                }
                if (!value) {
                    dispatchAnnotations({
                        type: 'commit',
                        annotations: annotations.filter((annotation) => annotation.id !== current.id),
                        previousAnnotations: annotations,
                        selectedAnnotationId: null,
                    });
                    return null;
                }
                if (existing.text === value && existing.point.x === current.point.x && existing.point.y === current.point.y) {
                    dispatchAnnotations({ type: 'select', id: existing.id });
                    return null;
                }
                const updated: Annotation = {
                    ...existing,
                    point: current.point,
                    text: value,
                    color: current.color,
                    fontSize: current.fontSize,
                };
                dispatchAnnotations({
                    type: 'commit',
                    annotations: annotations.map((annotation) => annotation.id === current.id ? updated : annotation),
                    previousAnnotations: annotations,
                    selectedAnnotationId: updated.id,
                });
                return null;
            }
            if (!value) {
                return null;
            }
            const annotation: Annotation = {
                id: crypto.randomUUID(),
                type: 'text',
                point: current.point,
                text: value,
                color: current.color,
                fontSize: current.fontSize,
            };
            dispatchAnnotations({ type: 'push', annotation });
            setTool('select');
            return null;
        });
    }, [annotations]);

    const deleteSelected = useCallback(() => {
        dispatchAnnotations({ type: 'deleteSelected' });
    }, []);

    const nudgeSelected = useCallback((dx: number, dy: number) => {
        dispatchAnnotations({ type: 'nudgeSelected', dx, dy });
    }, []);

    const cycleSelection = useCallback((direction: 1 | -1) => {
        if (annotations.length === 0) {
            return;
        }
        dispatchAnnotations({ type: 'cycleSelection', direction });
        setTool('select');
    }, [annotations.length]);

    const applyColor = useCallback((nextColor: string) => {
        setColor(nextColor);
        dispatchAnnotations({ type: 'applyColor', color: nextColor });
    }, []);

    const applySize = useCallback((nextSize: number) => {
        setStrokeWidth(nextSize);
        dispatchAnnotations({ type: 'applySize', size: nextSize });
    }, []);

    const undo = useCallback(() => {
        dispatchAnnotations({ type: 'undo' });
        setDraft(null);
        draftRef.current = null;
        setTextDraft(null);
    }, []);

    const redo = useCallback(() => {
        dispatchAnnotations({ type: 'redo' });
        setDraft(null);
        draftRef.current = null;
        setTextDraft(null);
    }, []);

    const clear = useCallback(() => {
        if (annotations.length === 0) {
            return;
        }
        dispatchAnnotations({ type: 'clear' });
        setDraft(null);
        draftRef.current = null;
        setTextDraft(null);
    }, [annotations.length]);

    const finish = useCallback(() => {
        if (!imageReady) {
            return;
        }
        let nextAnnotations = annotations;
        if (textDraft) {
            const value = textDraft.value.trim();
            if (textDraft.id) {
                nextAnnotations = value
                    ? annotations.map((annotation) => annotation.id === textDraft.id && annotation.type === 'text'
                        ? { ...annotation, point: textDraft.point, text: value, color: textDraft.color, fontSize: textDraft.fontSize }
                        : annotation)
                    : annotations.filter((annotation) => annotation.id !== textDraft.id);
            } else if (value) {
                nextAnnotations = [
                    ...annotations,
                    {
                        id: crypto.randomUUID(),
                        type: 'text',
                        point: textDraft.point,
                        text: value,
                        color: textDraft.color,
                        fontSize: textDraft.fontSize,
                    },
                ];
            }
        }
        render(null, nextAnnotations, null);
        const canvas = canvasRef.current;
        if (!canvas) {
            return;
        }
        const output = canvas.toDataURL('image/png');
        const nextName = name.replace(/\.(png|jpe?g|webp|gif)$/i, '') + '-annotated.png';
        onDone(output, nextName);
    }, [annotations, imageReady, name, onDone, render, textDraft]);

    useEffect(() => {
        let cancelled = false;
        imageRef.current = null;
        draftRef.current = null;
        dispatchAnnotations({ type: 'reset' });
        setDraft(null);
        setDrawing(false);
        setMoveDraft(null);
        setResizeDraft(null);
        setTextDraft(null);
        setImageSize(null);
        setImageLoadFailed(false);
        loadImage(dataUrl).then((image) => {
            if (cancelled) {
                return;
            }
            imageRef.current = image;
            setImageSize({ width: image.naturalWidth || image.width, height: image.naturalHeight || image.height });
            const canvas = canvasRef.current;
            if (canvas) {
                canvas.width = image.naturalWidth || image.width;
                canvas.height = image.naturalHeight || image.height;
            }
            requestAnimationFrame(() => render(null, [], null));
        }).catch(() => {
            if (!cancelled) {
                setImageLoadFailed(true);
            }
        });
        return () => {
            cancelled = true;
        };
    }, [dataUrl, render]);

    useEffect(() => {
        render(draft, annotations, selectedAnnotationId);
    }, [annotations, draft, render, selectedAnnotationId]);

    useEffect(() => {
        if (textDraft) {
            requestAnimationFrame(() => textInputRef.current?.focus());
        }
    }, [textDraft]);

    useEffect(() => {
        const handleKeyDown = (event: KeyboardEvent) => {
            const target = event.target;
            const isTyping = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
            if (textDraft) {
                if (event.key === 'Escape') {
                    event.preventDefault();
                    setTextDraft(null);
                } else if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                    event.preventDefault();
                    commitTextDraft();
                }
                return;
            }
            if (isTyping) {
                return;
            }
            if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') {
                event.preventDefault();
                if (event.shiftKey) {
                    redo();
                } else {
                    undo();
                }
            } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'y') {
                event.preventDefault();
                redo();
            } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a' && annotations.length > 0) {
                event.preventDefault();
                dispatchAnnotations({ type: 'selectLast' });
                setTool('select');
            } else if (event.key === 'Tab' && annotations.length > 0) {
                event.preventDefault();
                cycleSelection(event.shiftKey ? -1 : 1);
            } else if (selectedAnnotationId && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) {
                event.preventDefault();
                const step = event.shiftKey ? 10 : 1;
                const dx = event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0;
                const dy = event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0;
                nudgeSelected(dx, dy);
            } else if ((event.key === 'Delete' || event.key === 'Backspace') && selectedAnnotationId) {
                event.preventDefault();
                deleteSelected();
            } else if (event.key === 'Enter' && selectedAnnotation?.type === 'text') {
                event.preventDefault();
                beginTextEdit(selectedAnnotation);
            } else if (event.key === 'Escape') {
                if (selectedAnnotationId) {
                    event.preventDefault();
                    dispatchAnnotations({ type: 'select', id: null });
                } else {
                    onCancel();
                }
            } else if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                event.preventDefault();
                void finish();
            } else if (!event.metaKey && !event.ctrlKey && !event.altKey) {
                const nextTool = {
                    v: 'select',
                    a: 'arrow',
                    t: 'text',
                    b: 'rect',
                    r: 'rect',
                    f: 'filled-rect',
                    c: 'ellipse',
                    o: 'ellipse',
                    p: 'pen',
                }[event.key.toLowerCase()] as Tool | undefined;
                if (nextTool) {
                    event.preventDefault();
                    setTool(nextTool);
                }
            }
        };
        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [annotations, beginTextEdit, commitTextDraft, cycleSelection, deleteSelected, finish, nudgeSelected, onCancel, redo, selectedAnnotation, selectedAnnotationId, textDraft, undo]);

    const handlePointerDown = useCallback((event: React.PointerEvent<HTMLCanvasElement>) => {
        if (!imageReady) {
            return;
        }
        const point = getPoint(event);
        if (!point) {
            return;
        }
        if (textDraft) {
            commitTextDraft();
            return;
        }
        if (tool === 'select') {
            const hit = findAnnotationAtPoint(point);
            dispatchAnnotations({ type: 'select', id: hit?.id ?? null });
            if (hit?.type === 'text' && event.detail >= 2) {
                beginTextEdit(hit);
                return;
            }
            if (hit) {
                const handle = getResizeHandleAtPoint(hit, point);
                event.currentTarget.setPointerCapture(event.pointerId);
                if (handle) {
                    setResizeDraft({ id: hit.id, handle, bounds: paddedBounds(annotationBounds(hit), 6), previous: annotations, moved: false });
                } else {
                    setMoveDraft({ id: hit.id, origin: point, previous: annotations, moved: false });
                }
            }
            return;
        }
        if (tool === 'text') {
            const position = getTextDraftPosition(point);
            if (position) {
                setTextDraft({ point, value: '', color, fontSize: Math.max(18, strokeWidth * 5), ...position });
            }
            dispatchAnnotations({ type: 'select', id: null });
            return;
        }
        event.currentTarget.setPointerCapture(event.pointerId);
        dispatchAnnotations({ type: 'select', id: null });
        setDrawing(true);
        const id = crypto.randomUUID();
        const nextDraft: Annotation = tool === 'arrow'
            ? { id, type: 'arrow', start: point, end: point, color, strokeWidth }
            : tool === 'pen'
                ? { id, type: 'pen', points: [point], color, strokeWidth }
                : tool === 'rect' || tool === 'filled-rect'
                    ? { id, type: 'rect', start: point, end: point, color, strokeWidth, filled: tool === 'filled-rect' }
                    : { id, type: 'ellipse', start: point, end: point, color, strokeWidth, filled: tool === 'filled-ellipse' };
        setDraft(nextDraft);
        draftRef.current = nextDraft;
        render(nextDraft, annotations, null);
    }, [annotations, beginTextEdit, color, commitTextDraft, findAnnotationAtPoint, getPoint, getTextDraftPosition, imageReady, render, strokeWidth, textDraft, tool]);

    const handlePointerMove = useCallback((event: React.PointerEvent<HTMLCanvasElement>) => {
        if (!imageReady) {
            return;
        }
        const point = getPoint(event);
        if (!point) {
            return;
        }
        if (resizeDraft) {
            const nextBounds = resizeBoundsFromHandle(resizeDraft.bounds, resizeDraft.handle, point);
            dispatchAnnotations({
                type: 'preview',
                annotations: resizeDraft.previous.map((annotation) => annotation.id === resizeDraft.id ? resizeAnnotation(annotation, resizeDraft.bounds, nextBounds) : annotation),
            });
            setResizeDraft((current) => current ? { ...current, moved: true } : current);
            return;
        }
        if (moveDraft) {
            const dx = point.x - moveDraft.origin.x;
            const dy = point.y - moveDraft.origin.y;
            dispatchAnnotations({
                type: 'preview',
                annotations: moveDraft.previous.map((annotation) => annotation.id === moveDraft.id ? translateAnnotation(annotation, dx, dy) : annotation),
            });
            setMoveDraft((current) => current ? { ...current, moved: true } : current);
            return;
        }
        const currentDraft = draftRef.current ?? draft;
        if (!drawing || !currentDraft) {
            return;
        }
        const nextDraft: Annotation = currentDraft.type === 'pen'
            ? { ...currentDraft, points: [...currentDraft.points, point] }
            : currentDraft.type === 'arrow'
                ? { ...currentDraft, end: point }
                : currentDraft.type === 'rect' || currentDraft.type === 'ellipse'
                    ? { ...currentDraft, end: point }
                    : currentDraft;
        draftRef.current = nextDraft;
        setDraft(nextDraft);
        render(nextDraft, annotations, null);
    }, [annotations, draft, drawing, getPoint, imageReady, moveDraft, render, resizeDraft]);

    const handlePointerUp = useCallback((event: React.PointerEvent<HTMLCanvasElement>) => {
        if (!imageReady) {
            return;
        }
        if (resizeDraft) {
            event.currentTarget.releasePointerCapture(event.pointerId);
            if (resizeDraft.moved) {
                dispatchAnnotations({ type: 'recordHistory', previousAnnotations: resizeDraft.previous });
            }
            setResizeDraft(null);
            return;
        }
        if (moveDraft) {
            event.currentTarget.releasePointerCapture(event.pointerId);
            if (moveDraft.moved) {
                dispatchAnnotations({ type: 'recordHistory', previousAnnotations: moveDraft.previous });
            }
            setMoveDraft(null);
            return;
        }
        const currentDraft = draftRef.current ?? draft;
        if (!drawing || !currentDraft) {
            return;
        }
        event.currentTarget.releasePointerCapture(event.pointerId);
        setDrawing(false);
        setDraft(null);
        let finalDraft = currentDraft;
        const point = event.type === 'pointercancel' ? null : getPoint(event);
        if (point) {
            finalDraft = currentDraft.type === 'pen'
                ? { ...currentDraft, points: [...currentDraft.points, point] }
                : currentDraft.type === 'arrow'
                    ? { ...currentDraft, end: point }
                    : currentDraft.type === 'rect' || currentDraft.type === 'ellipse'
                        ? { ...currentDraft, end: point }
                        : currentDraft;
        }
        draftRef.current = null;
        if (finalDraft.type === 'pen' && finalDraft.points.length < 2) {
            render(null, annotations, selectedAnnotationId);
            return;
        }
        if ((finalDraft.type === 'arrow' || finalDraft.type === 'rect' || finalDraft.type === 'ellipse') && Math.hypot(finalDraft.end.x - finalDraft.start.x, finalDraft.end.y - finalDraft.start.y) < 5) {
            render(null, annotations, selectedAnnotationId);
            return;
        }
        pushAnnotation(finalDraft);
    }, [annotations, draft, drawing, getPoint, imageReady, moveDraft, pushAnnotation, render, resizeDraft, selectedAnnotationId]);

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center bg-(--bg-app)/85 p-4">
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={descriptionId}
                className="flex max-h-[94vh] w-full max-w-7xl flex-col overflow-hidden rounded-(--panel-radius) border border-(--border-focus) bg-(--bg-surface) shadow-(--shadow-xl)"
            >
                <div className="flex shrink-0 items-center justify-between gap-3 border-b border-(--border-subtle) px-4 py-3">
                    <div className="min-w-0">
                        <div id={titleId} className="text-sm font-semibold text-(--fg-primary)">{t('screenshot.editor.title')}</div>
                        <div id={descriptionId} className="truncate text-xs text-(--fg-tertiary)">{name}</div>
                    </div>
                    <button type="button" onClick={onCancel} aria-label={t('common.close')} className="rounded-[calc(var(--panel-radius)*0.35)] p-1 text-(--fg-tertiary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)">
                        <X className="h-4 w-4" />
                    </button>
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-(--border-subtle) bg-(--bg-panel) px-4 py-2">
                    <div className="flex flex-wrap items-center gap-1">
                        {tools.map((entry) => {
                            const Icon = entry.icon;
                            const active = tool === entry.id;
                            return (
                                <button
                                    key={entry.id}
                                    type="button"
                                    title={entry.label}
                                    aria-pressed={active}
                                    disabled={!imageReady}
                                    onClick={() => {
                                        if (textDraft) {
                                            commitTextDraft();
                                        }
                                        setTool(entry.id);
                                    }}
                                    className={`inline-flex h-8 items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] border px-2 text-[11px] font-medium transition disabled:cursor-not-allowed disabled:opacity-40 ${active ? 'border-[color-mix(in_srgb,var(--accent-ai)_45%,transparent)] bg-[color-mix(in_srgb,var(--accent-ai)_14%,transparent)] text-(--fg-primary)' : 'border-(--border-subtle) bg-(--bg-app) text-(--fg-secondary) hover:text-(--fg-primary)'}`}
                                >
                                    <Icon className={`h-3.5 w-3.5 ${entry.id.startsWith('filled') ? 'fill-current' : ''}`} />
                                    {entry.label}
                                </button>
                            );
                        })}
                    </div>
                    <div className="h-6 w-px bg-(--border-subtle)" />
                    <div className="flex items-center gap-1">
                        {COLORS.map((entry) => (
                            <button
                                key={entry}
                                type="button"
                                aria-label={t('screenshot.editor.useColor', { color: entry })}
                                aria-pressed={color === entry}
                                disabled={!imageReady}
                                onClick={() => applyColor(entry)}
                                className={`h-6 w-6 rounded-[calc(var(--panel-radius)*0.35)] border transition ${color === entry ? 'border-(--fg-primary) ring-2 ring-(--accent-ai)/35' : 'border-(--border-subtle)'}`}
                                style={{ backgroundColor: entry }}
                            />
                        ))}
                        <input
                            id={colorInputId}
                            type="color"
                            aria-label={t('screenshot.editor.useColor', { color })}
                            value={color}
                            onChange={(event) => applyColor(event.target.value)}
                            className="h-7 w-8 rounded-[calc(var(--panel-radius)*0.35)] border border-(--border-subtle) bg-(--bg-app) p-0.5"
                        />
                    </div>
                    <label htmlFor={sizeInputId} className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                        {t('screenshot.editor.size')}
                        <input
                            id={sizeInputId}
                            type="range"
                            min={2}
                            max={14}
                            value={effectiveSize}
                            disabled={!imageReady}
                            onChange={(event) => applySize(Number(event.target.value))}
                            className="w-24 accent-(--accent-ai)"
                        />
                    </label>
                    <div className="ml-auto flex items-center gap-1">
                        <button type="button" onClick={undo} disabled={undoStack.length === 0 || !imageReady} className="inline-flex h-8 items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] px-2 text-[11px] text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:opacity-40">
                            <Undo2 className="h-3.5 w-3.5" />
                            {t('common.undo')}
                        </button>
                        <button type="button" onClick={redo} disabled={redoStack.length === 0 || !imageReady} className="inline-flex h-8 items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] px-2 text-[11px] text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:opacity-40">
                            <Redo2 className="h-3.5 w-3.5" />
                            {t('screenshot.editor.redo')}
                        </button>
                        <button type="button" onClick={deleteSelected} disabled={!selectedAnnotation || !imageReady} className="inline-flex h-8 items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] px-2 text-[11px] text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:opacity-40">
                            <Trash2 className="h-3.5 w-3.5" />
                            {t('common.delete')}
                        </button>
                        <button type="button" onClick={clear} disabled={annotations.length === 0 || !imageReady} className="inline-flex h-8 items-center gap-1 rounded-[calc(var(--panel-radius)*0.45)] px-2 text-[11px] text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:opacity-40">
                            <Eraser className="h-3.5 w-3.5" />
                            {t('common.clear')}
                        </button>
                    </div>
                </div>
                <div ref={canvasWrapRef} className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto bg-(--bg-app) p-4">
                    <canvas
                        ref={canvasRef}
                        onPointerDown={handlePointerDown}
                        onPointerMove={handlePointerMove}
                        onPointerUp={handlePointerUp}
                        onPointerCancel={handlePointerUp}
                        className="max-h-[calc(94vh-176px)] max-w-full touch-none select-none rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-panel) shadow-(--shadow-md)"
                        style={{ cursor: imageReady ? canvasCursor : 'wait' }}
                    />
                    {textDraft && (
                        <textarea
                            ref={textInputRef}
                            value={textDraft.value}
                            onChange={(event) => setTextDraft((current) => current ? { ...current, value: event.target.value } : current)}
                            onBlur={commitTextDraft}
                            onKeyDown={(event) => {
                                if (event.key === 'Escape') {
                                    event.preventDefault();
                                    setTextDraft(null);
                                } else if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                                    event.preventDefault();
                                    commitTextDraft();
                                }
                            }}
                            placeholder={t('screenshot.editor.textPlaceholder')}
                            aria-label={t('screenshot.editor.textPlaceholder')}
                            className="absolute min-h-10 w-64 resize-none rounded-[calc(var(--panel-radius)*0.35)] border border-[color-mix(in_srgb,var(--accent-ai)_44%,transparent)] bg-(--bg-surface)/95 px-2 py-1 font-semibold text-(--fg-primary) shadow-(--shadow-lg) outline-none"
                            style={{
                                left: textDraft.left,
                                top: textDraft.top,
                                color: textDraft.color,
                                fontSize: textDraft.fontSize * textDraft.scale,
                            }}
                        />
                    )}
                    {!imageReady && !imageLoadFailed && <div className="text-xs text-(--fg-tertiary)">{t('screenshot.editor.loading')}</div>}
                    {imageLoadFailed && <div className="text-xs text-(--state-danger)">{t('screenshot.editor.loadFailed')}</div>}
                </div>
                <div className="flex shrink-0 items-center justify-between gap-2 border-t border-(--border-subtle) px-4 py-3">
                    <div className="text-xs text-(--fg-tertiary)">{t('screenshot.editor.help')}</div>
                    <div className="flex items-center gap-2">
                        <button type="button" onClick={onCancel} className="rounded-[calc(var(--panel-radius)*0.45)] px-3 py-1.5 text-xs font-medium text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)">
                            {t('common.cancel')}
                        </button>
                        <button type="button" onClick={finish} disabled={!imageReady} className="inline-flex items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] bg-(--accent-ai) px-3 py-1.5 text-xs font-medium text-(--fg-bright) transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40">
                            <Check className="h-3.5 w-3.5" />
                            {t('common.finish')}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
