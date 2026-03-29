'use client';
import React, { useState, useEffect, useRef, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
const MarkdownEditor = React.lazy(() =>
    import('./MarkdownEditor').then((module) => ({ default: module.MarkdownEditor }))
);
import type { CodeEditorHandle } from './CodeEditor';
import { useEditorActions } from '../contexts/EditorContext';
import { BladeDispatcher } from '../services/blade';
import { subscribeBladeEvents } from '../services/bladeEvents';
import { EditorFacade } from '../services/editorFacade';
import { FileEvent } from '../types/blade';
import { ArrowRight, Server, Cloud } from 'lucide-react';
import zbladeLogoUrl from '../assets/zblade-in-app-logo.png';
import { FileChangeBar } from './editor/FileChangeBar';
import { Breadcrumb } from './editor/Breadcrumb';
import { useUncommittedChanges } from '../hooks/useUncommittedChanges';
import { formatBladeError, formatUnknownBackendError } from '../utils/backendErrors';

const CodeEditor = React.lazy(() => import('./CodeEditor'));
const PdfViewer = React.lazy(() =>
    import('./PdfViewer').then((module) => ({ default: module.PdfViewer }))
);

const WelcomePage: React.FC<{
    hasRemoteApiKey?: boolean | null;
    onOpenSettings?: () => void;
}> = ({ hasRemoteApiKey = null, onOpenSettings }) => {
    const { t } = useTranslation();
    const [hasApiKey, setHasApiKey] = useState<boolean>(false);
    const [isLoading, setIsLoading] = useState(hasRemoteApiKey === null);

    const openSettingsSection = (section?: 'account' | 'localai') => {
        if (onOpenSettings) {
            document.dispatchEvent(new CustomEvent('open-settings', { detail: { section } }));
            return;
        }
        document.dispatchEvent(new CustomEvent('open-settings', { detail: { section } }));
    };

    useEffect(() => {
        if (hasRemoteApiKey !== null) {
            setHasApiKey(hasRemoteApiKey);
            setIsLoading(false);
            return;
        }

        // Check for API key on mount
        const checkApiKey = async () => {
            if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
                setIsLoading(false);
                return;
            }

            try {
                const settings = await invoke<{ api_key: string }>('get_remote_ai_settings');
                setHasApiKey(!!settings.api_key && settings.api_key.length > 0);
            } catch (e) {
                console.warn('Failed to check API key status:', e);
            } finally {
                setIsLoading(false);
            }
        };

        checkApiKey();

        // Listen for settings changes to update immediately
        const unlistenPromise = listen('remote-settings-changed', checkApiKey);
        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [hasRemoteApiKey]);

    return (
        <div className="h-full flex flex-col items-center justify-center bg-[var(--bg-editor)] text-center p-8 animate-in fade-in duration-300">
            <div className="max-w-xl w-full">
                <div className="mb-8 flex justify-center">
                    <img
                        src={zbladeLogoUrl}
                        alt="Zaguán Blade"
                        className="w-32 h-32 object-contain drop-shadow-lg"
                        draggable={false}
                    />
                </div>

                <h1 className="text-3xl font-bold text-[var(--fg-primary)] mb-3 tracking-tight">
                    {t('editor.landing.title')}
                </h1>
                <p className="text-[var(--fg-secondary)] text-lg mb-8 leading-relaxed">
                    {t('editor.landing.subtitle')}
                </p>

                <div className="grid gap-3 max-w-sm mx-auto">
                    {!isLoading && (
                        <>
                            <button
                                onClick={() => openSettingsSection('localai')}
                                className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg font-medium transition-colors shadow-lg shadow-emerald-900/20"
                            >
                                <Server className="w-4 h-4" />
                                {t('editor.landing.useLocalAi')}
                            </button>

                            <button
                                onClick={() => openSettingsSection('account')}
                                className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-[var(--bg-surface)] hover:bg-[var(--bg-surface-hover)] border border-[var(--border-subtle)] hover:border-[var(--border-focus)] text-[var(--fg-primary)] rounded-lg font-medium transition-all"
                            >
                                <Cloud className="w-4 h-4" />
                                {t('editor.landing.useCloudModels')}
                            </button>

                            <a
                                href={hasApiKey ? "https://zaguanai.com/dashboard" : "https://zaguanai.com/pricing"}
                                target="_blank"
                                rel="noreferrer"
                                className="flex items-center justify-center gap-2 w-full py-2 px-4 rounded-lg font-medium transition-all text-[var(--fg-secondary)] hover:text-[var(--fg-primary)]"
                            >
                                {t('editor.landing.manageSubscription')}
                                <ArrowRight className="w-4 h-4 opacity-70" />
                            </a>
                        </>
                    )}
                </div>

                <div className="mt-12 pt-8 border-t border-[var(--border-subtle)]">
                    <p className="text-xs text-[var(--fg-tertiary)]">
                        {t('editor.landing.noApiKey')}
                        <br />
                        {t('editor.landing.localPrivacy')}
                    </p>
                </div>
            </div>
        </div>
    );
};


interface EditorPanelProps {
    activeFile: string | null;
    highlightLines?: { startLine: number; endLine: number } | null;
    workspaceRoot?: string | null;
    hasRemoteApiKey?: boolean | null;
    savedContent?: string | null;
    draftContent?: string | null;
    isDirty?: boolean;
    onContentStateChange?: (state: {
        savedContent?: string;
        draftContent?: string;
        isDirty: boolean;
    }) => void;
    onOpenSettings?: () => void;
}

export const EditorPanel: React.FC<EditorPanelProps> = ({
    activeFile,
    highlightLines,
    workspaceRoot,
    hasRemoteApiKey,
    savedContent,
    draftContent,
    isDirty = false,
    onContentStateChange,
    onOpenSettings,
}) => {
    const { t } = useTranslation();
    const [content, setContent] = useState(() => draftContent ?? savedContent ?? '');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [reloadTrigger, setReloadTrigger] = useState(0);
    const { setActiveFile } = useEditorActions();
    const editorRef = useRef<CodeEditorHandle>(null);
    const pendingNavigation = useRef<{ path: string, line: number, col: number } | null>(null);
    const baseContentRef = useRef(savedContent ?? '');
    const syncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const documentVersionRef = useRef(0);
    const awaitingInitialSyncRef = useRef(false);
    const syncedDocumentPathRef = useRef<string | null>(null);

    const pathsMatch = (a: string, b: string): boolean => {
        if (a === b) return true;
        const normA = a.replace(/\\/g, '/');
        const normB = b.replace(/\\/g, '/');
        return normA.endsWith(normB) || normB.endsWith(normA);
    };

    // useEffect(() => {
    //     // Update editor context when active file changes
    //     setActiveFile(activeFile);
    // }, [activeFile, setActiveFile]);

    useEffect(() => {
        if (!activeFile) {
            setContent('');
            baseContentRef.current = '';
            awaitingInitialSyncRef.current = false;
            return;
        }

        if (isDirty && draftContent != null) {
            setContent(draftContent);
            if (savedContent != null) {
                baseContentRef.current = savedContent;
            }
            awaitingInitialSyncRef.current = false;
            return;
        }

        if (savedContent != null) {
            setContent(savedContent);
            baseContentRef.current = savedContent;
            awaitingInitialSyncRef.current = false;
            return;
        }

        awaitingInitialSyncRef.current = true;
    }, [activeFile, draftContent, isDirty, savedContent]);

    useEffect(() => {
        documentVersionRef.current = 0;

        return () => {
            if (syncTimerRef.current) {
                clearTimeout(syncTimerRef.current);
                syncTimerRef.current = null;
            }

            if (activeFile && syncedDocumentPathRef.current === activeFile) {
                void EditorFacade.closeDocument(activeFile);
                syncedDocumentPathRef.current = null;
            }
        };
    }, [activeFile]);

    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        const unlistenFileChangesPromise = listen<{ count: number, paths: string[] }>('file-changes-detected', (event) => {
                // If the active file is in the changed paths, reload it
                if (activeFile && event.payload.paths.some(p => pathsMatch(p, activeFile))) {
                    console.debug('[EDITOR] File changed on disk, reloading:', activeFile);
                    setReloadTrigger(prev => prev + 1);
                }
            });

            // Also listen for change-applied events from tool edits (apply_patch, edit_file, etc.)
            // The fs_watcher has a 250ms debounce that can drop events during rapid multi-edit sequences,
            // so this provides a reliable, direct notification when a tool modifies a file.
            const unlistenChangeAppliedPromise = listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
                if (activeFile && pathsMatch(event.payload.file_path, activeFile)) {
                    console.debug('[EDITOR] Tool change applied to active file, reloading:', activeFile);
                    setReloadTrigger(prev => prev + 1);
                }
            });

        return () => {
            unlistenFileChangesPromise
                .then(unlisten => unlisten())
                .catch(console.error);
            unlistenChangeAppliedPromise
                .then(unlisten => unlisten())
                .catch(console.error);
        };
    }, [activeFile]);

    // File Content Listener (Blade Protocol)
    useEffect(() => {
        if (!activeFile) return;

        const unsubscribe = subscribeBladeEvents((envelope) => {
            const bladeEvent = envelope.event;
            if (bladeEvent.type === 'File') {
                const fileEvent = bladeEvent.payload as FileEvent;
                if (fileEvent.type === 'Content' && pathsMatch(fileEvent.payload.path, activeFile)) {
                    console.debug('[EDITOR] Received content for:', activeFile);
                    awaitingInitialSyncRef.current = false;
                    setContent(fileEvent.payload.data);
                    baseContentRef.current = fileEvent.payload.data;
                    onContentStateChange?.({
                        savedContent: fileEvent.payload.data,
                        draftContent: undefined,
                        isDirty: false,
                    });
                    setLoading(false);
                    setError(null);
                } else if (fileEvent.type === 'Written' && pathsMatch(fileEvent.payload.path, activeFile)) {
                    console.debug('[EDITOR] Confirmed written:', activeFile);
                }
            } else if (bladeEvent.type === 'System') {
                const sysEvent = bladeEvent.payload;
                if (sysEvent.type === 'IntentFailed') {
                    if (loading) {
                        const err = sysEvent.payload.error;
                        if ('details' in err && (err.details as any).id?.includes(activeFile)) {
                            setError(`${t('editor.loadFailed')}: ${formatBladeError(err)}`);
                            setLoading(false);
                        }
                    }
                }
            }
        });

        return () => {
            unsubscribe();
        };
    }, [activeFile, loading, onContentStateChange]);

    useEffect(() => {
        async function loadFile() {
            if (!activeFile) {
                setContent('');
                return;
            }

            if (isDirty && draftContent != null && reloadTrigger === 0) {
                setLoading(false);
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
                setError(`${t('editor.loadFailed')}: ${formatUnknownBackendError(e)}`);
            } finally {
                setLoading(false);
            }
        }
        loadFile();
    }, [activeFile, draftContent, isDirty, reloadTrigger]);

    useEffect(() => {
        if (!activeFile || activeFile.endsWith('.pdf')) {
            return;
        }

        if (awaitingInitialSyncRef.current) {
            return;
        }

        const immediateContent = isDirty && draftContent != null ? draftContent : savedContent;
        if (documentVersionRef.current === 0 && immediateContent != null && content !== immediateContent) {
            return;
        }

        if (syncTimerRef.current) {
            clearTimeout(syncTimerRef.current);
        }

        syncTimerRef.current = setTimeout(() => {
            documentVersionRef.current += 1;
            syncedDocumentPathRef.current = activeFile;
            void EditorFacade.syncDocument(activeFile, content, documentVersionRef.current);
            syncTimerRef.current = null;
        }, 180);

        return () => {
            if (syncTimerRef.current) {
                clearTimeout(syncTimerRef.current);
                syncTimerRef.current = null;
            }
        };
    }, [activeFile, content, draftContent, isDirty, savedContent]);

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
                baseContentRef.current = text;
                onContentStateChange?.({
                    savedContent: text,
                    draftContent: undefined,
                    isDirty: false,
                });
                console.debug("Save intent dispatched:", activeFile);
                // ToDo: Toast notification
            } catch (e) {
                console.error("Save failed:", e);
            }
        }
    };

    const handleContentChange = (nextContent: string) => {
        setContent(nextContent);

        const nextIsDirty = nextContent !== baseContentRef.current;
        onContentStateChange?.({
            savedContent: nextIsDirty ? baseContentRef.current : nextContent,
            draftContent: nextIsDirty ? nextContent : undefined,
            isDirty: nextIsDirty,
        });
    };

    const handleNavigate = (path: string, line: number, character: number) => {
        console.debug("Navigating to:", path, line, character);
        setActiveFile(path);
        pendingNavigation.current = { path, line, col: character };
    };

    if (!activeFile) {
        return <WelcomePage hasRemoteApiKey={hasRemoteApiKey} onOpenSettings={onOpenSettings} />;
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
            <div className="flex-1 min-h-0 relative w-full">
                {isPdfFile ? (
                    <Suspense fallback={<div className="h-full w-full bg-[var(--bg-app)]" />}>
                        <PdfViewer filePath={activeFile} />
                    </Suspense>
                ) : isMarkdownFile ? (
                    <Suspense fallback={<div className="h-full w-full bg-[var(--bg-editor)]" />}>
                        <MarkdownEditor
                            content={content}
                            onChange={handleContentChange}
                            onSave={handleSave}
                            filename={activeFile}
                        />
                    </Suspense>
                ) : (
                    <EditorWithChangeBar
                        editorRef={editorRef}
                        content={content}
                        setContent={handleContentChange}
                        handleSave={handleSave}
                        activeFile={activeFile}
                        highlightLines={highlightLines ?? null}
                        handleNavigate={handleNavigate}
                    />
                )}
            </div>

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
        <div className="relative h-full w-full">
            <Suspense fallback={<div className="h-full w-full bg-[var(--bg-editor)]" />}>
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
            </Suspense>
            {change && (
                <FileChangeBar
                    change={change}
                    onAccept={handleAccept}
                    onReject={handleReject}
                />
            )}
        </div>
    );
};
