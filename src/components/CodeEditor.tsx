"use client";
import React, { useEffect, useRef, forwardRef, useImperativeHandle, useCallback } from "react";
import { EditorState, Compartment } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightActiveLine, drawSelection, dropCursor, rectangularSelection, crosshairCursor } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, indentOnInput, foldGutter, foldKeymap } from "@codemirror/language";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";

// Custom theme and extensions
import { zaguanTheme } from "./editor/theme/zaguanTheme";
import { getLanguageExtension } from "./editor/languages";
import {
    diffsField,
    clearDiffs,
    lineHighlightField,
    addLineHighlight,
    clearLineHighlight,
    virtualBufferField,
    setBaseContent,
    getVirtualContent,
    inlineDiffField,
    inlineDiffTheme,
    setInlineDiff,
    clearInlineDiff,
    computeDiffLines,
    indentGuides,
    rainbowBrackets,
    smoothCursor,
    scrollPastEnd,
    languageFeatures,
    diagnosticsExtension,
    signatureHelpExtension,
    codeActionsExtension,
    referencesExtension,
    renameExtension,
} from "./editor/extensions";
import { useEditor } from "../contexts/EditorContext";
import { useContextMenu, type ContextMenuItem } from "./ui/ContextMenu";
import { Copy, Scissors, Clipboard, Undo2, Redo2, Search } from "lucide-react";
import type { Change } from "../types/change";
import { LanguageService } from "../services/language";

// Map file extension to LSP language ID
function getLanguageId(filename: string): string {
    const ext = filename.split('.').pop()?.toLowerCase() || '';
    switch (ext) {
        case 'ts': case 'tsx': return 'typescript';
        case 'js': case 'jsx': return 'javascript';
        case 'rs': return 'rust';
        case 'py': return 'python';
        case 'go': return 'go';
        case 'json': return 'json';
        case 'html': return 'html';
        case 'css': return 'css';
        case 'md': return 'markdown';
        default: return 'plaintext';
    }
}

interface CodeEditorProps {
    content: string;
    onChange: (val: string) => void;
    onSave?: (val: string) => void;
    filename?: string;
    highlightLines?: { startLine: number; endLine: number } | null;
    /** Pending change to highlight inline (Windsurf-style) */
    pendingChange?: Change | null;
    /** Callback for navigating to symbol definition */
    onNavigate?: (path: string, line: number, character: number) => void;
}


export interface CodeEditorHandle {
    getView: () => EditorView | null;
    clearDiffs: () => void;
    /** Show inline diff highlighting for a pending change */
    showInlineDiff: (change: Change | null) => void;
    /** Set cursor position and scroll into view (line is 1-based, col is 0-based) */
    setCursor: (line: number, col: number) => void;
}


