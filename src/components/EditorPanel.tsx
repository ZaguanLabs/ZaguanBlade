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
import {
    createEditorContentSnapshot,
    getEditorContentStatePropagation,
    getEditorInitialContentConfig,
    type EditorContentSnapshot,
} from '../utils/editorBufferRegistry';
import { getEditorReviewTransition } from '../utils/editorReviewTransitions';
import type { UncommittedChangesUpdateReason } from '../utils/uncommittedChangeNotifications';

const CodeEditor = React.lazy(() => import('./CodeEditor'));
const PdfViewer = React.lazy(() =>
    import('./PdfViewer').then((module) => ({ default: module.PdfViewer }))
);

const getDirectoryPath = (path: string): string => {
    const normalized = path.replace(/\\/g, '/');
    const separatorIndex = normalized.lastIndexOf('/');
    return separatorIndex > 0 ? normalized.slice(0, separatorIndex) : normalized;
};

export type EditorContentState = EditorContentSnapshot;

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
                        alt={t('app.name')}
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
    onContentStateChange?: (state: EditorContentState) => void;
    onRegisterContentSnapshot?: (getSnapshot: (() => EditorContentState) | null) => void;
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
    onRegisterContentSnapshot,
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
    const pendingContentStateRef = useRef<EditorContentState | null>(null);
    const externalContentVersionRef = useRef(0);
    const documentVersionRef = useRef(0);
    const awaitingInitialSyncRef = useRef(false);
    const syncedDocumentPathRef = useRef<string | null>(null);
    const onContentStateChangeRef = useRef(onContentStateChange);
    const lastPropagatedDirtyRef = useRef(isDirty);
    const previousActiveFileRef = useRef(activeFile);
    const authoritativeReloadRef = useRef(false);
    const latestFileReadIntentIdRef = useRef<string | null>(null);

    const pathsMatch = (a: string, b: string): boolean => {
        if (a === b) return true;
        const normA = a.replace(/\\/g, '/');
        const normB = b.replace(/\\/g, '/');
        return normA.endsWith(normB) || normB.endsWith(normA);
    };

    useUncommittedChanges({
        onFileChanged: (filePath, reason: UncommittedChangesUpdateReason) => {
            if (!activeFile || !pathsMatch(filePath, activeFile)) {
                return;
            }

            awaitingInitialSyncRef.current = false;
            const reviewTransition = getEditorReviewTransition({
                filePath,
                reason,
                locallyDirty: lastPropagatedDirtyRef.current,
                currentContent: editorRef.current?.getContent() ?? liveContentRef.current,
            });

            if (reviewTransition.action === 'mark-clean') {
                liveContentRef.current = reviewTransition.contentState.savedContent ?? '';
                baseContentRef.current = reviewTransition.contentState.savedContent ?? '';
                editorRef.current?.replaceDocument({
                    path: filePath,
                    content: reviewTransition.contentState.savedContent ?? '',
                    resetHistory: false,
                    reason: 'external-clean-update',
                    unifiedDiff: undefined,
                    preserveScroll: true,
                    forceEffects: true,
                });
                if (contentStateTimerRef.current) {
                    clearTimeout(contentStateTimerRef.current);
                    contentStateTimerRef.current = null;
                }
                pendingContentStateRef.current = null;
                lastPropagatedDirtyRef.current = false;
                onContentStateChangeRef.current?.(reviewTransition.contentState);
                return;
            }

            if (reviewTransition.action === 'request-authoritative-reload') {
                authoritativeReloadRef.current = true;
            } else if (reviewTransition.action === 'ignore') {
                return;
            }
            setReloadTrigger(prev => prev + 1);
        },
    });

    useEffect(() => {
        onContentStateChangeRef.current = onContentStateChange;
    }, [onContentStateChange]);

    const getActiveEditorContent = useCallback(() => {
        return editorRef.current?.getContent() ?? liveContentRef.current;
    }, []);

    useEffect(() => {
        if (!onRegisterContentSnapshot) {
            return;
        }

        onRegisterContentSnapshot(() => {
            const currentContent = getActiveEditorContent();
            return createEditorContentSnapshot({
                filePath: activeFile ?? undefined,
                baselineContent: baseContentRef.current,
                currentContent,
            });
        });

        return () => {
            onRegisterContentSnapshot(null);
        };
    }, [activeFile, getActiveEditorContent, onRegisterContentSnapshot]);

    const isMarkdownFile = activeFile?.endsWith('.md') || activeFile?.endsWith('.markdown') || false;
    const isPdfFile = activeFile?.endsWith('.pdf') || false;

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

    const emitContentStateChange = useCallback((state: EditorContentState) => {
        const propagation = getEditorContentStatePropagation(state, lastPropagatedDirtyRef.current);

        if (propagation.immediate) {
            pendingContentStateRef.current = propagation.immediate;
            flushPendingContentState();
        }

        if (!propagation.debounced) {
            return;
        }

        pendingContentStateRef.current = propagation.debounced;

        if (contentStateTimerRef.current) {
            clearTimeout(contentStateTimerRef.current);
        }

        contentStateTimerRef.current = setTimeout(() => {
            flushPendingContentState();
        }, 120);
    }, [flushPendingContentState]);

    const applyContentToEditor = useCallback((
        nextContent: string,
        reason: 'open' | 'reload' | 'revert' | 'external-clean-update',
        resetHistory: boolean,
    ) => {
        const editorHandle = editorRef.current;
        if (!editorHandle || !activeFile) {
            externalContentVersionRef.current += 1;
            return;
        }

        editorHandle.replaceDocument({
            path: activeFile,
            content: nextContent,
            resetHistory,
            reason,
        });
    }, [activeFile]);

    useEffect(() => {
        const isFileSwitch = activeFile !== previousActiveFileRef.current;
        previousActiveFileRef.current = activeFile;

        if (!activeFile) {
            setContent('');
            baseContentRef.current = '';
            liveContentRef.current = '';
            externalContentVersionRef.current += 1;
            awaitingInitialSyncRef.current = false;
            authoritativeReloadRef.current = false;
            pendingContentStateRef.current = null;
            lastPropagatedDirtyRef.current = false;
            return;
        }

        const config = getEditorInitialContentConfig({
            activeFile,
            savedContent,
            draftContent,
            isDirty,
            isMarkdownFile,
            isFileSwitch,
        });

        setContent(config.content);
        baseContentRef.current = config.baselineContent;
        liveContentRef.current = config.content;
        awaitingInitialSyncRef.current = config.awaitingInitialSync;

        if (config.shouldLoad && config.reason) {
            applyContentToEditor(config.content, config.reason, config.resetHistory);
        }
    }, [activeFile, applyContentToEditor, draftContent, isDirty, savedContent, isMarkdownFile]);

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
                    if (
                        envelope.causality_id
                        && latestFileReadIntentIdRef.current
                        && envelope.causality_id !== latestFileReadIntentIdRef.current
                    ) {
                        return;
                    }

                    const isAuthoritativeReload = authoritativeReloadRef.current;
                    if (!isAuthoritativeReload && lastPropagatedDirtyRef.current && fileEvent.payload.data !== getActiveEditorContent()) {
                        console.debug('[EDITOR] Ignored stale content while local edits are dirty:', activeFile);
                        awaitingInitialSyncRef.current = false;
                        setLoading(false);
                        return;
                    }

                    console.debug('[EDITOR] Received content for:', activeFile);
                    latestFileReadIntentIdRef.current = null;
                    authoritativeReloadRef.current = false;
                    awaitingInitialSyncRef.current = false;
                    const editorHandle = editorRef.current;
                    editorHandle?.replaceDocument({
                        path: fileEvent.payload.path,
                        content: fileEvent.payload.data,
                        resetHistory: true,
                        reason: isAuthoritativeReload ? 'revert' : 'reload',
                    });
                    setContent(fileEvent.payload.data);
                    liveContentRef.current = fileEvent.payload.data;
                    baseContentRef.current = fileEvent.payload.data;
                    if (!editorHandle) {
                        externalContentVersionRef.current += 1;
                    }
                    emitContentStateChange(createEditorContentSnapshot({
                        filePath: fileEvent.payload.path,
                        baselineContent: fileEvent.payload.data,
                        currentContent: fileEvent.payload.data,
                    }));
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
    }, [activeFile, emitContentStateChange, getActiveEditorContent, loading, t]);

    useEffect(() => {
        async function loadFile() {
            if (!activeFile) {
                setContent('');
                liveContentRef.current = '';
                latestFileReadIntentIdRef.current = null;
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
                    const readIntentId = crypto.randomUUID();
                    latestFileReadIntentIdRef.current = readIntentId;
                    await BladeDispatcher.dispatch("File", {
                        type: "File",
                        payload: {
                            type: 'Read',
                            payload: { path: activeFile }
                        }
                    }, undefined, readIntentId);
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
                emitContentStateChange(createEditorContentSnapshot({
                    filePath: activeFile,
                    baselineContent: currentContent,
                    currentContent,
                }));
                console.debug("Save intent dispatched:", activeFile);
                // ToDo: Toast notification
            } catch (e) {
                console.error("Save failed:", e);
            }
        }
    };

    const handleEditorDocumentChange = () => {
        scheduleDocumentSync();
        const nextText = getActiveEditorContent();
        liveContentRef.current = nextText;
        emitContentStateChange(createEditorContentSnapshot({
            filePath: activeFile ?? undefined,
            baselineContent: baseContentRef.current,
            currentContent: nextText,
        }));
    };

    const handleNavigate = (path: string, line: number, character: number) => {
        console.debug("Navigating to:", path, line, character);
        setActiveFile(path);
        pendingNavigation.current = { path, line, col: character };
    };

    const immediateTabContent = isDirty && draftContent != null ? draftContent : savedContent;
    const hasActiveEditorContent = immediateTabContent != null;
    const activeEditorContent = immediateTabContent ?? content;

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
                ) : !hasActiveEditorContent ? (
                    <div className="h-full w-full bg-(--bg-editor)" />
                ) : isMarkdownFile ? (
                    <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                        <MarkdownEditor
                            key={activeFile}
                            ref={editorRef}
                            content={activeEditorContent}
                            externalContentVersion={externalContentVersionRef.current}
                            onDocumentChange={handleEditorDocumentChange}
                            onSave={handleSave}
                            filename={activeFile}
                        />
                    </Suspense>
                ) : (
                    <EditorWithChangeBar
                        editorRef={editorRef}
                        content={activeEditorContent}
                        externalContentVersion={externalContentVersionRef.current}
                        onDocumentChange={handleEditorDocumentChange}
                        handleSave={handleSave}
                        activeFile={activeFile}
                        highlightLines={highlightLines}
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
    externalContentVersion: number;
    onDocumentChange: () => void;
    handleSave: (text: string) => void;
    activeFile: string;
    highlightLines?: { startLine: number; endLine: number } | null;
    handleNavigate: (path: string, line: number, character: number) => void;
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
}: EditorWithChangeBarProps) {
    const { getChangeForFile, acceptFile, rejectFile } = useUncommittedChanges();
    const change = getChangeForFile(activeFile);

    const handleAccept = async () => {
        await acceptFile(activeFile);
    };

    const handleReject = async () => {
        await rejectFile(activeFile);
    };

    return (
        <div className="relative h-full w-full">
            <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                <CodeEditor
                    key={activeFile}
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
