"use client";
import React, { useEffect, useRef, forwardRef, useImperativeHandle, useCallback } from "react";
import { useTranslation } from 'react-i18next';
import { EditorState, Compartment, Prec } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightActiveLine, drawSelection, dropCursor, rectangularSelection, crosshairCursor, placeholder, highlightSpecialChars } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, indentOnInput, foldGutter, foldKeymap } from "@codemirror/language";
import { searchKeymap } from "@codemirror/search";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";

// Custom theme and extensions
import { getZaguanTheme } from "./editor/theme/zaguanTheme";
import { loadLanguageExtension } from "./editor/languages";
import {
    lineHighlightField,
    addLineHighlight,
    clearLineHighlight,
    virtualBufferField,
    setBaseContent,
    scrollPastEnd,
    diffDecorations,
    diffStateField,
    setDiffState,
    parseUnifiedDiff,
    aiGlowDecorations,
    triggerAiGlow,
    zlpHoverTooltip,
} from "./editor/extensions";
import { zlpLinter } from "./editor/extensions/zlpLinter";
import { useEditorActions } from "../contexts/EditorContext";
import { useTheme } from "../contexts/ThemeContext";
import { useContextMenu, type ContextMenuItem } from "./ui/ContextMenu";
import { Copy, Scissors, Clipboard, Undo2, Redo2, Search, Network } from "lucide-react";
import { ZLPService } from "../services/zlp";
import { StructureNode } from "../types/zlp";
import { GraphInspector } from "./GraphInspector";


const getDiffStateFromUnifiedDiff = (unifiedDiff?: string) => {
    if (!unifiedDiff) return null;
    return {
        lines: parseUnifiedDiff(unifiedDiff),
        originalContent: ''
    };
};


