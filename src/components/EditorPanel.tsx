'use client';
import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import CodeEditor, { type CodeEditorHandle } from './CodeEditor';
import { useEditor } from '../contexts/EditorContext';
import { EditorDiffOverlay } from './editor/EditorDiffOverlay';
import { EventNames, type ChangeAppliedPayload, type AllEditsAppliedPayload } from '../types/events';
import type { Change } from '../types/change';

interface EditorPanelProps {
    activeFile: string | null;
    highlightLines?: { startLine: number; endLine: number } | null;
    pendingEdit?: Change | null;
    onAcceptEdit?: (changeId: string) => void;
    onRejectEdit?: (changeId: string) => void;
}

export const EditorPanel: React.FC<EditorPanelProps> = ({ activeFile, highlightLines, pendingEdit, onAcceptEdit, onRejectEdit }) => {
    const [content, setContent] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [reloadTrigger, setReloadTrigger] = useState(0);
    const { setActiveFile } = useEditor();
    const editorRef = useRef<CodeEditorHandle>(null);

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
                    // Clear any diff widgets in CodeMirror
                    if (editorRef.current) {
                        editorRef.current.clearDiffs();
                    }
                    setReloadTrigger(prev => prev + 1);
                }
            });

            unlistenAllApplied = await listen<AllEditsAppliedPayload>(EventNames.ALL_EDITS_APPLIED, (event) => {
                if (event.payload.file_paths.includes(activeFile || '')) {
                    console.log('[EDITOR] All edits applied, reloading:', activeFile);
                    // Clear any diff widgets in CodeMirror
                    if (editorRef.current) {
                        editorRef.current.clearDiffs();
                    }
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
                    const text = await invoke<string>('read_file_content', { path: activeFile });
                    setContent(text);
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

    // Handle save (Ctrl+S)
    const handleSave = async (text: string) => {
        if (activeFile) {
            try {
                await invoke('write_file_content', { path: activeFile, content: text });
                console.log("Saved:", activeFile);
                // ToDo: Toast notification
            } catch (e) {
                console.error("Save failed:", e);
            }
        }
    };

    if (!activeFile) {
        return (
            <div className="h-full flex flex-col items-center justify-center text-zinc-600 select-none bg-zinc-900/50">
                <div className="text-4xl opacity-20 mb-4 font-thin">∅</div>
                <p className="font-mono text-xs uppercase tracking-widest opacity-50">No Active Buffer</p>
            </div>
        );
    }

    return (
        <div className="h-full flex flex-col relative bg-[#1e1e1e]">
            {loading && (
                <div className="absolute inset-0 bg-black/50 z-10 flex items-center justify-center">
                    <div className="animate-spin w-5 h-5 border-2 border-emerald-500 border-t-transparent rounded-full" />
                </div>
            )}
            {error && (
                <div className="bg-red-900/50 text-red-200 p-2 text-xs font-mono">
                    ERR_LOAD: {error}
                </div>
            )}
            <CodeEditor
                ref={editorRef}
                content={content}
                onChange={setContent}
                onSave={handleSave}
                filename={activeFile}
                highlightLines={highlightLines || undefined}
            />
            {pendingEdit && pendingEdit.path === activeFile && onAcceptEdit && onRejectEdit && (
                <EditorDiffOverlay
                    change={pendingEdit}
                    onAccept={async () => {
                        await onAcceptEdit(pendingEdit.id);
                        setTimeout(() => setReloadTrigger(prev => prev + 1), 100);
                    }}
                    onReject={() => {
                        onRejectEdit(pendingEdit.id);
                    }}
                />
            )}
        </div>
    );
};
