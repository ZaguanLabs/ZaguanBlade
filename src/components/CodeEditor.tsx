"use client";
import React, { useEffect, useRef, forwardRef, useImperativeHandle } from "react";
import { EditorState, Compartment } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching, indentOnInput, syntaxHighlighting, defaultHighlightStyle, foldGutter } from "@codemirror/language";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { oneDark } from "@codemirror/theme-one-dark";
import { rust } from "@codemirror/lang-rust";
import { javascript } from "@codemirror/lang-javascript";
import { diffsField, clearDiffs } from "./editor/extensions/diffView";
import { lineHighlightField, addLineHighlight, clearLineHighlight } from "./editor/extensions/lineHighlight";
import { virtualBufferField, setBaseContent, getVirtualContent } from "./editor/extensions/virtualBuffer";
import { useEditor } from "../contexts/EditorContext";

interface CodeEditorProps {
    content: string;
    onChange: (val: string) => void;
    onSave?: (val: string) => void;
    filename?: string;
    highlightLines?: { startLine: number; endLine: number } | null;
}

export interface CodeEditorHandle {
    getView: () => EditorView | null;
    clearDiffs: () => void;
}

const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(({ content, onChange, onSave, filename, highlightLines }, ref) => {
    const editorRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const languageConf = useRef(new Compartment());
    const { setCursorPosition, setSelection, clearSelection } = useEditor();

    // Expose methods to parent via ref
    useImperativeHandle(ref, () => ({
        getView: () => viewRef.current,
        clearDiffs: () => {
            if (viewRef.current) {
                viewRef.current.dispatch({
                    effects: clearDiffs.of(null)
                });
            }
        }
    }));

    const getLanguageExtension = (fname?: string) => {
        if (!fname) return [];
        if (fname.endsWith('.rs')) return rust();
        if (fname.endsWith('.js') || fname.endsWith('.jsx')) return javascript();
        if (fname.endsWith('.ts') || fname.endsWith('.tsx')) return javascript({ typescript: true });
        return [];
    };

    // Initial setup
    useEffect(() => {
        if (!editorRef.current) return;
        if (viewRef.current) return;

        const state = EditorState.create({
            doc: content,
            extensions: [
                lineNumbers(),
                highlightActiveLineGutter(),
                foldGutter(),
                history(),
                bracketMatching(),
                closeBrackets(),
                autocompletion(),
                highlightSelectionMatches(),
                indentOnInput(),
                syntaxHighlighting(defaultHighlightStyle),
                oneDark,
                diffsField,
                lineHighlightField,
                virtualBufferField,
                EditorView.theme({
                    "&": { height: "100%" },
                    ".cm-scroller": { overflow: "auto" }
                }),
                languageConf.current.of(getLanguageExtension(filename)),
                keymap.of([
                    {
                        key: "Mod-s",
                        run: (view) => {
                            if (onSave) {
                                // Save virtual content (base + accepted diffs)
                                const virtualContent = getVirtualContent(view);
                                onSave(virtualContent);
                            }
                            return true;
                        }
                    },
                    ...closeBracketsKeymap,
                    ...defaultKeymap,
                    ...searchKeymap,
                    ...historyKeymap,
                    ...completionKeymap,
                ]),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        onChange(update.state.doc.toString());
                    }
                    
                    // Track cursor position and selection
                    if (update.selectionSet) {
                        const selection = update.state.selection.main;
                        const line = update.state.doc.lineAt(selection.head);
                        const lineNumber = line.number;
                        const column = selection.head - line.from;
                        
                        setCursorPosition(lineNumber, column);
                        
                        // Track selection if there is one
                        if (selection.from !== selection.to) {
                            const startLine = update.state.doc.lineAt(selection.from).number;
                            const endLine = update.state.doc.lineAt(selection.to).number;
                            setSelection(startLine, endLine);
                        } else {
                            clearSelection();
                        }
                    }
                })
            ]
        });

        const view = new EditorView({
            state,
            parent: editorRef.current
        });

        viewRef.current = view;

        return () => {
            view.destroy();
            viewRef.current = null;
        };
    }, []);

    // Handle file switch and content updates
    const lastFilename = useRef(filename);
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        const isFileSwitch = filename !== lastFilename.current;

        if (isFileSwitch) {
            lastFilename.current = filename;

            // Replace entire document content
            view.dispatch({
                changes: { from: 0, to: view.state.doc.length, insert: content },
                effects: [
                    languageConf.current.reconfigure(getLanguageExtension(filename)),
                    setBaseContent.of(content) // Initialize virtual buffer with base content
                ]
            });
        } else {
            // Same file, but content changed externally (e.g., file loaded)
            const currentDoc = view.state.doc.toString();
            if (currentDoc !== content) {
                view.dispatch({
                    changes: { from: 0, to: view.state.doc.length, insert: content },
                    effects: setBaseContent.of(content) // Update base content
                });
            }
        }
    }, [filename, content]);

    // Handle line highlighting when highlightLines prop changes or content loads
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;
        
        // Only apply highlighting if we have content loaded
        if (!content) return;

        if (highlightLines) {
            try {
                // Small delay to ensure content is fully rendered
                setTimeout(() => {
                    if (!viewRef.current) return;
                    
                    // Apply line highlighting and scroll to the range
                    const startLine = viewRef.current.state.doc.line(highlightLines.startLine);
                    viewRef.current.dispatch({
                        effects: [
                            addLineHighlight.of({
                                startLine: highlightLines.startLine,
                                endLine: highlightLines.endLine
                            }),
                            EditorView.scrollIntoView(startLine.from, { y: "center" })
                        ]
                    });
                }, 100);
            } catch (error) {
                console.error('Error applying line highlight:', error);
            }
        } else {
            // Clear highlighting
            view.dispatch({
                effects: clearLineHighlight.of(null)
            });
        }
    }, [highlightLines, content]);

    return (
        <div className="h-full w-full relative">
            <div ref={editorRef} className="h-full w-full overflow-hidden text-base" />
        </div>
    );
});

CodeEditor.displayName = 'CodeEditor';

export default CodeEditor;
