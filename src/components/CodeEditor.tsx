"use client";
import React, { useEffect, useRef, forwardRef, useImperativeHandle, useCallback } from "react";
import { useTranslation } from 'react-i18next';
import { Annotation, EditorState, Compartment, Prec, type Extension, type TransactionSpec } from "@codemirror/state";
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
    createDiffStateFromUnifiedDiff,
    resolveDiffStateUpdate,
    aiGlowDecorations,
    triggerAiGlow,
    zlpHoverTooltip,
} from "./editor/extensions";
import { zlpLinter } from "./editor/extensions/zlpLinter";
import { useEditorActions } from "../contexts/EditorContext";
import { useTheme } from "../contexts/ThemeContext";
import { useContextMenu, type ContextMenuItem } from "./ui/ContextMenu";
import { Copy, Scissors, Clipboard, Undo2, Redo2, Search, Network } from "lucide-react";
import { SymbolsIndexService, type InspectorSymbol } from "../services/symbolIndex";
import { GraphInspector } from "./GraphInspector";
import { normalizeEditorPath } from "../utils/editorBufferRegistry";

const EDITING_AID_MAX_DOC_LENGTH = 500_000;

const runPreservingScroll = (view: EditorView, run: () => void) => {
    const scroller = view.scrollDOM;
    const scrollTop = scroller.scrollTop;
    const scrollLeft = scroller.scrollLeft;

    run();

    const restoreScroll = () => {
        if (!scroller.isConnected) {
            return;
        }

        scroller.scrollTop = scrollTop;
        scroller.scrollLeft = scrollLeft;
    };

    restoreScroll();
    requestAnimationFrame(() => {
        restoreScroll();
        requestAnimationFrame(restoreScroll);
    });
};

const dispatchPreservingScroll = (view: EditorView, spec: TransactionSpec) => {
    runPreservingScroll(view, () => view.dispatch(spec));
};

type EditorMainSelection = { anchor: number; head?: number };

const externalDocumentUpdate = Annotation.define<boolean>();

const getClampedMainSelection = (view: EditorView, contentLength: number): EditorMainSelection => {
    const { main } = view.state.selection;
    return {
        anchor: Math.min(main.anchor, contentLength),
        head: Math.min(main.head, contentLength),
    };
};

const replaceEditorDocument = (view: EditorView, content: string, unifiedDiff?: string | null, preserveScroll = false) => {
    const diffState = resolveDiffStateUpdate(
        view.state.field(diffStateField, false),
        unifiedDiff,
    );
    const selection = getClampedMainSelection(view, content.length);

    const spec = {
        changes: { from: 0, to: view.state.doc.length, insert: content },
        selection,
        annotations: externalDocumentUpdate.of(true),
        effects: [
            setBaseContent.of(content),
            setDiffState.of(diffState),
        ]
    };

    if (preserveScroll) {
        dispatchPreservingScroll(view, spec);
    } else {
        view.dispatch(spec);
    }
};

const getEditingAidExtensions = (isMarkdown: boolean, docLength: number): Extension[] => {
    if (isMarkdown || docLength > EDITING_AID_MAX_DOC_LENGTH) {
        return [];
    }

    return [
        lintGutter(),
        bracketMatching(),
        closeBrackets(),
        autocompletion(),
    ];
};

interface CodeEditorProps {
    content: string;
    onDocumentChange?: () => void;
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
    getContent: () => string;
    replaceDocument: (input: CodeEditorReplaceDocumentInput) => void;
    /** Set cursor position and scroll into view (line is 1-based, col is 0-based) */
    setCursor: (line: number, col: number) => void;
}

export type CodeEditorReplaceDocumentReason = 'open' | 'reload' | 'revert' | 'external-clean-update';

export type CodeEditorReplaceDocumentInput = {
    path?: string | null;
    content: string;
    resetHistory: boolean;
    reason: CodeEditorReplaceDocumentReason;
    /** Omit to preserve the current review diff; pass null to clear it. */
    unifiedDiff?: string | null;
    preserveScroll?: boolean;
    forceEffects?: boolean;
};


