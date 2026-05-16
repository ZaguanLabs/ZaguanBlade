'use client';
import React, { useState, useEffect, useRef, Suspense, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { watchImmediate } from '@tauri-apps/plugin-fs';
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
import { recordDebugPerf } from '../utils/debugPerf';

const CodeEditor = React.lazy(() => import('./CodeEditor'));
const PdfViewer = React.lazy(() =>
    import('./PdfViewer').then((module) => ({ default: module.PdfViewer }))
);

const getDirectoryPath = (path: string): string => {
    const normalized = path.replace(/\\/g, '/');
    const separatorIndex = normalized.lastIndexOf('/');
    return separatorIndex > 0 ? normalized.slice(0, separatorIndex) : normalized;
};

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
        <div className="h-full flex flex-col items-center justify-center bg-(--bg-editor) text-center p-8 animate-in fade-in duration-(--transition-slow)">
            <div className="max-w-xl w-full">
                <div className="mb-8 flex justify-center">
                    <img
                        src={zbladeLogoUrl}
                        alt="Zaguán Blade"
                        className="w-32 h-32 object-contain drop-shadow-lg"
                        draggable={false}
                    />
                </div>

                <h1 className="text-3xl font-bold text-(--fg-primary) mb-3 tracking-tight">
                    {t('editor.landing.title')}
                </h1>
                <p className="text-(--fg-secondary) text-lg mb-8 leading-relaxed">
                    {t('editor.landing.subtitle')}
                </p>

                <div className="grid gap-3 max-w-sm mx-auto">
                    {!isLoading && (
                        <>
                            <button
                                onClick={() => openSettingsSection('localai')}
                                className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-(--accent-mention) text-(--fg-bright) rounded-[calc(var(--panel-radius)*0.65)] font-medium transition-opacity shadow-(--shadow-md) hover:opacity-90"
                            >
                                <Server className="w-4 h-4" />
                                {t('editor.landing.useLocalAi')}
                            </button>

                            <button
                                onClick={() => openSettingsSection('account')}
                                className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-(--bg-surface) hover:bg-(--bg-surface-hover) border border-(--border-subtle) hover:border-(--border-focus) text-(--fg-primary) rounded-[calc(var(--panel-radius)*0.65)] font-medium transition-all"
                            >
                                <Cloud className="w-4 h-4" />
                                {t('editor.landing.useCloudModels')}
                            </button>

                            <a
                                href={hasApiKey ? "https://zaguanai.com/dashboard" : "https://zaguanai.com/pricing"}
                                target="_blank"
                                rel="noreferrer"
                                className="flex items-center justify-center gap-2 w-full py-2 px-4 rounded-[calc(var(--panel-radius)*0.55)] font-medium transition-all text-(--fg-secondary) hover:text-(--fg-primary)"
                            >
                                {t('editor.landing.manageSubscription')}
                                <ArrowRight className="w-4 h-4 opacity-70" />
                            </a>
                        </>
                    )}
                </div>

                <div className="mt-12 pt-8 border-t border-(--border-subtle)">
                    <p className="text-xs text-(--fg-tertiary)">
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
    recordDebugPerf('EditorPanel.render');
    const { t } = useTranslation();
    const [content, setContent] = useState(() => draftContent ?? savedContent ?? '');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [reloadTrigger, setReloadTrigger] = useState(0);
    const { setActiveFile } = useEditorActions();
    const editorRef = useRef<CodeEditorHandle>(null);
    const pendingNavigation = useRef<{ path: string, line: number, col: number } | null>(null);
    const baseContentRef = useRef(savedContent ?? '');
    const liveContentRef = useRef(draftContent ?? savedContent ?? '');
    const syncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const contentStateTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const externalFileReloadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const pendingContentStateRef = useRef<{
        savedContent?: string;
        draftContent?: string;
        isDirty: boolean;
    } | null>(null);
    const externalContentVersionRef = useRef(0);
    const documentVersionRef = useRef(0);
    const awaitingInitialSyncRef = useRef(false);
    const syncedDocumentPathRef = useRef<string | null>(null);
    const loadingRef = useRef(loading);
    const onContentStateChangeRef = useRef(onContentStateChange);
    const lastPropagatedDirtyRef = useRef(isDirty);
    const previousActiveFileRef = useRef(activeFile);

    const pathsMatch = (a: string, b: string): boolean => {
        if (a === b) return true;
        const normA = a.replace(/\\/g, '/');
        const normB = b.replace(/\\/g, '/');
        return normA.endsWith(normB) || normB.endsWith(normA);
    };

    useUncommittedChanges({
        onFileChanged: (filePath) => {
            if (!activeFile || !pathsMatch(filePath, activeFile)) {
                return;
            }

            awaitingInitialSyncRef.current = false;
            setReloadTrigger(prev => prev + 1);
        },
    });

    useEffect(() => {
        loadingRef.current = loading;
    }, [loading]);

    useEffect(() => {
        onContentStateChangeRef.current = onContentStateChange;
    }, [onContentStateChange]);

    const isMarkdownFile = activeFile?.endsWith('.md') || activeFile?.endsWith('.markdown') || false;
    const isPdfFile = activeFile?.endsWith('.pdf') || false;

    const getActiveEditorContent = useCallback(() => {
        return editorRef.current?.getContent() ?? liveContentRef.current;
    }, []);

    const scheduleDocumentSync = useCallback(() => {
        if (!activeFile || isPdfFile) {
            return;
        }

        if (awaitingInitialSyncRef.current) {
            return;
        }

        if (syncTimerRef.current) {
            clearTimeout(syncTimerRef.current);
        }

        syncTimerRef.current = setTimeout(() => {
            documentVersionRef.current += 1;
            syncedDocumentPathRef.current = activeFile;
            void EditorFacade.syncDocument(activeFile, getActiveEditorContent(), documentVersionRef.current);
            syncTimerRef.current = null;
        }, 180);
    }, [activeFile, getActiveEditorContent, isPdfFile]);

    const flushPendingContentState = useCallback(() => {
        if (contentStateTimerRef.current) {
            clearTimeout(contentStateTimerRef.current);
            contentStateTimerRef.current = null;
        }

        const pendingState = pendingContentStateRef.current;
        if (!pendingState) {
            return;
        }

        pendingContentStateRef.current = null;
        lastPropagatedDirtyRef.current = pendingState.isDirty;
        onContentStateChangeRef.current?.(pendingState);
    }, []);

    const emitContentStateChange = useCallback((state: {
        savedContent?: string;
        draftContent?: string;
        isDirty: boolean;
    }) => {
        const isFirstDirtyTransition = state.isDirty && !lastPropagatedDirtyRef.current;

        if (!state.isDirty) {
            pendingContentStateRef.current = state;
            flushPendingContentState();
            return;
        }

        if (isFirstDirtyTransition) {
            pendingContentStateRef.current = {
                savedContent: state.savedContent,
                draftContent: undefined,
                isDirty: true,
            };
            flushPendingContentState();
        }

        pendingContentStateRef.current = state;

        if (contentStateTimerRef.current) {
            clearTimeout(contentStateTimerRef.current);
        }

        contentStateTimerRef.current = setTimeout(() => {
            flushPendingContentState();
        }, 120);
    }, [flushPendingContentState]);

    useEffect(() => {
        const isFileSwitch = activeFile !== previousActiveFileRef.current;
        previousActiveFileRef.current = activeFile;

        if (!activeFile) {
            setContent('');
            baseContentRef.current = '';
            liveContentRef.current = '';
            externalContentVersionRef.current += 1;
            awaitingInitialSyncRef.current = false;
            pendingContentStateRef.current = null;
            lastPropagatedDirtyRef.current = false;
            return;
        }

        if (!isMarkdownFile) {
            if (savedContent != null) {
                baseContentRef.current = savedContent;
            }

            if (isDirty) {
                awaitingInitialSyncRef.current = false;
                return;
            }

            if (savedContent != null) {
                setContent(savedContent);
                liveContentRef.current = savedContent;
                externalContentVersionRef.current += 1;
                awaitingInitialSyncRef.current = false;
                return;
            }

            awaitingInitialSyncRef.current = true;
            return;
        }

        if (isDirty && draftContent != null) {
            setContent(draftContent);
            liveContentRef.current = draftContent;
            externalContentVersionRef.current += 1;
            if (savedContent != null) {
                baseContentRef.current = savedContent;
            }
            awaitingInitialSyncRef.current = false;
            return;
        }

        if (isDirty) {
            if (savedContent != null) {
                baseContentRef.current = savedContent;
            }

            if (isFileSwitch && savedContent != null) {
                setContent(savedContent);
                liveContentRef.current = savedContent;
                externalContentVersionRef.current += 1;
            }

            awaitingInitialSyncRef.current = false;
            return;
        }

        if (savedContent != null) {
            setContent(savedContent);
            liveContentRef.current = savedContent;
            baseContentRef.current = savedContent;
            externalContentVersionRef.current += 1;
            awaitingInitialSyncRef.current = false;
            return;
        }

        awaitingInitialSyncRef.current = true;
    }, [activeFile, draftContent, isDirty, savedContent, isMarkdownFile]);

    useEffect(() => {
        documentVersionRef.current = 0;

        return () => {
            if (syncTimerRef.current) {
                clearTimeout(syncTimerRef.current);
                syncTimerRef.current = null;
            }

            if (contentStateTimerRef.current) {
                clearTimeout(contentStateTimerRef.current);
                contentStateTimerRef.current = null;
            }
            pendingContentStateRef.current = null;

            if (activeFile && syncedDocumentPathRef.current === activeFile) {
                void EditorFacade.closeDocument(activeFile);
                syncedDocumentPathRef.current = null;
            }
        };
    }, [activeFile]);

    useEffect(() => {
        if (!activeFile || isPdfFile) {
            return;
        }

        const watchedFile = activeFile;
        const watchedDirectory = getDirectoryPath(watchedFile);
        let disposed = false;
        let unwatch: (() => void) | null = null;

        const scheduleExternalReload = () => {
            if (isDirty) {
                return;
            }

            if (externalFileReloadTimerRef.current) {
                clearTimeout(externalFileReloadTimerRef.current);
            }

            externalFileReloadTimerRef.current = setTimeout(() => {
                if (disposed || watchedFile !== activeFile) {
                    return;
                }

                awaitingInitialSyncRef.current = false;
                setReloadTrigger(prev => prev + 1);
                externalFileReloadTimerRef.current = null;
            }, 120);
        };

        watchImmediate(watchedDirectory, (event) => {
            if (disposed || !event.paths.some(path => pathsMatch(path, watchedFile))) {
                return;
            }

            scheduleExternalReload();
        }).then((dispose) => {
            if (disposed) {
                dispose();
                return;
            }

            unwatch = dispose;
        }).catch(error => {
            console.warn('[EDITOR] Failed to watch file for external changes:', watchedFile, error);
        });

        return () => {
            disposed = true;
            if (externalFileReloadTimerRef.current) {
                clearTimeout(externalFileReloadTimerRef.current);
                externalFileReloadTimerRef.current = null;
            }
            unwatch?.();
        };
    }, [activeFile, isDirty, isPdfFile]);

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
                    liveContentRef.current = fileEvent.payload.data;
                    baseContentRef.current = fileEvent.payload.data;
                    externalContentVersionRef.current += 1;
                    emitContentStateChange({
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
                    if (loadingRef.current) {
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
    }, [activeFile, emitContentStateChange, t]);

    useEffect(() => {
        async function loadFile() {
            if (!activeFile) {
                setContent('');
                liveContentRef.current = '';
                return;
            }

            if (isDirty && reloadTrigger === 0) {
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
    }, [activeFile, isDirty, reloadTrigger, t]);

    useEffect(() => {
        if (!activeFile || isPdfFile) {
            return;
        }

        if (awaitingInitialSyncRef.current) {
            return;
        }

        const immediateContent = isDirty && draftContent != null ? draftContent : savedContent;
        const syncSourceContent = getActiveEditorContent();
        if (documentVersionRef.current === 0 && immediateContent != null && syncSourceContent !== immediateContent) {
            return;
        }

        scheduleDocumentSync();

        return () => {
            if (syncTimerRef.current) {
                clearTimeout(syncTimerRef.current);
                syncTimerRef.current = null;
            }
        };
    }, [activeFile, draftContent, getActiveEditorContent, isDirty, savedContent, isPdfFile, scheduleDocumentSync]);

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
    }, [loading, activeFile]);

    // Handle save (Ctrl+S)
    const handleSave = async (text: string) => {
        if (activeFile) {
            const currentContent = editorRef.current?.getContent() ?? text;
            try {
                await BladeDispatcher.file({
                    type: 'Write',
                    payload: { path: activeFile, content: currentContent }
                });
                baseContentRef.current = currentContent;
                liveContentRef.current = currentContent;
                emitContentStateChange({
                    savedContent: currentContent,
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

    const handleEditorDocumentChange = () => {
        scheduleDocumentSync();

        if (!lastPropagatedDirtyRef.current) {
            pendingContentStateRef.current = {
                savedContent: baseContentRef.current,
                draftContent: undefined,
                isDirty: true,
            };
            flushPendingContentState();
        }

        if (contentStateTimerRef.current) {
            clearTimeout(contentStateTimerRef.current);
        }

        contentStateTimerRef.current = setTimeout(() => {
            contentStateTimerRef.current = null;
            const nextText = getActiveEditorContent();
            liveContentRef.current = nextText;
            const nextIsDirty = nextText !== baseContentRef.current;
            pendingContentStateRef.current = {
                savedContent: nextIsDirty ? baseContentRef.current : nextText,
                draftContent: nextIsDirty ? nextText : undefined,
                isDirty: nextIsDirty,
            };
            flushPendingContentState();
        }, 120);
    };

    const handleNavigate = (path: string, line: number, character: number) => {
        console.debug("Navigating to:", path, line, character);
        setActiveFile(path);
        pendingNavigation.current = { path, line, col: character };
    };

    if (!activeFile) {
        return <WelcomePage hasRemoteApiKey={hasRemoteApiKey} onOpenSettings={onOpenSettings} />;
    }

    return (
        <div className="h-full flex flex-col relative bg-(--bg-app)">
            {activeFile && !isPdfFile && (
                <Breadcrumb filePath={activeFile} workspaceRoot={workspaceRoot || undefined} />
            )}
            {loading && !isPdfFile && (
                <div className="absolute inset-0 bg-[color-mix(in_srgb,var(--bg-app)_72%,transparent)] z-10 flex items-center justify-center">
                    <div className="animate-spin w-5 h-5 border-2 border-(--accent-mention) border-t-transparent rounded-full" />
                </div>
            )}
            {error && !isPdfFile && (
                <div className="bg-[color-mix(in_srgb,var(--state-danger)_16%,var(--bg-app))] text-(--state-danger) p-2 text-xs font-mono">
                    {error}
                </div>
            )}
            <div className="flex-1 min-h-0 relative">
                {isPdfFile ? (
                    <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                        <PdfViewer filePath={activeFile} />
                    </Suspense>
                ) : isMarkdownFile ? (
                    <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                        <MarkdownEditor
                            ref={editorRef}
                            content={content}
                            onDocumentChange={handleEditorDocumentChange}
                            onSave={handleSave}
                            filename={activeFile}
                        />
                    </Suspense>
                ) : (
                    <EditorWithChangeBar
                        editorRef={editorRef}
                        content={content}
                        externalContentVersion={externalContentVersionRef.current}
                        onDocumentChange={handleEditorDocumentChange}
                        handleSave={handleSave}
                        activeFile={activeFile}
                        highlightLines={highlightLines}
                        handleNavigate={handleNavigate}
                        onFileContentChanged={() => {
                            awaitingInitialSyncRef.current = false;
                            setReloadTrigger(prev => prev + 1);
                        }}
                    />
                )}
            </div>
        </div>
    );
};

interface EditorWithChangeBarProps {
    editorRef: React.RefObject<CodeEditorHandle | null>;
    content: string;
    externalContentVersion: number;
    onDocumentChange: () => void;
    handleSave: (text: string) => void;
    activeFile: string;
    highlightLines?: { startLine: number; endLine: number } | null;
    handleNavigate: (path: string, line: number, character: number) => void;
    onFileContentChanged: (filePath: string) => void;
}

function EditorWithChangeBar({
    editorRef,
    content,
    externalContentVersion,
    onDocumentChange,
    handleSave,
    activeFile,
    highlightLines,
    handleNavigate,
    onFileContentChanged,
}: EditorWithChangeBarProps) {
    const { getChangeForFile, acceptFile, rejectFile, refresh } = useUncommittedChanges({
        onFileChanged: onFileContentChanged,
    });
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
            <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                <CodeEditor
                    ref={editorRef}
                    content={content}
                    externalContentVersion={externalContentVersion}
                    onDocumentChange={onDocumentChange}
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
}