interface CodeEditorProps {
    content: string;
    onChange: (val: string) => void;
    onSave?: (val: string) => void;
    filename?: string;
    externalContentVersion?: number;
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


const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(({ content, onChange, onSave, filename, externalContentVersion = 0, highlightLines, onNavigate, lineWrap, unifiedDiff }, ref) => {
    const { t } = useTranslation();
    // Auto-enable line wrap for markdown files
    const isMarkdown = filename?.endsWith('.md') || filename?.endsWith('.markdown');
    const shouldWrap = lineWrap ?? isMarkdown;
    const editorRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const languageConf = useRef(new Compartment());
    const themeConf = useRef(new Compartment());
    const wrapConf = useRef(new Compartment());
    const { setCursorPosition, setSelection, clearSelection } = useEditorActions();
    const { currentTheme } = useTheme();
    const { showMenu } = useContextMenu();

    // Call Graph Inspector State
    const [inspectorData, setInspectorData] = React.useState<{ id: string; name: string } | null>(null);


    // Track whether a content change was user-initiated (to avoid update loops)
    const isUserEditRef = useRef(false);
    const languageRequestIdRef = useRef(0);
    const selectionSyncFrameRef = useRef<number | null>(null);
    const pendingSelectionSyncRef = useRef<{
        line: number;
        column: number;
        selectionStartLine: number | null;
        selectionEndLine: number | null;
    } | null>(null);

    // Ref to capture the latest onSave callback (avoids stale closure in keymap)
    const onSaveRef = useRef(onSave);
    const onChangeRef = useRef(onChange);
    useEffect(() => {
        onSaveRef.current = onSave;
    }, [onSave]);
    useEffect(() => {
        onChangeRef.current = onChange;
    }, [onChange]);

    useEffect(() => {
        return () => {
            if (selectionSyncFrameRef.current !== null) {
                cancelAnimationFrame(selectionSyncFrameRef.current);
                selectionSyncFrameRef.current = null;
            }
        };
    }, []);

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

    const reconfigureLanguage = useCallback(async (targetFilename?: string) => {
        const requestId = ++languageRequestIdRef.current;
        const extensions = await loadLanguageExtension(targetFilename);

        if (requestId !== languageRequestIdRef.current) {
            return;
        }

        const view = viewRef.current;
        if (!view) {
            return;
        }

        view.dispatch({
            effects: languageConf.current.reconfigure(extensions),
        });
    }, []);

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
                ...(isMarkdown ? [] : [autocompletion()]),
                indentOnInput(),

                themeConf.current.of(getZaguanTheme(currentTheme.appearance === 'dark')),

                // UX enhancements
                placeholder("Start typing or paste code here..."),
                highlightSpecialChars(),

                // Error handling
                EditorView.exceptionSink.of(exception => {
                    console.error('[CodeMirror Error]', exception);
                }),

                // Custom extensions for enhanced UX
                // Disable heavy extensions for markdown (rainbow brackets, indent guides)
                scrollPastEnd,

                // Editor state extensions
                lineHighlightField,
                virtualBufferField,
                diffDecorations(),
                aiGlowDecorations(),

                // Line wrapping (enabled for markdown and when explicitly requested)
                wrapConf.current.of(shouldWrap ? [EditorView.lineWrapping] : []),

                // Layout
                EditorView.theme({
                    "&": { height: "100%" },
                    ".cm-scroller": { overflow: "auto" }
                }),

                // Language support (dynamic)
                languageConf.current.of([]),

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
                        onChangeRef.current(update.state.doc.toString());
                    }

                    if (update.selectionSet) {
                        // Markdown typing can feel sluggish when we push context updates for
                        // every keystroke-driven cursor movement. Keep cursor/selection sync for
                        // explicit navigation/selection changes, and skip docChanged+selectionSet
                        // updates while typing in markdown.
                        if (isMarkdown && update.docChanged) {
                            return;
                        }

                        const { main } = update.state.selection;
                        const line = update.state.doc.lineAt(main.head);
                        pendingSelectionSyncRef.current = {
                            line: line.number,
                            column: main.head - line.from,
                            selectionStartLine: main.from !== main.to ? update.state.doc.lineAt(main.from).number : null,
                            selectionEndLine: main.from !== main.to ? update.state.doc.lineAt(main.to).number : null,
                        };

                        if (selectionSyncFrameRef.current === null) {
                            selectionSyncFrameRef.current = requestAnimationFrame(() => {
                                selectionSyncFrameRef.current = null;
                                const pending = pendingSelectionSyncRef.current;
                                if (!pending) {
                                    return;
                                }

                                setCursorPosition(pending.line, pending.column);
                                if (pending.selectionStartLine !== null && pending.selectionEndLine !== null) {
                                    setSelection(pending.selectionStartLine, pending.selectionEndLine);
                                } else {
                                    clearSelection();
                                }
                            });
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
        void reconfigureLanguage(filename);

        return () => {
            languageRequestIdRef.current += 1;
            view.destroy();
            viewRef.current = null;
        };
    }, [filename, isMarkdown, reconfigureLanguage, setCursorPosition, setSelection, clearSelection, shouldWrap]);

    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        view.dispatch({
            effects: themeConf.current.reconfigure(getZaguanTheme(currentTheme.appearance === 'dark')),
        });
    }, [currentTheme.appearance]);

    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        view.dispatch({
            effects: wrapConf.current.reconfigure(shouldWrap ? [EditorView.lineWrapping] : []),
        });
    }, [shouldWrap]);

    useEffect(() => {
        void reconfigureLanguage(filename);
    }, [filename, reconfigureLanguage]);

    // Handle file switch and content updates
    const lastFilename = useRef(filename);
    const lastExternalContentVersionRef = useRef(externalContentVersion);
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        const isFileSwitch = filename !== lastFilename.current;
        const isExternalContentUpdate = externalContentVersion !== lastExternalContentVersionRef.current;

        if (isFileSwitch) {
            lastFilename.current = filename;
            lastExternalContentVersionRef.current = externalContentVersion;

            const diffState = getDiffStateFromUnifiedDiff(unifiedDiff);

            // Replace entire document content
            view.dispatch({
                changes: { from: 0, to: view.state.doc.length, insert: content },
                effects: [
                    setBaseContent.of(content), // Initialize virtual buffer with base content
                    setDiffState.of(diffState),
                ]
            });

        } else if (isMarkdown && !isUserEditRef.current) {
            // Only sync external content changes (e.g., file loaded, external modification)
            // Skip if this was a user edit to prevent feedback loops
            const currentDoc = view.state.doc.toString();
            if (currentDoc !== content) {
                const diffState = getDiffStateFromUnifiedDiff(unifiedDiff);
                view.dispatch({
                    changes: { from: 0, to: view.state.doc.length, insert: content },
                    effects: [
                        setBaseContent.of(content), // Update base content
                        setDiffState.of(diffState),
                    ]
                });
            }
        } else if (!isMarkdown && isExternalContentUpdate) {
            lastExternalContentVersionRef.current = externalContentVersion;

            const currentDoc = view.state.doc.toString();
            if (currentDoc !== content) {
                const diffState = getDiffStateFromUnifiedDiff(unifiedDiff);
                view.dispatch({
                    changes: { from: 0, to: view.state.doc.length, insert: content },
                    effects: [
                        setBaseContent.of(content),
                        setDiffState.of(diffState),
                    ]
                });
            }
        }
        // Reset the user edit flag after processing
        isUserEditRef.current = false;
    }, [filename, content, externalContentVersion, onNavigate, unifiedDiff, isMarkdown]);

    // Apply diff decorations when unifiedDiff changes
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        if (unifiedDiff) {
            view.dispatch({
                effects: [
                    setDiffState.of(getDiffStateFromUnifiedDiff(unifiedDiff)),
                    triggerAiGlow.of(undefined),
                ]
            });
        } else {
            // Clear diff state when no diff
            view.dispatch({
                effects: setDiffState.of(null)
            });
        }
    }, [unifiedDiff]);


    // Handle line highlighting when highlightLines prop changes
    const lastAppliedHighlightRef = useRef<string | null>(null);
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        const highlightKey = highlightLines
            ? `${filename ?? ''}:${highlightLines.startLine}:${highlightLines.endLine}`
            : null;

        if (highlightLines) {
            if (view.state.doc.length === 0 || highlightKey === lastAppliedHighlightRef.current) {
                return;
            }

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
                    lastAppliedHighlightRef.current = highlightKey;
                }
            } catch (error) {
                console.error('Error applying line highlight:', error);
            }
        } else {
            lastAppliedHighlightRef.current = null;
            view.dispatch({
                effects: clearLineHighlight.of(null)
            });
        }
    }, [highlightLines, filename]);

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
                label: t('fileTree.cut'),
                icon: <Scissors className="w-4 h-4" />,
                shortcut: 'Ctrl+X',
                disabled: !hasSelection,
                onClick: () => {
                    document.execCommand('cut');
                }
            },
            {
                id: 'copy',
                label: t('common.copy'),
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                disabled: !hasSelection,
                onClick: () => {
                    document.execCommand('copy');
                }
            },
            {
                id: 'paste',
                label: t('fileTree.paste'),
                icon: <Clipboard className="w-4 h-4" />,
                shortcut: 'Ctrl+V',
                onClick: () => {
                    document.execCommand('paste');
                }
            },
            { id: 'div-1', label: '', divider: true },
            {
                id: 'undo',
                label: t('common.undo'),
                icon: <Undo2 className="w-4 h-4" />,
                shortcut: 'Ctrl+Z',
                onClick: () => {
                    document.execCommand('undo');
                }
            },
            {
                id: 'redo',
                label: t('contextMenu.redo'),
                icon: <Redo2 className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+Z',
                onClick: () => {
                    document.execCommand('redo');
                }
            },
            { id: 'div-2', label: '', divider: true },
            {
                id: 'find',
                label: t('contextMenu.find'),
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
                label: t('contextMenu.renameSymbol'),
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
                label: t('contextMenu.showCallGraph'),
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
    }, [showMenu, t]);

    return (
        <div className="code-editor-scroll h-full w-full relative" onContextMenu={handleContextMenu}>
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
