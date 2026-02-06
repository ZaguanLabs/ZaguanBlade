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
import { ArrowRight, Settings } from 'lucide-react';
import { FileChangeBar } from './editor/FileChangeBar';
import { Breadcrumb } from './editor/Breadcrumb';
import { useUncommittedChanges } from '../hooks/useUncommittedChanges';

const WelcomePage: React.FC<{ onOpenSettings?: () => void }> = ({ onOpenSettings }) => {
    const [hasApiKey, setHasApiKey] = useState<boolean>(false);
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        // Check for API key on mount
        const checkApiKey = async () => {
            if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
                setIsLoading(false);
                return;
            }

            try {
                // We import ApiConfig type dynamically or use 'any' if not strictly needed here, 
                // but let's try to infer from response
                const settings = await invoke<{ api_key: string }>('get_global_settings');
                setHasApiKey(!!settings.api_key && settings.api_key.length > 0);
            } catch (e) {
                console.warn('Failed to check API key status:', e);
            } finally {
                setIsLoading(false);
            }
        };

        checkApiKey();

        // Listen for settings changes to update immediately
        const unlistenPromise = listen('global-settings-changed', checkApiKey);
        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, []);

    return (
        <div className="h-full flex flex-col items-center justify-center bg-[var(--bg-editor)] text-center p-8 animate-in fade-in duration-300">
            <div className="max-w-xl w-full">
                <div className="mb-8 flex justify-center">
                    <div className="relative w-24 h-24 rounded-2xl bg-gradient-to-br from-emerald-500/20 to-sky-500/20 flex items-center justify-center border border-[var(--border-default)] shadow-xl shadow-emerald-500/10">
                        <div className="absolute inset-0 rounded-2xl bg-gradient-to-br from-emerald-500/10 to-transparent blur-xl"></div>
                        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" className="text-emerald-500 relative z-10">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M12 3v18M3 12h18M5 5l14 14M5 19L19 5" />
                        </svg>
                    </div>
                </div>

                <h1 className="text-3xl font-bold text-[var(--fg-primary)] mb-3 tracking-tight">
                    Zaguán Blade
                </h1>
                <p className="text-[var(--fg-secondary)] text-lg mb-8 leading-relaxed">
                    The AI-Native Code Editor for the future of development.
                </p>

                <div className="grid gap-4 max-w-sm mx-auto">
                    {!isLoading && (
                        <>
                            {!hasApiKey && (
                                <button
                                    onClick={onOpenSettings}
                                    className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg font-medium transition-colors shadow-lg shadow-emerald-900/20"
                                >
                                    <Settings className="w-4 h-4" />
                                    Configure API Key
                                </button>
                            )}

                            <a
                                href={hasApiKey ? "https://zaguanai.com/dashboard" : "https://zaguanai.com/pricing"}
                                target="_blank"
                                rel="noreferrer"
                                className={`flex items-center justify-center gap-2 w-full py-3 px-4 rounded-lg font-medium transition-all ${hasApiKey
                                        ? "bg-emerald-600 hover:bg-emerald-500 text-white shadow-lg shadow-emerald-900/20"
                                        : "bg-[var(--bg-surface)] hover:bg-[var(--bg-surface-hover)] border border-[var(--border-subtle)] hover:border-[var(--border-focus)] text-[var(--fg-primary)]"
                                    }`}
                            >
                                {hasApiKey ? "Manage your Subscription" : "Get Subscription"}
                                <ArrowRight className={`w-4 h-4 ${hasApiKey ? "" : "opacity-50"}`} />
                            </a>
                        </>
                    )}
                </div>

                <div className="mt-12 pt-8 border-t border-[var(--border-subtle)]">
                    <p className="text-xs text-[var(--fg-tertiary)]">
                        {hasApiKey
                            ? "AI features are ready to use."
                            : "To use AI features, you need an active Zaguán Blade subscription and valid API Key."
                        }
                        <br />
                        Code is processed securely according to our privacy policy.
                    </p>
                </div>
            </div>
        </div>
    );
};


interface EditorPanelProps {
    activeFile: string | null;
    highlightLines?: { startLine: number; endLine: number } | null;
    onOpenSettings?: () => void;
}

