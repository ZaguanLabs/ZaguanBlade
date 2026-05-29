import {
    recolorAnnotation,
    resizeStrokeAnnotation,
    translateAnnotation,
    type Annotation,
} from './annotationGeometry';

export interface AnnotationEditorState {
    annotations: Annotation[];
    undoStack: Annotation[][];
    redoStack: Annotation[][];
    selectedAnnotationId: string | null;
}

export const initialAnnotationEditorState: AnnotationEditorState = {
    annotations: [],
    undoStack: [],
    redoStack: [],
    selectedAnnotationId: null,
};

export type AnnotationEditorAction =
    | { type: 'reset' }
    | { type: 'select'; id: string | null }
    | { type: 'selectLast' }
    | { type: 'cycleSelection'; direction: 1 | -1 }
    | { type: 'commit'; annotations: Annotation[]; previousAnnotations?: Annotation[]; selectedAnnotationId?: string | null }
    | { type: 'preview'; annotations: Annotation[] }
    | { type: 'recordHistory'; previousAnnotations: Annotation[] }
    | { type: 'push'; annotation: Annotation }
    | { type: 'deleteSelected' }
    | { type: 'nudgeSelected'; dx: number; dy: number }
    | { type: 'applyColor'; color: string }
    | { type: 'applySize'; size: number }
    | { type: 'clear' }
    | { type: 'undo' }
    | { type: 'redo' };

function commitAnnotations(
    state: AnnotationEditorState,
    annotations: Annotation[],
    previousAnnotations: Annotation[] = state.annotations,
    selectedAnnotationId: string | null = state.selectedAnnotationId
): AnnotationEditorState {
    return {
        annotations,
        undoStack: [...state.undoStack, previousAnnotations],
        redoStack: [],
        selectedAnnotationId,
    };
}

export function annotationEditorReducer(
    state: AnnotationEditorState,
    action: AnnotationEditorAction
): AnnotationEditorState {
    switch (action.type) {
        case 'reset':
            return initialAnnotationEditorState;
        case 'select':
            return { ...state, selectedAnnotationId: action.id };
        case 'selectLast': {
            const last = state.annotations[state.annotations.length - 1];
            return last ? { ...state, selectedAnnotationId: last.id } : state;
        }
        case 'cycleSelection': {
            if (state.annotations.length === 0) {
                return state;
            }
            const currentIndex = state.selectedAnnotationId
                ? state.annotations.findIndex((annotation) => annotation.id === state.selectedAnnotationId)
                : -1;
            const nextIndex = currentIndex === -1
                ? (action.direction === 1 ? 0 : state.annotations.length - 1)
                : (currentIndex + action.direction + state.annotations.length) % state.annotations.length;
            return { ...state, selectedAnnotationId: state.annotations[nextIndex].id };
        }
        case 'commit':
            return commitAnnotations(
                state,
                action.annotations,
                action.previousAnnotations ?? state.annotations,
                action.selectedAnnotationId === undefined ? state.selectedAnnotationId : action.selectedAnnotationId
            );
        case 'preview':
            return { ...state, annotations: action.annotations };
        case 'recordHistory':
            return {
                ...state,
                undoStack: [...state.undoStack, action.previousAnnotations],
                redoStack: [],
            };
        case 'push':
            return commitAnnotations(
                state,
                [...state.annotations, action.annotation],
                state.annotations,
                action.annotation.id
            );
        case 'deleteSelected': {
            if (!state.selectedAnnotationId) {
                return state;
            }
            const nextAnnotations = state.annotations.filter((annotation) => annotation.id !== state.selectedAnnotationId);
            if (nextAnnotations.length === state.annotations.length) {
                return state;
            }
            return commitAnnotations(state, nextAnnotations, state.annotations, null);
        }
        case 'nudgeSelected': {
            if (!state.selectedAnnotationId) {
                return state;
            }
            const selected = state.annotations.find((annotation) => annotation.id === state.selectedAnnotationId);
            if (!selected) {
                return state;
            }
            return commitAnnotations(
                state,
                state.annotations.map((annotation) => annotation.id === state.selectedAnnotationId
                    ? translateAnnotation(annotation, action.dx, action.dy)
                    : annotation),
                state.annotations,
                state.selectedAnnotationId
            );
        }
        case 'applyColor': {
            if (!state.selectedAnnotationId) {
                return state;
            }
            const selected = state.annotations.find((annotation) => annotation.id === state.selectedAnnotationId);
            if (!selected || selected.color === action.color) {
                return state;
            }
            return commitAnnotations(
                state,
                state.annotations.map((annotation) => annotation.id === state.selectedAnnotationId
                    ? recolorAnnotation(annotation, action.color)
                    : annotation),
                state.annotations,
                state.selectedAnnotationId
            );
        }
        case 'applySize': {
            if (!state.selectedAnnotationId) {
                return state;
            }
            const selected = state.annotations.find((annotation) => annotation.id === state.selectedAnnotationId);
            if (!selected) {
                return state;
            }
            const currentSize = selected.type === 'text'
                ? Math.round(selected.fontSize / 5)
                : selected.strokeWidth;
            if (currentSize === action.size) {
                return state;
            }
            return commitAnnotations(
                state,
                state.annotations.map((annotation) => annotation.id === state.selectedAnnotationId
                    ? resizeStrokeAnnotation(annotation, action.size)
                    : annotation),
                state.annotations,
                state.selectedAnnotationId
            );
        }
        case 'clear':
            if (state.annotations.length === 0) {
                return state;
            }
            return commitAnnotations(state, [], state.annotations, null);
        case 'undo': {
            const previous = state.undoStack[state.undoStack.length - 1];
            if (!previous) {
                return state;
            }
            return {
                annotations: previous,
                undoStack: state.undoStack.slice(0, -1),
                redoStack: [...state.redoStack, state.annotations],
                selectedAnnotationId: null,
            };
        }
        case 'redo': {
            const next = state.redoStack[state.redoStack.length - 1];
            if (!next) {
                return state;
            }
            return {
                annotations: next,
                undoStack: [...state.undoStack, state.annotations],
                redoStack: state.redoStack.slice(0, -1),
                selectedAnnotationId: null,
            };
        }
    }
}
