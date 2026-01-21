
import React, { createContext, useContext, useState, useCallback } from 'react';

interface EditorState {
    activeFile: string | null;
    cursorLine: number | null;
    cursorColumn: number | null;
    selectionStartLine: number | null;
    selectionEndLine: number | null;
    enableLsp: boolean;
}

interface EditorContextType {
    editorState: EditorState;
    setActiveFile: (file: string | null) => void;
    setCursorPosition: (line: number, column: number) => void;
    setSelection: (startLine: number, endLine: number) => void;
    clearSelection: () => void;
    setEnableLsp: (enabled: boolean) => void;
}

const EditorContext = createContext<EditorContextType | undefined>(undefined);

export const EditorProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [editorState, setEditorState] = useState<EditorState>({
        activeFile: null,
        cursorLine: null,
        cursorColumn: null,
        selectionStartLine: null,
        selectionEndLine: null,
        enableLsp: true, // Default to true
    });

    const setActiveFile = useCallback((file: string | null) => {
        setEditorState(prev => ({ ...prev, activeFile: file }));
    }, []);

    const setCursorPosition = useCallback((line: number, column: number) => {
        setEditorState(prev => ({
            ...prev,
            cursorLine: line,
            cursorColumn: column
        }));
    }, []);

    const setSelection = useCallback((startLine: number, endLine: number) => {
        setEditorState(prev => ({
            ...prev,
            selectionStartLine: startLine,
            selectionEndLine: endLine
        }));
    }, []);

    const clearSelection = useCallback(() => {
        setEditorState(prev => ({
            ...prev,
            selectionStartLine: null,
            selectionEndLine: null
        }));
    }, []);

    const setEnableLsp = useCallback((enabled: boolean) => {
        setEditorState(prev => ({ ...prev, enableLsp: enabled }));
    }, []);

    return (
        <EditorContext.Provider value={{
            editorState,
            setActiveFile,
            setCursorPosition,
            setSelection,
            clearSelection,
            setEnableLsp
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
