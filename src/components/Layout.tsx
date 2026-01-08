'use client';
import React, { useState, useEffect, useRef } from 'react';
import { useTranslations } from 'next-intl';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { ChatPanel } from './ChatPanel';
import { ExplorerPanel } from './ExplorerPanel';
import { EditorPanel } from './EditorPanel';
import { TerminalPane } from './TerminalPane';
import { DocumentTabs } from './DocumentTabs';
import { DocumentViewer } from './DocumentViewer';
import { Settings } from 'lucide-react';
import { EditorProvider } from '../contexts/EditorContext';
import { useChat } from '../hooks/useChat';
import { ProtocolExplorer } from './dev/ProtocolExplorer';

interface Tab {
    id: string;
    title: string;
    type: 'file' | 'ephemeral';
    path?: string;
    content?: string;
    suggestedName?: string;
    highlightLines?: { startLine: number; endLine: number };
}

const AppLayoutInner: React.FC = () => {
    const t = useTranslations();
    const [activeTabId, setActiveTabId] = useState<string | null>(null);
    const [tabs, setTabs] = useState<Tab[]>([]);
    const [terminalHeight, setTerminalHeight] = useState(300);
    const [isDragging, setIsDragging] = useState(false);
    const [virtualFiles, setVirtualFiles] = useState<Set<string>>(new Set());
    const { pendingChanges, approveChange, rejectChange } = useChat();
    const processingFilesRef = useRef<Set<string>>(new Set());

    // Poll for virtual buffer state
    useEffect(() => {
        const checkVirtualBuffers = async () => {
            try {
                const files = await invoke<string[]>('get_virtual_files');
                setVirtualFiles(new Set(files));
            } catch (e) {
                console.error('[VIRTUAL BUFFER] Failed to get virtual files:', e);
            }
        };

        checkVirtualBuffers();
        const interval = setInterval(checkVirtualBuffers, 1000);
        return () => clearInterval(interval);
    }, []);

    // Auto-open files when edit proposals arrive
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen<{ id: string; path: string; old_content: string; new_content: string }[]>('propose-edit', (event) => {
                if (event.payload.length > 0) {
                    const firstEdit = event.payload[0];
                    const existingTab = tabs.find(t => t.type === 'file' && t.path === firstEdit.path);

                    if (existingTab) {
                        setActiveTabId(existingTab.id);
                    } else {
                        const filename = firstEdit.path.split('/').pop() || firstEdit.path;
                        const newTab: Tab = {
                            id: `file-${firstEdit.path}`,
                            title: filename,
                            type: 'file',
                            path: firstEdit.path,
                        };
                        setTabs(prev => [...prev, newTab]);
                        setActiveTabId(newTab.id);
                    }
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, [tabs]);

    const handleFileSelect = (path: string) => {
        // Add to tabs if not already open
        const existingTab = tabs.find(t => t.type === 'file' && t.path === path);
        if (!existingTab) {
            const filename = path.split('/').pop() || path;
            const newTab: Tab = {
                id: `file-${path}`,
                title: filename,
                type: 'file',
                path,
            };
            setTabs(prev => [...prev, newTab]);
            setActiveTabId(newTab.id);
        } else {
            setActiveTabId(existingTab.id);
        }
    };

    const handleTabClose = (tabId: string) => {
        setTabs(prev => prev.filter(t => t.id !== tabId));
        if (activeTabId === tabId) {
            setActiveTabId(tabs.length > 1 ? tabs[0].id : null);
        }
    };

    const handleEphemeralSave = (ephemeralTabId: string, savedPath: string) => {
        // Convert ephemeral tab to regular file tab
        setTabs(prev => {
            const ephemeralTab = prev.find(t => t.id === ephemeralTabId);
            if (!ephemeralTab) return prev;

            const filename = savedPath.split('/').pop() || savedPath;
            const newTab: Tab = {
                id: `file-${savedPath}`,
                title: filename,
                type: 'file',
                path: savedPath,
            };

            // Remove ephemeral tab and add file tab
            return [...prev.filter(t => t.id !== ephemeralTabId), newTab];
        });

        // Switch to the new file tab
        setActiveTabId(`file-${savedPath}`);

        // No-op here; approval handled by caller
    };

    const handleMouseDown = (e: React.MouseEvent) => {
        setIsDragging(true);
        e.preventDefault();
    };

    useEffect(() => {
        const handleMouseMove = (e: MouseEvent) => {
            if (!isDragging) return;
            // Calculate new height from bottom
            // Height = TotalWindowHeight - MouseY - StatusBarHeight(24px)
            const newHeight = window.innerHeight - e.clientY - 24;
            // Clamp height
            if (newHeight > 100 && newHeight < window.innerHeight - 100) {
                setTerminalHeight(newHeight);
            }
        };

        const handleMouseUp = () => {
            setIsDragging(false);
        };

        if (isDragging) {
            document.addEventListener('mousemove', handleMouseMove);
            document.addEventListener('mouseup', handleMouseUp);
        }

        return () => {
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
        };
    }, [isDragging]);

    // Listen for open-file and open-ephemeral-document events
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlistenFile: (() => void) | undefined;
        let unlistenFileOpened: (() => void) | undefined;
        let unlistenFileWithHighlight: (() => void) | undefined;
        let unlistenEphemeral: (() => void) | undefined;
        let unlistenChangeApplied: (() => void) | undefined;

        const setupListeners = async () => {
            const handleOpenFile = (path: string, sourceEvent: string) => {
                console.log(`Opening file from backend (${sourceEvent}):`, path);
                const tabId = `file-${path}`;

                // Prevent duplicate processing
                if (processingFilesRef.current.has(path)) {
                    console.log('[LAYOUT] Ignoring duplicate file open event for:', path);
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
                setActiveTabId(tabId);
            };

            // Current backend event name (Rust emits this)
            unlistenFile = await listen<string>('open-file', (event) => {
                handleOpenFile(event.payload, 'open-file');
            });

            // Backwards-compatible alias (kept for older emitters)
            unlistenFileOpened = await listen<string>('file-opened', (event) => {
                handleOpenFile(event.payload, 'file-opened');
            });

            unlistenFileWithHighlight = await listen<{ path: string; start_line: number; end_line: number }>('open-file-with-highlight', (event) => {
                console.log('Opening file with highlight from backend:', event.payload);
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
                setActiveTabId(tabId);
            });

            unlistenEphemeral = await listen<{ id: string; title: string; content: string; suggestedName: string }>('open-ephemeral-document', (event) => {
                console.log('[LAYOUT] 📥 Received open-ephemeral-document event:', {
                    id: event.payload.id,
                    title: event.payload.title,
                    contentLength: event.payload.content.length,
                    suggestedName: event.payload.suggestedName
                });

                const { id, title, content, suggestedName } = event.payload;

                setTabs(prev => {
                    // Check if tab already exists
                    const existingTab = prev.find(t => t.id === id);
                    if (existingTab) {
                        console.log('[LAYOUT] ⚠️ Tab already exists, just activating:', id);
                        return prev;
                    }

                    console.log('[LAYOUT] ✅ Creating new tab with ID:', id);
                    const newTab: Tab = {
                        id,
                        title,
                        type: 'ephemeral',
                        content,
                        suggestedName,
                    };
                    console.log('[LAYOUT] Adding tab to existing tabs:', prev.length, '→', prev.length + 1);
                    return [...prev, newTab];
                });
                setActiveTabId(id);
            });

            // Listen for change-applied events to convert ephemeral tabs to file tabs
            const unlistenChangeApplied = await listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
                console.log('[LAYOUT] Change applied:', event.payload);
                const { change_id, file_path } = event.payload;

                // Find ephemeral tab associated with this change
                const ephemeralTabId = `new-file-${change_id}`;
                setTabs(prev => {
                    const ephemeralTab = prev.find(t => t.id === ephemeralTabId);
                    if (!ephemeralTab) return prev;

                    console.log('[LAYOUT] Converting ephemeral tab to file tab:', ephemeralTabId, '→', file_path);
                    const filename = file_path.split('/').pop() || file_path;
                    const newTab: Tab = {
                        id: `file-${file_path}`,
                        title: filename,
                        type: 'file',
                        path: file_path,
                    };

                    // Remove ephemeral tab and add file tab
                    return [...prev.filter(t => t.id !== ephemeralTabId), newTab];
                });

                // Switch to the new file tab
                setActiveTabId(`file-${file_path}`);
            });
        };

        setupListeners();

        return () => {
            if (unlistenFile) unlistenFile();
            if (unlistenFileOpened) unlistenFileOpened();
            if (unlistenFileWithHighlight) unlistenFileWithHighlight();
            if (unlistenEphemeral) unlistenEphemeral();
            if (unlistenChangeApplied) unlistenChangeApplied();
        };
    }, [tabs]);

    return (
        <div className="h-screen w-screen bg-black overflow-hidden flex flex-col font-sans">
            {/* Activity Bar / Title Bar placeholder */}
            <div className="h-9 bg-[#09090b] border-b border-zinc-800 flex items-center justify-between px-3 select-none -webkit-app-region-drag">
                <div className="text-xs font-mono text-zinc-500 font-medium tracking-tight">Zaguán Blade</div>
                <div className="flex items-center gap-3">
                    {/* Window Controls would go here on custom titlebar */}
                </div>
            </div>

            <div className="flex-1 flex overflow-hidden">
                {/* Activity Bar (Vertical) */}
                <div className="w-12 bg-[#09090b] border-r border-black flex flex-col items-center py-3 gap-2 z-20">
                    <div className="p-2.5 bg-zinc-800/80 rounded-sm text-zinc-200 shadow-sm border border-zinc-700/50">
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                        </svg>
                    </div>
                    <div className="p-2.5 text-zinc-600 hover:text-zinc-400 transition-colors cursor-pointer">
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                    </div>
                    <div className="mt-auto p-2.5 text-zinc-600 hover:text-zinc-400 cursor-pointer">
                        <Settings className="w-5 h-5" />
                    </div>
                </div>

                {/* Content Area */}
                <div className="flex-1 flex w-full">


                    {/* Explorer */}
                    <div className="w-64 min-w-[200px] flex flex-col border-r border-black bg-[#101012]">
                        <ExplorerPanel onFileSelect={handleFileSelect} activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                    </div>

                    {/* Editor & Terminal */}
                    <div className="flex-1 flex flex-col min-w-[300px] bg-[#1e1e1e]">
                        {/* Document Tabs */}
                        {tabs.length > 0 && (
                            <DocumentTabs
                                tabs={tabs.map(t => ({
                                    id: t.id,
                                    title: t.title,
                                    isEphemeral: t.type === 'ephemeral',
                                    isDirty: false,
                                    hasVirtualChanges: t.path ? virtualFiles.has(t.path) : false,
                                }))}
                                activeTabId={activeTabId}
                                onTabClick={setActiveTabId}
                                onTabClose={handleTabClose}
                            />
                        )}

                        <div className="flex-1 overflow-hidden">
                            {(() => {
                                const activeTab = tabs.find(t => t.id === activeTabId);
                                if (!activeTab) return <EditorPanel activeFile={null} highlightLines={null} />;

                                if (activeTab.type === 'ephemeral') {
                                    // Check if this is a new file proposal
                                    const isNewFileProposal = activeTab.id.startsWith('new-file-');
                                    const changeId = isNewFileProposal ? activeTab.id.replace('new-file-', '') : undefined;

                                    console.log('[Layout] Rendering ephemeral tab:', {
                                        tabId: activeTab.id,
                                        isNewFileProposal,
                                        changeId,
                                        pendingChangesCount: pendingChanges.length
                                    });

                                    return (
                                        <DocumentViewer
                                            documentId={activeTab.id}
                                            title={activeTab.title}
                                            content={activeTab.content || ''}
                                            isEphemeral={true}
                                            suggestedName={activeTab.suggestedName}
                                            onClose={() => handleTabClose(activeTab.id)}
                                            onSave={(savedPath) => handleEphemeralSave(activeTab.id, savedPath)}
                                            changeId={changeId}
                                            onApprove={changeId ? () => approveChange(changeId) : undefined}
                                        />
                                    );
                                }

                                // Find pending change for this file
                                const pendingChange = pendingChanges.find(c => c.path === activeTab.path);
                                return (
                                    <EditorPanel
                                        activeFile={activeTab.path || null}
                                        highlightLines={activeTab.highlightLines || null}
                                        pendingEdit={pendingChange}
                                        onAcceptEdit={approveChange}
                                        onRejectEdit={rejectChange}
                                    />
                                );
                            })()}
                        </div>
                        {/* Terminal Resizer & Pane */}
                        <div
                            className={`h-1 cursor-row-resize bg-[#2d2d2d] hover:bg-blue-500 transition-colors ${isDragging ? 'bg-blue-500' : ''}`}
                            onMouseDown={handleMouseDown}
                        />
                        <div style={{ height: terminalHeight }} className="border-t border-[#3c3c3c]">
                            <TerminalPane />
                        </div>
                    </div>

                    {/* AI Chat */}
                    <div className="w-96 min-w-[300px] border-l border-zinc-800 bg-[#09090b] flex flex-col shadow-2xl z-10">
                        <ChatPanel />
                    </div>

                </div>
            </div>

            {/* Status Bar */}
            <div className="h-6 bg-[#007acc] text-white flex items-center px-3 text-[10px] font-mono justify-between select-none z-30">
                <div className="flex items-center gap-4">
                    <span className="flex items-center gap-1">
                        <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></svg>
                        main*
                    </span>
                    <span>{(() => {
                        const activeTab = tabs.find(tab => tab.id === activeTabId);
                        return activeTab ? activeTab.title : t('editor.noFileOpen');
                    })()}</span>
                </div>
                <div className="flex items-center gap-4 opacity-80">
                    <span>{t('editor.encoding')}</span>
                    <span>Rust</span>
                    <span>{t('app.name')}</span>
                </div>
            </div>

            {/* Dev Tools */}
            <ProtocolExplorer />
        </div>
    );
};

export const AppLayout: React.FC = () => {
    return (
        <EditorProvider>
            <AppLayoutInner />
        </EditorProvider>
    );
};