const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(({ content, onChange, onSave, filename, highlightLines, onNavigate }, ref) => {
    const editorRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const languageConf = useRef(new Compartment());
    const languageFeaturesConf = useRef(new Compartment());
    const diagnosticsConf = useRef(new Compartment());
    const signatureHelpConf = useRef(new Compartment());
    const codeActionsConf = useRef(new Compartment());
    const referencesConf = useRef(new Compartment());
    const renameConf = useRef(new Compartment());
    const { setCursorPosition, setSelection, clearSelection } = useEditor();
    const { showMenu } = useContextMenu();

    // LSP document sync tracking
    const documentVersion = useRef(0);
    const didChangeTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Track whether a content change was user-initiated (to avoid update loops)
    const isUserEditRef = useRef(false);

    // Ref to capture the latest onSave callback (avoids stale closure in keymap)
    const onSaveRef = useRef(onSave);
    useEffect(() => {
        onSaveRef.current = onSave;
    }, [onSave]);

    // Expose methods to parent via ref
    useImperativeHandle(ref, () => ({
        getView: () => viewRef.current,
        clearDiffs: () => {
            if (viewRef.current) {
                viewRef.current.dispatch({
                    effects: clearDiffs.of(null)
                });
            }
        },
        showInlineDiff: (change: Change | null) => {
            const view = viewRef.current;
            if (!view) return;

            if (!change) {
                // Clear inline diff
                view.dispatch({ effects: clearInlineDiff.of(null) });
                return;
            }

            // Convert Change to PendingInlineDiff
            const currentContent = view.state.doc.toString();
            const hunks: { id: string; fromLine: number; toLine: number; oldText: string; newText: string }[] = [];

            if (change.change_type === 'patch') {
                const diffInfo = computeDiffLines(currentContent, change.old_content, change.new_content);
                if (diffInfo) {
                    hunks.push({
                        id: `${change.id}-0`,
                        fromLine: diffInfo.removedLines[0] || 1,
                        toLine: diffInfo.removedLines[diffInfo.removedLines.length - 1] || 1,
                        oldText: change.old_content,
                        newText: change.new_content,
                    });
                }
            } else if (change.change_type === 'multi_patch') {
                change.patches.forEach((patch, idx) => {
                    const diffInfo = computeDiffLines(currentContent, patch.old_text, patch.new_text);
                    if (diffInfo) {
                        hunks.push({
                            id: `${change.id}-${idx}`,
                            fromLine: patch.start_line || diffInfo.removedLines[0] || 1,
                            toLine: patch.end_line || diffInfo.removedLines[diffInfo.removedLines.length - 1] || 1,
                            oldText: patch.old_text,
                            newText: patch.new_text,
                        });
                    }
                });
            }

            if (hunks.length > 0) {
                view.dispatch({
                    effects: setInlineDiff.of({
                        id: change.id,
                        hunks,
                        path: change.path,
                    })
                });
            }
        },
        setCursor: (line: number, col: number) => {
            const view = viewRef.current;
            if (!view) return;

            const doc = view.state.doc;
            const safeLine = Math.max(1, Math.min(line, doc.lines));
            const lineObj = doc.line(safeLine);
            const safeCol = Math.max(0, Math.min(col, lineObj.length));

            const pos = lineObj.from + safeCol;

            view.dispatch({
                selection: { anchor: pos, head: pos },
                effects: EditorView.scrollIntoView(pos, { y: "center" })
            });
            view.focus();
        }
    }));

    // Initial setup
    useEffect(() => {
        if (!editorRef.current) return;
        if (viewRef.current) return;

        const state = EditorState.create({
            doc: content,
            extensions: [
                // Core editor features
                lineNumbers(),
                highlightActiveLineGutter(),
                highlightActiveLine(),
                foldGutter(),
                drawSelection(),
                dropCursor(),
                rectangularSelection(),
                crosshairCursor(),
                lintGutter(),

                // Editing features
                history(),
                bracketMatching(),
                closeBrackets(),
                autocompletion(),
                highlightSelectionMatches(),
                indentOnInput(),

                // Custom Zaguan theme (includes syntax highlighting)
                zaguanTheme,

                // Custom extensions for enhanced UX
                indentGuides,
                rainbowBrackets,
                smoothCursor,
                scrollPastEnd,

                // Diff and virtual buffer extensions
                diffsField,
                lineHighlightField,
                virtualBufferField,
                inlineDiffField,
                inlineDiffTheme,

                // Layout
                EditorView.theme({
                    "&": { height: "100%" },
                    ".cm-scroller": { overflow: "auto" }
                }),

                // Language support (dynamic)
                languageConf.current.of(getLanguageExtension(filename)),
                languageFeaturesConf.current.of(languageFeatures(filename || "", onNavigate)),
                diagnosticsConf.current.of(diagnosticsExtension(filename || "")),
                signatureHelpConf.current.of(signatureHelpExtension(filename || "")),
                codeActionsConf.current.of(codeActionsExtension(filename || "")),
                referencesConf.current.of(referencesExtension(filename || "", (path, line, char) => {
                    if (onNavigate) onNavigate(path, line, char);
                })),
                renameConf.current.of(renameExtension(filename || "", (changes) => {
                    // Start of handling rename edits
                    console.log("Applying rename edits:", changes);
                    // For now we just log, real implementation requires workspace edit handling
                    // which is currently out of scope for this specific file editor component
                })),

                // Keymaps
                keymap.of([
                    indentWithTab,
                    {
                        key: "Mod-s",
                        run: (view) => {
                            if (onSaveRef.current) {
                                // Use the actual editor document content, not virtual buffer
                                // The document is the source of truth for user edits
                                const content = view.state.doc.toString();
                                onSaveRef.current(content);
                            }
                            return true;
                        }
                    },
                    ...closeBracketsKeymap,
                    ...defaultKeymap,
                    ...searchKeymap,
                    ...historyKeymap,
                    ...foldKeymap,
                    ...completionKeymap,
                ]),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        // Mark this as a user-initiated edit to prevent feedback loops
                        isUserEditRef.current = true;
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
                    languageFeaturesConf.current.reconfigure(languageFeatures(filename || "", onNavigate)),
                    diagnosticsConf.current.reconfigure(diagnosticsExtension(filename || "")),
                    signatureHelpConf.current.reconfigure(signatureHelpExtension(filename || "")),
                    codeActionsConf.current.reconfigure(codeActionsExtension(filename || "")),
                    referencesConf.current.reconfigure(referencesExtension(filename || "", (path, line, char) => {
                        if (onNavigate) onNavigate(path, line, char);
                    })),
                    setBaseContent.of(content) // Initialize virtual buffer with base content
                ]
            });

            // Notify LSP that file was opened
            if (filename) {
                const languageId = getLanguageId(filename);
                documentVersion.current = 1;
                LanguageService.didOpen(filename, content, languageId).catch(e =>
                    console.warn('[LSP] didOpen failed:', e)
                );
            }
        } else if (!isUserEditRef.current) {
            // Only sync external content changes (e.g., file loaded, external modification)
            // Skip if this was a user edit to prevent feedback loops
            const currentDoc = view.state.doc.toString();
            if (currentDoc !== content) {
                view.dispatch({
                    changes: { from: 0, to: view.state.doc.length, insert: content },
                    effects: setBaseContent.of(content) // Update base content
                });
            }
        }
        // Reset the user edit flag after processing
        isUserEditRef.current = false;
    }, [filename, content, onNavigate]);

    // Send didChange to LSP on document changes (debounced)
    useEffect(() => {
        const view = viewRef.current;
        if (!view || !filename) return;

        // Debounce didChange notifications
        if (didChangeTimeout.current) {
            clearTimeout(didChangeTimeout.current);
        }

        didChangeTimeout.current = setTimeout(async () => {
            documentVersion.current += 1;
            try {
                await LanguageService.didChange(filename, content, documentVersion.current);

                // After a short delay for LSP to process, fetch diagnostics
                setTimeout(async () => {
                    try {
                        const diagnostics = await LanguageService.getDiagnostics(filename);
                        // Diagnostics will be delivered via DiagnosticsUpdated event
                        // which the diagnosticsExtension listens for
                    } catch (e) {
                        // Diagnostics fetch is best-effort
                    }
                }, 200);
            } catch (e) {
                console.warn('[LSP] didChange failed:', e);
            }
        }, 150); // 150ms debounce

        return () => {
            if (didChangeTimeout.current) {
                clearTimeout(didChangeTimeout.current);
            }
        };
    }, [content, filename]);

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

    // Custom context menu for the editor
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const view = viewRef.current;
        if (!view) return;

        const hasSelection = view.state.selection.main.from !== view.state.selection.main.to;

        const items: ContextMenuItem[] = [
            {
                id: 'cut',
                label: 'Cut',
                icon: <Scissors className="w-4 h-4" />,
                shortcut: 'Ctrl+X',
                disabled: !hasSelection,
                onClick: () => {
                    document.execCommand('cut');
                }
            },
            {
                id: 'copy',
                label: 'Copy',
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                disabled: !hasSelection,
                onClick: () => {
                    document.execCommand('copy');
                }
            },
            {
                id: 'paste',
                label: 'Paste',
                icon: <Clipboard className="w-4 h-4" />,
                shortcut: 'Ctrl+V',
                onClick: () => {
                    document.execCommand('paste');
                }
            },
            { id: 'div-1', label: '', divider: true },
            {
                id: 'undo',
                label: 'Undo',
                icon: <Undo2 className="w-4 h-4" />,
                shortcut: 'Ctrl+Z',
                onClick: () => {
                    document.execCommand('undo');
                }
            },
            {
                id: 'redo',
                label: 'Redo',
                icon: <Redo2 className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+Z',
                onClick: () => {
                    document.execCommand('redo');
                }
            },
            { id: 'div-2', label: '', divider: true },
            {
                id: 'find',
                label: 'Find',
                icon: <Search className="w-4 h-4" />,
                shortcut: 'Ctrl+F',
                onClick: () => {
                    // Trigger CodeMirror's search
                    const event = new KeyboardEvent('keydown', {
                        key: 'f',
                        ctrlKey: true,
                        bubbles: true
                    });
                    view.contentDOM.dispatchEvent(event);
                }
            },
            {
                id: 'rename',
                label: 'Rename Symbol',
                shortcut: 'F2',
                onClick: () => {
                    // Dispatch F2 key event to trigger the rename extension
                    const event = new KeyboardEvent('keydown', {
                        key: 'F2',
                        bubbles: true
                    });
                    view.contentDOM.dispatchEvent(event);
                }
            }
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [showMenu]);

    return (
        <div className="h-full w-full relative" onContextMenu={handleContextMenu}>
            <div ref={editorRef} className="h-full w-full overflow-hidden text-base" />
        </div>
    );
});

CodeEditor.displayName = 'CodeEditor';

export default CodeEditor;
