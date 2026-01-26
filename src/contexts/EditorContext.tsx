import React, { createContext, useContext, useState, useCallback, useEffect, useRef } from 'react';
import { EditorFacade, initEditorFacade } from '../services/editorFacade';

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

    const setActiveFile = useCallback((file: string | null) => {
        setEditorState(prev => ({ ...prev, activeFile: file }));
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
