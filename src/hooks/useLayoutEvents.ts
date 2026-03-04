import { useState, useEffect, useCallback, RefObject } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ChatMessage } from '../types/chat';
import type { Tab } from './useTabManager';

interface UseLayoutEventsOptions {
    setTabs: React.Dispatch<React.SetStateAction<Tab[]>>;
    setActiveTabId: React.Dispatch<React.SetStateAction<string | null>>;
    setAiEditedFilePaths: React.Dispatch<React.SetStateAction<Set<string>>>;
    setUnseenAiEditedFilePaths: React.Dispatch<React.SetStateAction<Set<string>>>;
    processingFilesRef: RefObject<Set<string>>;
    setConversation: React.Dispatch<React.SetStateAction<ChatMessage[]>>;
}

interface ResearchProgress {
    message: string;
    stage: string;
    percent: number;
    isActive: boolean;
}

const isTerminalResearchStage = (stage: string): boolean => {
    const normalized = stage.toLowerCase();
    return (
        normalized.includes('complete')
        || normalized.includes('done')
        || normalized.includes('error')
        || normalized.includes('fail')
        || normalized.includes('cancel')
        || normalized.includes('stop')
    );
};

export function useLayoutEvents({
    setTabs,
    setActiveTabId,
    setAiEditedFilePaths,
    setUnseenAiEditedFilePaths,
    processingFilesRef,
    setConversation,
}: UseLayoutEventsOptions) {
    const [researchProgress, setResearchProgress] = useState<ResearchProgress | null>(null);

    // Auto-open files when edit proposals arrive (without stealing focus)
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        const unlistenPromise = listen<{ id: string; path: string; old_content: string; new_content: string }[]>('propose-edit', (event) => {
            if (event.payload.length === 0) return;

            const editedPaths = event.payload.map(edit => edit.path);

            setAiEditedFilePaths(prev => {
                const next = new Set(prev);
                for (const path of editedPaths) next.add(path);
                return next;
            });

            setUnseenAiEditedFilePaths(prev => {
                const next = new Set(prev);
                for (const path of editedPaths) next.add(path);
                return next;
            });

            setTabs(prev => {
                let next = prev;

                for (const path of editedPaths) {
                    const existingTab = next.find(t => t.type === 'file' && t.path === path);
                    if (existingTab) continue;

                    const filename = path.split('/').pop() || path;
                    const newTab: Tab = {
                        id: `file-${path}`,
                        title: filename,
                        type: 'file',
                        path,
                    };

                    if (next === prev) {
                        next = [...prev, newTab];
                    } else {
                        next.push(newTab);
                    }
                }

                return next;
            });
        });

        return () => {
            unlistenPromise
                .then(unlisten => unlisten())
                .catch(console.error);
        };
    }, [setTabs, setAiEditedFilePaths, setUnseenAiEditedFilePaths]);

    // Listen for open-file, open-ephemeral-document, research-progress, change-applied events
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        const unlistenPromises: Promise<() => void>[] = [];

        const handleOpenFile = (path: string, sourceEvent: string) => {
            console.debug(`Opening file from backend (${sourceEvent}):`, path);
            const tabId = `file-${path}`;

            if (processingFilesRef.current.has(path)) {
                console.debug('[LAYOUT] Ignoring duplicate file open event for:', path);
                return;
            }
            processingFilesRef.current.add(path);

            setTabs(prev => {
                const existingTab = prev.find(t => t.type === 'file' && t.path === path);
                if (existingTab) {
                    processingFilesRef.current.delete(path);
                    return prev;
                }
                const filename = path.split('/').pop() || path;
                const newTab: Tab = {
                    id: tabId,
                    title: filename,
                    type: 'file',
                    path,
                };
                processingFilesRef.current.delete(path);
                return [...prev, newTab];
            });
            setActiveTabId(prev => prev ?? tabId);
        };

        // Current backend event name (Rust emits this)
        unlistenPromises.push(listen<string>('open-file', (event) => {
            handleOpenFile(event.payload, 'open-file');
        }));

        // Backwards-compatible alias (kept for older emitters)
        unlistenPromises.push(listen<string>('file-opened', (event) => {
            handleOpenFile(event.payload, 'file-opened');
        }));

        unlistenPromises.push(listen<{ path: string; start_line: number; end_line: number }>('open-file-with-highlight', (event) => {
            console.debug('Opening file with highlight from backend:', event.payload);
            const { path, start_line, end_line } = event.payload;
            const tabId = `file-${path}`;
            setTabs(prev => {
                const existingTab = prev.find(t => t.type === 'file' && t.path === path);
                if (existingTab) {
                    return prev.map(t =>
                        t.id === existingTab.id
                            ? { ...t, highlightLines: { startLine: start_line, endLine: end_line } }
                            : t
                    );
                }
                const filename = path.split('/').pop() || path;
                const newTab: Tab = {
                    id: tabId,
                    title: filename,
                    type: 'file',
                    path,
                    highlightLines: { startLine: start_line, endLine: end_line },
                };
                return [...prev, newTab];
            });
            setActiveTabId(prev => prev ?? tabId);
        }));

        unlistenPromises.push(listen<{ id: string; title: string; content: string; suggestedName: string }>('open-ephemeral-document', (event) => {
            console.debug('[LAYOUT] 📥 Received open-ephemeral-document event:', {
                id: event.payload.id,
                title: event.payload.title,
                contentLength: event.payload.content.length,
                suggestedName: event.payload.suggestedName
            });

            // Clear research progress when result arrives
            setResearchProgress(null);

            const { id, title, content, suggestedName } = event.payload;

            setTabs(prev => {
                const existingTab = prev.find(t => t.id === id);
                if (existingTab) {
                    console.debug('[LAYOUT] ⚠️ Tab already exists, just activating:', id);
                    return prev;
                }

                console.debug('[LAYOUT] ✅ Creating new tab with ID:', id);
                const newTab: Tab = {
                    id,
                    title,
                    type: 'ephemeral',
                    content,
                    suggestedName,
                };
                console.debug('[LAYOUT] Adding tab to existing tabs:', prev.length, '→', prev.length + 1);
                return [...prev, newTab];
            });
            setActiveTabId(id);
        }));

        // Listen for research progress events
        unlistenPromises.push(listen<{ message: string; stage: string; percent: number }>('research-progress', (event) => {
            console.debug('[LAYOUT] Research progress:', event.payload);

            setResearchProgress({
                ...event.payload,
                isActive: true
            });

            // Persist research activity in message history
            setConversation(prev => {
                const updated = [...prev];
                for (let i = updated.length - 1; i >= 0; i--) {
                    if (updated[i].role === 'Assistant') {
                        const msg = updated[i];
                        const existingActivities = msg.researchActivities || [];
                        const lastActivity = existingActivities.length > 0
                            ? existingActivities[existingActivities.length - 1]
                            : null;

                        const shouldUpdateLast = !!lastActivity && (
                            !isTerminalResearchStage(lastActivity.stage)
                            || (
                                lastActivity.stage === event.payload.stage
                                && lastActivity.message === event.payload.message
                            )
                        );

                        const activityId = shouldUpdateLast && lastActivity
                            ? lastActivity.id
                            : crypto.randomUUID();
                        const nextActivity = {
                            id: activityId,
                            message: event.payload.message,
                            stage: event.payload.stage,
                            percent: event.payload.percent,
                            timestamp: Date.now(),
                        };

                        const newActivities = shouldUpdateLast
                            ? [
                                ...existingActivities.slice(0, -1),
                                {
                                    ...lastActivity!,
                                    ...nextActivity,
                                }
                            ]
                            : [...existingActivities, nextActivity];

                        const newBlocks = [...(msg.blocks || [])];
                        if (!newBlocks.some(b => b.type === 'research_progress' && b.id === activityId)) {
                            newBlocks.push({ type: 'research_progress', id: activityId });
                        }

                        updated[i] = {
                            ...msg,
                            researchActivities: newActivities,
                            blocks: newBlocks
                        };
                        break;
                    }
                }
                return updated;
            });
        }));

        // Listen for chat errors to clear progress
        unlistenPromises.push(listen('chat-error', () => {
            setResearchProgress(null);
        }));

        // Listen for change-applied events to convert ephemeral tabs to file tabs
        unlistenPromises.push(listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
            console.debug('[LAYOUT] Change applied:', event.payload);
            const { change_id, file_path } = event.payload;

            const filename = file_path.split('/').pop() || file_path;

            processingFilesRef.current.add(file_path);

            setTabs(prev => {
                const ephemeralTab = prev.find(t =>
                    t.id === `new-file-${change_id}` ||
                    (t.type === 'ephemeral' && (
                        t.suggestedName === filename ||
                        t.title === filename ||
                        t.suggestedName?.includes(filename)
                    ))
                );

                if (!ephemeralTab) {
                    processingFilesRef.current.delete(file_path);
                    return prev;
                }

                console.debug('[LAYOUT] Found matching ephemeral tab, converting to file tab:', ephemeralTab.id, '→', file_path);
                const fileTab: Tab = {
                    id: `file-${file_path}`,
                    title: filename,
                    type: 'file',
                    path: file_path,
                };

                const newTabs = prev.filter(t => t.id !== ephemeralTab.id);
                return [...newTabs, fileTab];
            });

            setActiveTabId(prev => prev ?? `file-${file_path}`);

            setTimeout(() => {
                processingFilesRef.current.delete(file_path);
            }, 500);
        }));

        return () => {
            for (const unlistenPromise of unlistenPromises) {
                unlistenPromise
                    .then(unlisten => unlisten())
                    .catch(console.error);
            }
        };
    }, [setTabs, setActiveTabId, processingFilesRef, setConversation]);

    // Finalize research activities when chat stops
    const finalizeResearchActivities = useCallback((loading: boolean, wasStoppedByUser: boolean) => {
        if (!loading) {
            setResearchProgress(prev => prev ? { ...prev, isActive: false } : null);
            const finalStage = wasStoppedByUser ? 'STOPPED' : 'COMPLETE';

            setConversation(prev => {
                let changed = false;
                const next = prev.map(msg => {
                    const activities = msg.researchActivities;
                    if (!activities || activities.length === 0) return msg;

                    const updatedActivities = activities.map(activity => {
                        if (isTerminalResearchStage(activity.stage)) return activity;

                        changed = true;
                        return {
                            ...activity,
                            stage: finalStage,
                            percent: 100,
                        };
                    });

                    if (!changed) return msg;

                    return {
                        ...msg,
                        researchActivities: updatedActivities,
                    };
                });

                return changed ? next : prev;
            });
        }
    }, [setConversation]);

    return {
        researchProgress,
        setResearchProgress,
        finalizeResearchActivities,
    };
}
