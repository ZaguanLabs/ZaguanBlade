import React, { createContext, useContext, useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { EditorFacade, initEditorFacade, isBackendAuthoritative } from '../services/editorFacade';
import type { BladeEventEnvelope, EditorEvent } from '../types/blade';

interface EditorState {
    activeFile: string | null;
    openFiles: string[];
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
}

interface EditorContextType extends EditorActionsType {
    editorState: EditorState;
}

const EditorStateContext = createContext<EditorState | undefined>(undefined);
const EditorActionsContext = createContext<EditorActionsType | undefined>(undefined);

export const EditorProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [editorState, setEditorState] = useState<EditorState>({
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
        initEditorFacade().catch(console.error);
    }, []);

    // Listen for backend EditorEvent updates when backend authority is enabled
    useEffect(() => {
        const unlistenPromise = listen<BladeEventEnvelope>('blade-event', (event) => {
                const bladeEvent = event.payload.event;
                if (bladeEvent.type !== 'Editor') return;

                const editorEvent = bladeEvent.payload as EditorEvent;

                if (editorEvent.type === 'ActiveFileChanged') {
                    setEditorState(prev => ({
                        ...prev,
                        activeFile: editorEvent.payload.path ?? null
                    }));
                } else if (editorEvent.type === 'CursorMoved') {
                    setEditorState(prev => ({
                        ...prev,
                        cursorLine: editorEvent.payload.line,
                        cursorColumn: editorEvent.payload.column
                    }));
                } else if (editorEvent.type === 'SelectionChanged') {
                    setEditorState(prev => ({
                        ...prev,
                        selectionStartLine: editorEvent.payload.start,
                        selectionEndLine: editorEvent.payload.end
                    }));
                } else if (editorEvent.type === 'StateSnapshot') {
                    setEditorState(prev => ({
                        activeFile: editorEvent.payload.active_file ?? null,
                        openFiles: editorEvent.payload.open_files ?? prev.openFiles,
                        cursorLine: editorEvent.payload.cursor_line ?? null,
                        cursorColumn: editorEvent.payload.cursor_column ?? null,
                        selectionStartLine: editorEvent.payload.selection_start ?? null,
                        selectionEndLine: editorEvent.payload.selection_end ?? null,
                    }));
                }
            });

        return () => {
            unlistenPromise
                .then(unlisten => unlisten())
                .catch(console.error);
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
        // Always update local state for immediate UI feedback
        setEditorState(prev => ({ ...prev, activeFile: file }));

        // If backend authority is enabled, also notify backend
        if (isBackendAuthoritative()) {
            EditorFacade.setActiveFile(file).catch(console.error);
        }
    }, []);

    const setCursorPosition = useCallback((line: number, column: number) => {
        setEditorState(prev => ({
            ...prev,
            cursorLine: line,
            cursorColumn: column
        }));

        // Debounced sync to backend (100ms) - always syncs for AI context
        if (cursorDebounceRef.current) {
            clearTimeout(cursorDebounceRef.current);
        }
        cursorDebounceRef.current = setTimeout(() => {
            EditorFacade.updateCursor(line, column);
        }, 100);
    }, []);

    const setSelection = useCallback((startLine: number, endLine: number) => {
        setEditorState(prev => ({
            ...prev,
            selectionStartLine: startLine,
            selectionEndLine: endLine
        }));

        // Debounced sync to backend (100ms) - always syncs for AI context
        if (selectionDebounceRef.current) {
            clearTimeout(selectionDebounceRef.current);
        }
        selectionDebounceRef.current = setTimeout(() => {
            EditorFacade.updateSelection(startLine, endLine);
        }, 100);
    }, []);

    const setOpenFiles = useCallback((files: string[]) => {
        setEditorState(prev => ({ ...prev, openFiles: files }));
    }, []);

    const clearSelection = useCallback(() => {
        setEditorState(prev => ({
            ...prev,
            selectionStartLine: null,
            selectionEndLine: null
        }));
    }, []);



    const actions = useMemo<EditorActionsType>(() => ({
        setActiveFile,
        setOpenFiles,
        setCursorPosition,
        setSelection,
        clearSelection,
    }), [setActiveFile, setOpenFiles, setCursorPosition, setSelection, clearSelection]);

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
