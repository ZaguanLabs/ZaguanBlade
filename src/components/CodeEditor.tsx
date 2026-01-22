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
} from "./editor/extensions";
import { zlpLinter } from "./editor/extensions/zlpLinter";
import { useEditor } from "../contexts/EditorContext";
import { useContextMenu, type ContextMenuItem } from "./ui/ContextMenu";
import { Copy, Scissors, Clipboard, Undo2, Redo2, Search, Network } from "lucide-react";
import type { Change } from "../types/change";
import { LanguageService } from "../services/language";
import { ZLPService } from "../services/zlp";
import { StructureNode } from "../types/zlp";
import { GraphInspector } from "./GraphInspector";

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
    const { editorState, setCursorPosition, setSelection, clearSelection } = useEditor();
    const { showMenu } = useContextMenu();

    // Call Graph Inspector State
    const [inspectorData, setInspectorData] = React.useState<{ id: string; name: string } | null>(null);

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

                // ZLP Linter
                zlpLinter(filename || ''),

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
            },
            { id: 'div-3', label: '', divider: true },
            {
                id: 'graph',
                label: 'Show Call Graph',
                icon: <Network className="w-4 h-4" />,
                onClick: async () => {
                    if (!filename) return;
                    const pos = view.state.selection.main.head;
                    const line = view.state.doc.lineAt(pos);

                    try {
                        const structure = await ZLPService.getStructure(filename, "");

                        const findNode = (nodes: StructureNode[]): StructureNode | null => {
                            for (const node of nodes) {
                                // Structure ranges are 0-based in ZLP (usually)
                                const startLine = node.range.start.line;
                                const endLine = node.range.end.line;
                                const currentLine0 = line.number - 1;

                                if (currentLine0 >= startLine && currentLine0 <= endLine) {
                                    if (node.children) {
                                        const child = findNode(node.children);
                                        if (child) return child;
                                    }
                                    return node;
                                }
                            }
                            return null;
                        };

                        const node = findNode(structure);
                        if (node) {
                            setInspectorData({ id: node.name, name: node.name });
                        } else {
                            console.warn("No symbol found at cursor for graph");
                        }
                    } catch (e) {
                        console.error("Failed to resolve symbol for graph", e);
                    }
                }
            }
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [showMenu]);

    return (
        <div className="h-full w-full relative" onContextMenu={handleContextMenu}>
            <div ref={editorRef} className="h-full w-full overflow-hidden text-base" />

            {inspectorData && filename && (
                <GraphInspector
                    symbolId={inspectorData.id}
                    symbolName={inspectorData.name}
                    filePath={filename}
                    onClose={() => setInspectorData(null)}
                    onNavigate={(p, l, c) => {
                        if (onNavigate) onNavigate(p, l, c);
                    }}
                />
            )}
        </div>
    );
});

CodeEditor.displayName = 'CodeEditor';

export default CodeEditor;
