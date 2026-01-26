import React, { createContext, useContext, useState, useCallback, useEffect, useRef } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { EditorFacade, initEditorFacade, isBackendAuthoritative } from '../services/editorFacade';
import type { BladeEventEnvelope, EditorEvent } from '../types/blade';

interface EditorState {
    activeFile: string | null;
    cursorLine: number | null;
    cursorColumn: number | null;
    selectionStartLine: number | null;
    selectionEndLine: number | null;
}

interface EditorContextType {
    editorState: EditorState;
    setActiveFile: (file: string | null) => void;
    setCursorPosition: (line: number, column: number) => void;
    setSelection: (startLine: number, endLine: number) => void;
    clearSelection: () => void;
}

const EditorContext = createContext<EditorContextType | undefined>(undefined);

export const EditorProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [editorState, setEditorState] = useState<EditorState>({
        activeFile: null,
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
        let unlisten: UnlistenFn | undefined;

        const setup = async () => {
            unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
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
                    setEditorState({
                        activeFile: editorEvent.payload.active_file ?? null,
                        cursorLine: editorEvent.payload.cursor_line ?? null,
                        cursorColumn: editorEvent.payload.cursor_column ?? null,
                        selectionStartLine: editorEvent.payload.selection_start ?? null,
                        selectionEndLine: editorEvent.payload.selection_end ?? null,
                    });
                }
            });
        };

        setup().catch(console.error);

        return () => {
            if (unlisten) unlisten();
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

    const clearSelection = useCallback(() => {
        setEditorState(prev => ({
            ...prev,
            selectionStartLine: null,
            selectionEndLine: null
        }));
    }, []);



    return (
        <EditorContext.Provider value={{
            editorState,
            setActiveFile,
            setCursorPosition,
            setSelection,
            clearSelection
        }}>
            {children}
        </EditorContext.Provider>
    );
};

export const useEditor = () => {
    const context = useContext(EditorContext);
    if (!context) {
        throw new Error('useEditor must be used within EditorProvider');
    }
    return context;
};