export const EditorPanel: React.FC<EditorPanelProps> = ({
    activeFile,
    highlightLines,
    onOpenSettings,
}) => {
    const [content, setContent] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [reloadTrigger, setReloadTrigger] = useState(0);
    const [workspaceRoot, setWorkspaceRoot] = useState<string | null>(null);
    const { setActiveFile } = useEditor();
    const editorRef = useRef<CodeEditorHandle>(null);
    const pendingNavigation = useRef<{ path: string, line: number, col: number } | null>(null);

    // useEffect(() => {
    //     // Update editor context when active file changes
    //     setActiveFile(activeFile);
    // }, [activeFile, setActiveFile]);

    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlistenFileChanges: (() => void) | undefined;
        let unlistenChangeApplied: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenFileChanges = await listen<{ count: number, paths: string[] }>('file-changes-detected', (event) => {
                // If the active file is in the changed paths, reload it
                if (activeFile && event.payload.paths.some(p => p === activeFile)) {
                    console.log('[EDITOR] File changed on disk, reloading:', activeFile);
                    setReloadTrigger(prev => prev + 1);
                }
            });

            // Also listen for change-applied events from tool edits (apply_patch, edit_file, etc.)
            // The fs_watcher has a 250ms debounce that can drop events during rapid multi-edit sequences,
            // so this provides a reliable, direct notification when a tool modifies a file.
            unlistenChangeApplied = await listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
                if (activeFile && event.payload.file_path === activeFile) {
                    console.log('[EDITOR] Tool change applied to active file, reloading:', activeFile);
                    setReloadTrigger(prev => prev + 1);
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenFileChanges) unlistenFileChanges();
            if (unlistenChangeApplied) unlistenChangeApplied();
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

    // Get workspace root on mount
    useEffect(() => {
        const getWorkspace = async () => {
            try {
                const root = await invoke<string | null>('get_current_workspace');
                setWorkspaceRoot(root);
            } catch (e) {
                console.error('Failed to get workspace root:', e);
            }
        };
        getWorkspace();
    }, []);

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
        return <WelcomePage onOpenSettings={onOpenSettings} />;
    }

    // Check file type
    const isMarkdownFile = activeFile.endsWith('.md') || activeFile.endsWith('.markdown');
    const isPdfFile = activeFile.endsWith('.pdf');

    return (
        <div className="h-full flex flex-col relative bg-[var(--bg-app)]">
            {activeFile && !isPdfFile && (
                <Breadcrumb filePath={activeFile} workspaceRoot={workspaceRoot || undefined} />
            )}
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
                <EditorWithChangeBar
                    editorRef={editorRef}
                    content={content}
                    setContent={setContent}
                    handleSave={handleSave}
                    activeFile={activeFile}
                    highlightLines={highlightLines ?? null}
                    handleNavigate={handleNavigate}
                />
            )}

        </div>
    );
};

interface EditorWithChangeBarProps {
    editorRef: React.RefObject<CodeEditorHandle | null>;
    content: string;
    setContent: (content: string) => void;
    handleSave: (text: string) => void;
    activeFile: string;
    highlightLines: { startLine: number; endLine: number } | null;
    handleNavigate: (path: string, line: number, character: number) => void;
}

const EditorWithChangeBar: React.FC<EditorWithChangeBarProps> = ({
    editorRef,
    content,
    setContent,
    handleSave,
    activeFile,
    highlightLines,
    handleNavigate,
}) => {
    const { getChangeForFile, acceptFile, rejectFile, refresh } = useUncommittedChanges();
    const change = getChangeForFile(activeFile);

    const handleAccept = async () => {
        await acceptFile(activeFile);
    };

    const handleReject = async () => {
        await rejectFile(activeFile);
        // Reload file content after revert
        setTimeout(() => {
            refresh();
        }, 100);
    };

    return (
        <div className="flex flex-col h-full">
            {change && (
                <FileChangeBar
                    change={change}
                    onAccept={handleAccept}
                    onReject={handleReject}
                />
            )}
            <div className="flex-1 min-h-0">
                <CodeEditor
                    ref={editorRef}
                    content={content}
                    onChange={setContent}
                    onSave={handleSave}
                    filename={activeFile}
                    highlightLines={highlightLines || undefined}
                    onNavigate={handleNavigate}
                    unifiedDiff={change?.unified_diff}
                />
            </div>
        </div>
    );
};
