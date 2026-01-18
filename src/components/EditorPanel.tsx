'use client';
import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import CodeEditor, { type CodeEditorHandle } from './CodeEditor';
import { MarkdownEditor } from './MarkdownEditor';
import { PdfViewer } from './PdfViewer';
import { useEditor } from '../contexts/EditorContext';
import { BladeDispatcher } from '../services/blade';
import { BladeEvent, FileEvent } from '../types/blade';
import { ChangeActionBar } from './editor/ChangeActionBar';
import { EventNames, type ChangeAppliedPayload, type AllEditsAppliedPayload } from '../types/events';
import type { Change } from '../types/change';


interface EditorPanelProps {
    activeFile: string | null;
    highlightLines?: { startLine: number; endLine: number } | null;
    pendingEdit?: Change | null;
    onAcceptEdit?: (changeId: string) => void;
    onRejectEdit?: (changeId: string) => void;
    /** Total number of files with pending changes */
    totalPendingFiles?: number;
    /** Current file index (1-based) among files with pending changes */
    currentFileIndex?: number;
    /** Callback to navigate to next file with pending changes */
    onNextFile?: () => void;
    /** Callback to navigate to previous file with pending changes */
    onPrevFile?: () => void;
}

export const EditorPanel: React.FC<EditorPanelProps> = ({
    activeFile,
    highlightLines,
    pendingEdit,
    onAcceptEdit,
    onRejectEdit,
    totalPendingFiles = 1,
    currentFileIndex = 1,
    onNextFile,
    onPrevFile,
}) => {
    const [content, setContent] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [reloadTrigger, setReloadTrigger] = useState(0);
    const { setActiveFile } = useEditor();
    const editorRef = useRef<CodeEditorHandle>(null);
    const pendingNavigation = useRef<{ path: string, line: number, col: number } | null>(null);

    useEffect(() => {
        // Update editor context when active file changes
        setActiveFile(activeFile);
    }, [activeFile, setActiveFile]);

    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlistenEditApplied: (() => void) | undefined;
        let unlistenAllApplied: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenEditApplied = await listen<ChangeAppliedPayload>(EventNames.CHANGE_APPLIED, (event) => {
                if (event.payload.file_path === activeFile) {
                    console.log('[EDITOR] Change applied, reloading:', activeFile);
                    if (editorRef.current) editorRef.current.clearDiffs();
                    // Instead of trigger reload, we can just let the content update via FileEvent if backend emits it
                    // But for now, we'll keep the reload trigger to force a fresh read intent
                    setReloadTrigger(prev => prev + 1);
                }
            });

            unlistenAllApplied = await listen<AllEditsAppliedPayload>(EventNames.ALL_EDITS_APPLIED, (event) => {
                if (event.payload.file_paths.includes(activeFile || '')) {
                    console.log('[EDITOR] All edits applied, reloading:', activeFile);
                    if (editorRef.current) editorRef.current.clearDiffs();
                    setReloadTrigger(prev => prev + 1);
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenEditApplied) unlistenEditApplied();
            if (unlistenAllApplied) unlistenAllApplied();
        };
    }, [activeFile]);

    // File Content Listener (Blade Protocol)
    useEffect(() => {
        if (!activeFile) return;

        let unlistenSys: (() => void) | undefined;

        const setupSysListener = async () => {
            unlistenSys = await listen<BladeEvent>('sys-event', (event) => {
                const bladeEvent = event.payload;
                if (bladeEvent.type === 'File') {
                    const fileEvent = bladeEvent.payload;
                    if (fileEvent.type === 'Content' && fileEvent.payload.path === activeFile) {
                        console.log('[EDITOR] Received content for:', activeFile);
                        setContent(fileEvent.payload.data);
                        setLoading(false);
                        setError(null);
                    } else if (fileEvent.type === 'Written' && fileEvent.payload.path === activeFile) {
                        console.log('[EDITOR] Confirmed written:', activeFile);
                        // Optional: Show toast
                    }
                } else if (bladeEvent.type === 'System') {
                    const sysEvent = bladeEvent.payload;
                    if (sysEvent.type === 'IntentFailed') {
                        // We can't easily match intent ID here without tracking it, 
                        // but if we are loading and get an error referencing the file, we can assume.
                        if (loading) {
                            // Ideally check sysEvent.payload.error
                            const err = sysEvent.payload.error;
                            if ('details' in err && (err.details as any).id?.includes(activeFile)) {
                                setError(`Failed to load: ${JSON.stringify(err)}`);
                                setLoading(false);
                            }
                        }
                    }
                }
            });
        };

        setupSysListener();

        return () => {
            if (unlistenSys) unlistenSys();
        };
    }, [activeFile, loading]);

    useEffect(() => {
        async function loadFile() {
            if (!activeFile) {
                setContent('');
                return;
            }

            setLoading(true);
            setError(null);
            try {
                if (typeof window !== 'undefined') {
                    // Send Read Intent
                    await BladeDispatcher.file({
                        type: 'Read',
                        payload: { path: activeFile }
                    });
                    // Content will be set by the listener
                }
            } catch (e) {
                console.error(e);
                setError(String(e));
            } finally {
                setLoading(false);
            }
        }
        loadFile();
    }, [activeFile, reloadTrigger]);

    // Handle pending navigation after content load
    useEffect(() => {
        if (!loading && activeFile && pendingNavigation.current && pendingNavigation.current.path === activeFile) {
            setTimeout(() => {
                if (editorRef.current && pendingNavigation.current) {
                    const { line, col } = pendingNavigation.current;
                    // Convert 0-based line from backend to 1-based for editor
                    editorRef.current.setCursor(line + 1, col);
                    pendingNavigation.current = null;
                }
            }, 150);
        }
    }, [content, loading, activeFile]);

    // Handle save (Ctrl+S)
    const handleSave = async (text: string) => {
        if (activeFile) {
            try {
                await BladeDispatcher.file({
                    type: 'Write',
                    payload: { path: activeFile, content: text }
                });
                console.log("Save intent dispatched:", activeFile);
                // ToDo: Toast notification
            } catch (e) {
                console.error("Save failed:", e);
            }
        }
    };

    const handleNavigate = (path: string, line: number, character: number) => {
        console.log("Navigating to:", path, line, character);
        setActiveFile(path);
        pendingNavigation.current = { path, line, col: character };
    };

    if (!activeFile) {
        return (
            <div
                className="h-full flex flex-col items-center justify-center text-zinc-600 select-none bg-zinc-900/50"
                onContextMenu={(e) => e.preventDefault()}
            >
                <div className="text-4xl opacity-20 mb-4 font-thin">∅</div>
                <p className="font-mono text-xs uppercase tracking-widest opacity-50">No Active Buffer</p>
            </div>
        );
    }

    // Check file type
    const isMarkdownFile = activeFile.endsWith('.md') || activeFile.endsWith('.markdown');
    const isPdfFile = activeFile.endsWith('.pdf');

    return (
        <div className="h-full flex flex-col relative bg-[#1e1e1e]">
            {loading && !isPdfFile && (
                <div className="absolute inset-0 bg-black/50 z-10 flex items-center justify-center">
                    <div className="animate-spin w-5 h-5 border-2 border-emerald-500 border-t-transparent rounded-full" />
                </div>
            )}
            {error && !isPdfFile && (
                <div className="bg-red-900/50 text-red-200 p-2 text-xs font-mono">
                    ERR_LOAD: {error}
                </div>
            )}
            {isPdfFile ? (
                <PdfViewer filePath={activeFile} />
            ) : isMarkdownFile ? (
                <MarkdownEditor
                    content={content}
                    onChange={setContent}
                    onSave={handleSave}
                    filename={activeFile}
                />
            ) : (
                <CodeEditor
                    ref={editorRef}
                    content={content}
                    onChange={setContent}
                    onSave={handleSave}
                    filename={activeFile}
                    highlightLines={highlightLines || undefined}
                    onNavigate={handleNavigate}
                />
            )}
            {/* Non-invasive bottom action bar for pending changes */}
            {pendingEdit && pendingEdit.path === activeFile && onAcceptEdit && onRejectEdit && (
                <ChangeActionBar
                    currentFileIndex={currentFileIndex}
                    totalFiles={totalPendingFiles}
                    onAccept={async () => {
                        await onAcceptEdit(pendingEdit.id);
                        setTimeout(() => setReloadTrigger(prev => prev + 1), 100);
                    }}
                    onReject={() => {
                        onRejectEdit(pendingEdit.id);
                    }}
                    onNextFile={onNextFile}
                    onPrevFile={onPrevFile}
                    filename={pendingEdit.path.split('/').pop()}
                />
            )}
        </div>
    );
};
