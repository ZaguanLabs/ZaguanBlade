"use client";
import React, { useEffect, useRef, forwardRef, useImperativeHandle, useCallback } from "react";
import { EditorState, Compartment, Prec } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightActiveLine, drawSelection, dropCursor, rectangularSelection, crosshairCursor, placeholder, highlightSpecialChars } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, indentOnInput, foldGutter, foldKeymap } from "@codemirror/language";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";

// Custom theme and extensions
import { zaguanTheme } from "./editor/theme/zaguanTheme";
import { getLanguageExtension } from "./editor/languages";
import {
    lineHighlightField,
    addLineHighlight,
    clearLineHighlight,
    virtualBufferField,
    setBaseContent,
    indentGuides,
    rainbowBrackets,
    smoothCursor,
    scrollPastEnd,
    diffDecorations,
    diffStateField,
    setDiffState,
    parseUnifiedDiff,
    zlpHoverTooltip,
} from "./editor/extensions";
import { zlpLinter } from "./editor/extensions/zlpLinter";
import { useEditor } from "../contexts/EditorContext";
import { useContextMenu, type ContextMenuItem } from "./ui/ContextMenu";
import { Copy, Scissors, Clipboard, Undo2, Redo2, Search, Network } from "lucide-react";
import { ZLPService } from "../services/zlp";
import { StructureNode } from "../types/zlp";
import { GraphInspector } from "./GraphInspector";


interface CodeEditorProps {
    content: string;
    onChange: (val: string) => void;
    onSave?: (val: string) => void;
    filename?: string;
    highlightLines?: { startLine: number; endLine: number } | null;
    /** Callback for navigating to symbol definition */
    onNavigate?: (path: string, line: number, character: number) => void;
    /** Enable soft line wrapping (default: false, auto-enabled for markdown) */
    lineWrap?: boolean;
    /** Unified diff string for showing change decorations */
    unifiedDiff?: string;
}


export interface CodeEditorHandle {
    getView: () => EditorView | null;
    /** Set cursor position and scroll into view (line is 1-based, col is 0-based) */
    setCursor: (line: number, col: number) => void;
}


const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(({ content, onChange, onSave, filename, highlightLines, onNavigate, lineWrap, unifiedDiff }, ref) => {
    // Auto-enable line wrap for markdown files
    const isMarkdown = filename?.endsWith('.md') || filename?.endsWith('.markdown');
    const shouldWrap = lineWrap ?? isMarkdown;
    const editorRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const languageConf = useRef(new Compartment());
    const { editorState, setCursorPosition, setSelection, clearSelection } = useEditor();
    const { showMenu } = useContextMenu();

    // Call Graph Inspector State
    const [inspectorData, setInspectorData] = React.useState<{ id: string; name: string } | null>(null);


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
                // Lint gutter only for code files (not markdown)
                ...(isMarkdown ? [] : [lintGutter()]),

                // Editing features
                history(),
                // Bracket matching/closing only for code files
                ...(isMarkdown ? [] : [bracketMatching(), closeBrackets()]),
                autocompletion(),
                highlightSelectionMatches(),
                indentOnInput(),

                // Custom Zaguan theme (includes syntax highlighting)
                zaguanTheme,

                // UX enhancements
                placeholder("Start typing or paste code here..."),
                highlightSpecialChars(),

                // Error handling
                EditorView.exceptionSink.of(exception => {
                    console.error('[CodeMirror Error]', exception);
                }),

                // Custom extensions for enhanced UX
                // Disable heavy extensions for markdown (rainbow brackets, indent guides)
                ...(isMarkdown ? [] : [indentGuides, rainbowBrackets]),
                smoothCursor,
                scrollPastEnd,

                // Editor state extensions
                lineHighlightField,
                virtualBufferField,
                diffDecorations(),

                // Line wrapping (enabled for markdown and when explicitly requested)
                ...(shouldWrap ? [EditorView.lineWrapping] : []),

                // Layout
                EditorView.theme({
                    "&": { height: "100%" },
                    ".cm-scroller": { overflow: "auto" }
                }),

                // Language support (dynamic)
                languageConf.current.of(getLanguageExtension(filename)),

                // ZLP Linter and Hover Tooltip (disabled for markdown - not applicable)
                ...(isMarkdown ? [] : [
                    zlpLinter(filename || ''),
                    zlpHoverTooltip(filename || '')
                ]),

                // Keymaps (high precedence for custom bindings)
                Prec.high(keymap.of([
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
                ])),
                EditorView.updateListener.of((update) => {
                    if (update.docChanged) {
                        isUserEditRef.current = true;
                        onChange(update.state.doc.toString());
                    }

                    if (update.selectionSet) {
                        const { main } = update.state.selection;
                        const line = update.state.doc.lineAt(main.head);
                        
                        setCursorPosition(line.number, main.head - line.from);

                        if (main.from !== main.to) {
                            setSelection(
                                update.state.doc.lineAt(main.from).number,
                                update.state.doc.lineAt(main.to).number
                            );
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

    // Apply diff decorations when unifiedDiff changes
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        if (unifiedDiff) {
            const diffLines = parseUnifiedDiff(unifiedDiff);
            view.dispatch({
                effects: setDiffState.of({
                    lines: diffLines,
                    originalContent: '' // We don't need this for decorations
                })
            });
        } else {
            // Clear diff state when no diff
            view.dispatch({
                effects: setDiffState.of(null)
            });
        }
    }, [unifiedDiff]);


    // Handle line highlighting when highlightLines prop changes
    useEffect(() => {
        const view = viewRef.current;
        if (!view || !content) return;

        if (highlightLines) {
            try {
                const { startLine, endLine } = highlightLines;
                if (startLine <= view.state.doc.lines) {
                    const line = view.state.doc.line(startLine);
                    view.dispatch({
                        effects: [
                            addLineHighlight.of({ startLine, endLine }),
                            EditorView.scrollIntoView(line.from, { y: "center" })
                        ]
                    });
                }
            } catch (error) {
                console.error('Error applying line highlight:', error);
            }
        } else {
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