const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(({ content, onDocumentChange, onSave, filename, externalContentVersion = 0, highlightLines, onNavigate, lineWrap, unifiedDiff }, ref) => {
    const { t } = useTranslation();
    // Auto-enable line wrap for markdown files
    const isMarkdown = filename?.endsWith('.md') || filename?.endsWith('.markdown') || false;
    const shouldWrap = lineWrap ?? isMarkdown;
    const editorRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);
    const languageConf = useRef(new Compartment());
    const themeConf = useRef(new Compartment());
    const wrapConf = useRef(new Compartment());
    const editingAidConf = useRef(new Compartment());
    const zlpConf = useRef(new Compartment());
    const { setCursorPosition, setSelection, clearSelection } = useEditorActions();
    const { currentTheme } = useTheme();
    const { showMenu } = useContextMenu();
    const contentRef = useRef(content);
    const filenameRef = useRef(filename);
    const unifiedDiffRef = useRef(unifiedDiff);
    const lineWrapRef = useRef(lineWrap);
    const themeAppearanceRef = useRef(currentTheme.appearance);
    const editorActionsRef = useRef({ setCursorPosition, setSelection, clearSelection });

    contentRef.current = content;
    filenameRef.current = filename;
    unifiedDiffRef.current = unifiedDiff;
    lineWrapRef.current = lineWrap;
    themeAppearanceRef.current = currentTheme.appearance;
    editorActionsRef.current = { setCursorPosition, setSelection, clearSelection };

    // Local Symbols Index Graph Inspector State
    const [inspectorData, setInspectorData] = React.useState<InspectorSymbol | null>(null);


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
    const onDocumentChangeRef = useRef(onDocumentChange);
    useEffect(() => {
        onSaveRef.current = onSave;
    }, [onSave]);
    useEffect(() => {
        onDocumentChangeRef.current = onDocumentChange;
    }, [onDocumentChange]);

    // Latest translator, read at editor-creation time. Depending on `t`
    // directly would change createEditorState's identity on language switch
    // and recreate the editor (losing undo/scroll state) for a placeholder.
    const tRef = useRef(t);
    useEffect(() => {
        tRef.current = t;
    }, [t]);

    useEffect(() => {
        return () => {
            if (selectionSyncFrameRef.current !== null) {
                cancelAnimationFrame(selectionSyncFrameRef.current);
                selectionSyncFrameRef.current = null;
            }
        };
    }, []);

    const createEditorState = useCallback((
        targetContent: string,
        targetFilename?: string | null,
        initialSelection?: EditorMainSelection,
    ) => {
        const targetIsMarkdown = targetFilename?.endsWith('.md') || targetFilename?.endsWith('.markdown') || false;
        const targetShouldWrap = lineWrapRef.current ?? targetIsMarkdown;

        return EditorState.create({
            doc: targetContent,
            selection: initialSelection,
            extensions: [
                // Core editor features
                lineNumbers(),
                highlightActiveLineGutter(),
                highlightActiveLine(),
                foldGutter(),
                drawSelection({ cursorBlinkRate: 0 }),
                dropCursor(),
                rectangularSelection(),
                crosshairCursor(),
                editingAidConf.current.of(getEditingAidExtensions(targetIsMarkdown, targetContent.length)),

                // Editing features
                history(),
                indentOnInput(),

                themeConf.current.of(getZaguanTheme(themeAppearanceRef.current === 'dark')),

                // UX enhancements
                placeholder(tRef.current('editor.placeholder')),
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
                wrapConf.current.of(targetShouldWrap ? [EditorView.lineWrapping] : []),

                // Layout
                EditorView.theme({
                    "&": { height: "100%" },
                    ".cm-scroller": { overflow: "auto" }
                }),

                // Language support (dynamic)
                languageConf.current.of([]),

                zlpConf.current.of(targetIsMarkdown ? [] : [
                    zlpLinter(targetFilename || ''),
                    zlpHoverTooltip(targetFilename || '')
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
                    if (
                        update.docChanged
                        && !update.transactions.some(transaction => transaction.annotation(externalDocumentUpdate))
                    ) {
                        onDocumentChangeRef.current?.();
                    }

                    if (update.selectionSet) {
                        // Markdown typing can feel sluggish when we push context updates for
                        // every keystroke-driven cursor movement. Keep cursor/selection sync for
                        // explicit navigation/selection changes, and skip docChanged+selectionSet
                        // updates while typing.
                        if (update.docChanged && update.state.selection.main.empty) {
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

                                editorActionsRef.current.setCursorPosition(pending.line, pending.column);
                                if (pending.selectionStartLine !== null && pending.selectionEndLine !== null) {
                                    editorActionsRef.current.setSelection(pending.selectionStartLine, pending.selectionEndLine);
                                } else {
                                    editorActionsRef.current.clearSelection();
                                }
                            });
                        }
                    }
                })
            ]
        });
    }, []);

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

    const replaceDocument = useCallback((input: CodeEditorReplaceDocumentInput) => {
        const view = viewRef.current;
        if (!view) return;
        if (
            input.path
            && filenameRef.current
            && normalizeEditorPath(input.path) !== normalizeEditorPath(filenameRef.current)
        ) {
            return;
        }

        if (!input.resetHistory && view.state.doc.toString() === input.content) {
            if (!input.forceEffects) {
                return;
            }

            const effectsSpec = {
                effects: [
                    setBaseContent.of(input.content),
                    setDiffState.of(resolveDiffStateUpdate(
                        view.state.field(diffStateField, false),
                        input.unifiedDiff,
                    )),
                ],
            };
            if (input.preserveScroll) {
                dispatchPreservingScroll(view, effectsSpec);
            } else {
                view.dispatch(effectsSpec);
            }
            contentRef.current = input.content;
            return;
        }

        if (input.resetHistory) {
            const targetFilename = input.path ?? filenameRef.current;
            const resetEditorState = () => {
                const initialSelection = input.preserveScroll
                    ? getClampedMainSelection(view, input.content.length)
                    : undefined;
                view.setState(createEditorState(input.content, targetFilename, initialSelection));
                view.dispatch({
                    effects: [
                        setBaseContent.of(input.content),
                        setDiffState.of(resolveDiffStateUpdate(
                            view.state.field(diffStateField, false),
                            input.unifiedDiff,
                        )),
                    ],
                });
            };
            if (input.preserveScroll) {
                runPreservingScroll(view, resetEditorState);
            } else {
                resetEditorState();
            }
            void reconfigureLanguage(targetFilename ?? undefined);
        } else {
            replaceEditorDocument(
                view,
                input.content,
                input.unifiedDiff,
                input.preserveScroll,
            );
        }
        contentRef.current = input.content;
    }, [createEditorState, reconfigureLanguage]);

    // Expose methods to parent via ref
    useImperativeHandle(ref, () => ({
        getView: () => viewRef.current,
        getContent: () => viewRef.current?.state.doc.toString() ?? contentRef.current,
        replaceDocument,
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

        const initialFilename = filenameRef.current;
        const state = createEditorState(contentRef.current, initialFilename);

        const view = new EditorView({
            state,
            parent: editorRef.current
        });

        viewRef.current = view;
        void reconfigureLanguage(initialFilename);

        return () => {
            languageRequestIdRef.current += 1;
            view.destroy();
            viewRef.current = null;
        };
    }, [createEditorState, reconfigureLanguage]);

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
        const view = viewRef.current;
        if (!view) return;

        view.dispatch({
            effects: editingAidConf.current.reconfigure(getEditingAidExtensions(isMarkdown, content.length)),
        });
    }, [content.length, isMarkdown]);

    useEffect(() => {
        void reconfigureLanguage(filename);
    }, [filename, reconfigureLanguage]);

    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        view.dispatch({
            effects: zlpConf.current.reconfigure(isMarkdown ? [] : [
                zlpLinter(filename || ''),
                zlpHoverTooltip(filename || '')
            ]),
        });
    }, [filename, isMarkdown]);

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

            replaceDocument({
                path: filename,
                content: contentRef.current,
                resetHistory: true,
                reason: 'open',
                unifiedDiff: unifiedDiffRef.current,
            });

        } else if (isExternalContentUpdate) {
            lastExternalContentVersionRef.current = externalContentVersion;

            replaceDocument({
                path: filename,
                content: contentRef.current,
                resetHistory: false,
                reason: 'external-clean-update',
                unifiedDiff: unifiedDiffRef.current,
                preserveScroll: true,
            });
        }
    }, [filename, externalContentVersion, replaceDocument]);

    // Apply diff decorations when unifiedDiff changes
    useEffect(() => {
        const view = viewRef.current;
        if (!view) return;

        if (unifiedDiff) {
            const nextDiffState = createDiffStateFromUnifiedDiff(unifiedDiff);
            if (view.state.field(diffStateField, false) === nextDiffState) {
                return;
            }

            dispatchPreservingScroll(view, {
                effects: [
                    setDiffState.of(nextDiffState),
                    triggerAiGlow.of(undefined),
                ]
            });
        } else {
            if (!view.state.field(diffStateField, false)) {
                return;
            }

            dispatchPreservingScroll(view, {
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
                label: t('contextMenu.showSymbolGraph'),
                icon: <Network className="w-4 h-4" />,
                onClick: async () => {
                    if (!filename) return;
                    const pos = view.state.selection.main.head;
                    const line = view.state.doc.lineAt(pos);

                    try {
                        const symbol = await SymbolsIndexService.resolveSymbolAt(
                            filename,
                            line.number - 1,
                            pos - line.from,
                        );
                        if (symbol) {
                            setInspectorData(symbol);
                        } else {
                            console.warn("No indexed symbol found at cursor for graph");
                        }
                    } catch (e) {
                        console.error("Failed to resolve local symbol for graph", e);
                    }
                }
            }
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [filename, showMenu, t]);

    return (
        <div className="code-editor-scroll h-full w-full relative" data-font-zoom-scope="editor" onContextMenu={handleContextMenu}>
            <div ref={editorRef} className="h-full w-full overflow-hidden text-base" />

            {inspectorData && filename && (
                <GraphInspector
                    symbol={inspectorData}
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
