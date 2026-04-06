import React, { createContext, useContext, useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { useOptionalStartupBootstrap } from './StartupBootstrapContext';
import { EditorFacade, initEditorFacade, isBackendAuthoritative } from '../services/editorFacade';
import { subscribeBladeNestedEventType } from '../services/bladeEvents';

function areStringArraysEqual(a: string[], b: string[]): boolean {
    if (a === b) {
        return true;
    }
    if (a.length !== b.length) {
        return false;
    }
    for (let index = 0; index < a.length; index += 1) {
        if (a[index] !== b[index]) {
            return false;
        }
    }
    return true;
}

interface EditorState {
    activeFile: string | null;
    openFiles: string[];
}

interface EditorSnapshot extends EditorState {
    cursorLine: number | null;
    cursorColumn: number | null;
    selectionStartLine: number | null;
    selectionEndLine: number | null;
}

interface EditorActionsType {
    setActiveFile: (file: string | null) => void;
    setOpenFiles: (files: string[]) => void;
    setCursorPosition: (line: number, column: number) => void;
    setSelection: (startLine: number, endLine: number) => void;
    clearSelection: () => void;
    getEditorSnapshot: () => EditorSnapshot;
}

interface EditorContextType extends EditorActionsType {
    editorState: EditorState;
}

const EditorStateContext = createContext<EditorState | undefined>(undefined);
const EditorActionsContext = createContext<EditorActionsType | undefined>(undefined);

export const EditorProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const bootstrapContext = useOptionalStartupBootstrap();
    const [editorState, setEditorState] = useState<EditorState>({
        activeFile: null,
        openFiles: [],
    });
    const snapshotRef = useRef<EditorSnapshot>({
        activeFile: null,
        openFiles: [],
        cursorLine: null,
        cursorColumn: null,
        selectionStartLine: null,
        selectionEndLine: null,
    });

    // Debounce refs for cursor/selection sync
    const cursorDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const selectionDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Initialize EditorFacade on mount
    useEffect(() => {
        initEditorFacade(bootstrapContext?.bootstrap?.feature_flags ?? null).catch(console.error);
    }, [bootstrapContext?.bootstrap?.feature_flags]);

    // Listen for backend EditorEvent updates when backend authority is enabled
    useEffect(() => {
        const unsubscribeActiveFileChanged = subscribeBladeNestedEventType('Editor', 'ActiveFileChanged', (payload) => {
            snapshotRef.current = {
                ...snapshotRef.current,
                activeFile: payload.path ?? null,
            };
            setEditorState(prev => ({
                ...prev,
                activeFile: payload.path ?? null
            }));
        });
        const unsubscribeCursorMoved = subscribeBladeNestedEventType('Editor', 'CursorMoved', (payload) => {
            snapshotRef.current = {
                ...snapshotRef.current,
                cursorLine: payload.line,
                cursorColumn: payload.column
            };
        });
        const unsubscribeSelectionChanged = subscribeBladeNestedEventType('Editor', 'SelectionChanged', (payload) => {
            snapshotRef.current = {
                ...snapshotRef.current,
                selectionStartLine: payload.start,
                selectionEndLine: payload.end
            };
        });
        const unsubscribeStateSnapshot = subscribeBladeNestedEventType('Editor', 'StateSnapshot', (payload) => {
            const nextOpenFiles = payload.open_files ?? snapshotRef.current.openFiles;
            snapshotRef.current = {
                activeFile: payload.active_file ?? null,
                openFiles: nextOpenFiles,
                cursorLine: payload.cursor_line ?? null,
                cursorColumn: payload.cursor_column ?? null,
                selectionStartLine: payload.selection_start ?? null,
                selectionEndLine: payload.selection_end ?? null,
            };
            setEditorState(prev => ({
                activeFile: payload.active_file ?? null,
                openFiles: payload.open_files ?? prev.openFiles,
            }));
        });

        return () => {
            unsubscribeActiveFileChanged();
            unsubscribeCursorMoved();
            unsubscribeSelectionChanged();
            unsubscribeStateSnapshot();
        };
    }, []);

    // Clear pending debounced sync calls on unmount
    useEffect(() => {
        return () => {
            if (cursorDebounceRef.current) {
                clearTimeout(cursorDebounceRef.current);
                cursorDebounceRef.current = null;
            }
            if (selectionDebounceRef.current) {
                clearTimeout(selectionDebounceRef.current);
                selectionDebounceRef.current = null;
            }
        };
    }, []);

    const setActiveFile = useCallback((file: string | null) => {
        if (snapshotRef.current.activeFile === file) {
            return;
        }
        snapshotRef.current = {
            ...snapshotRef.current,
            activeFile: file,
        };
        // Always update local state for immediate UI feedback
        setEditorState(prev => ({ ...prev, activeFile: file }));

        // If backend authority is enabled, also notify backend
        if (isBackendAuthoritative()) {
            EditorFacade.setActiveFile(file).catch(console.error);
        }
    }, []);

    const setCursorPosition = useCallback((line: number, column: number) => {
        if (snapshotRef.current.cursorLine === line && snapshotRef.current.cursorColumn === column) {
            return;
        }
        snapshotRef.current = {
            ...snapshotRef.current,
            cursorLine: line,
            cursorColumn: column
        };

        // Debounced sync to backend (100ms) - always syncs for AI context
        if (cursorDebounceRef.current) {
            clearTimeout(cursorDebounceRef.current);
        }
        cursorDebounceRef.current = setTimeout(() => {
            EditorFacade.updateCursor(line, column);
        }, 100);
    }, []);

    const setSelection = useCallback((startLine: number, endLine: number) => {
        if (
            snapshotRef.current.selectionStartLine === startLine
            && snapshotRef.current.selectionEndLine === endLine
        ) {
            return;
        }
        snapshotRef.current = {
            ...snapshotRef.current,
            selectionStartLine: startLine,
            selectionEndLine: endLine
        };

        // Debounced sync to backend (100ms) - always syncs for AI context
        if (selectionDebounceRef.current) {
            clearTimeout(selectionDebounceRef.current);
        }
        selectionDebounceRef.current = setTimeout(() => {
            EditorFacade.updateSelection(startLine, endLine);
        }, 100);
    }, []);

    const setOpenFiles = useCallback((files: string[]) => {
        if (areStringArraysEqual(snapshotRef.current.openFiles, files)) {
            return;
        }
        snapshotRef.current = {
            ...snapshotRef.current,
            openFiles: files,
        };
        setEditorState(prev => ({ ...prev, openFiles: files }));
    }, []);

    const clearSelection = useCallback(() => {
        if (
            snapshotRef.current.selectionStartLine === null
            && snapshotRef.current.selectionEndLine === null
        ) {
            return;
        }
        snapshotRef.current = {
            ...snapshotRef.current,
            selectionStartLine: null,
            selectionEndLine: null
        };
    }, []);

    const getEditorSnapshot = useCallback(() => snapshotRef.current, []);

    const actions = useMemo<EditorActionsType>(() => ({
        setActiveFile,
        setOpenFiles,
        setCursorPosition,
        setSelection,
        clearSelection,
        getEditorSnapshot,
    }), [setActiveFile, setOpenFiles, setCursorPosition, setSelection, clearSelection, getEditorSnapshot]);

    return (
        <EditorActionsContext.Provider value={actions}>
            <EditorStateContext.Provider value={editorState}>
                {children}
            </EditorStateContext.Provider>
        </EditorActionsContext.Provider>
    );
};

export const useEditorState = () => {
    const context = useContext(EditorStateContext);
    if (!context) {
        throw new Error('useEditorState must be used within EditorProvider');
    }
    return context;
};

export const useEditorActions = () => {
    const context = useContext(EditorActionsContext);
    if (!context) {
        throw new Error('useEditorActions must be used within EditorProvider');
    }
    return context;
};

export const useEditor = () => {
    const editorState = useEditorState();
    const actions = useEditorActions();
    return {
        editorState,
        ...actions,
    } as EditorContextType;
};
