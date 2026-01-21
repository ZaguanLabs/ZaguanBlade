'use client';
import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { BladeEvent, FileEntry } from '../types/blade';
import { listen } from '@tauri-apps/api/event';
import { FileExplorer } from './FileExplorer';
import { ErrorBoundary } from './ErrorBoundary';
import { ChevronRight } from 'lucide-react';
import { OutlinePanel } from './OutlinePanel';

interface ExplorerPanelProps {
    onFileSelect: (path: string, line?: number, character?: number) => void;
    activeFile: string | null;
}

export const ExplorerPanel: React.FC<ExplorerPanelProps> = ({ onFileSelect, activeFile }) => {
    const [roots, setRoots] = useState<FileEntry[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [refreshKey, setRefreshKey] = useState(0);
    const [outlineHeight, setOutlineHeight] = useState(300); // Fixed height for outline for now

    const loadRoot = React.useCallback(async () => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        try {
            // List workspace root
            await BladeDispatcher.file({
                type: 'List',
                payload: { path: null }
            });
            setError(null);
        } catch (e) {
            console.warn("Failed to load root:", e);
            setError(String(e));
        }
    }, []);

    // Root Listener
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        const setupListener = async () => {
            unlisten = await listen<BladeEvent>('sys-event', (event) => {
                const bladeEvent = event.payload;
                if (bladeEvent.type === 'File') {
                    const fileEvent = bladeEvent.payload;
                    if (fileEvent.type === 'Listing' && fileEvent.payload.path === null) {
                        setRoots(fileEvent.payload.entries);
                    }
                }
            });
        };
        setupListener();
        return () => { if (unlisten) unlisten(); };
    }, []);

    useEffect(() => {
        loadRoot();
    }, [loadRoot]);

    useEffect(() => {
        // Listen for refresh requests from backend
        let unlistenFn: (() => void) | undefined;
        let debounceTimer: ReturnType<typeof setTimeout> | null = null;

        const setupListener = async () => {
            unlistenFn = await listen('refresh-explorer', () => {
                if (debounceTimer) clearTimeout(debounceTimer);
                debounceTimer = setTimeout(() => {
                    console.log('[EXPLORER] Refresh event received (debounced)');
                    setRefreshKey(prev => prev + 1);
                    loadRoot();
                    debounceTimer = null;
                }, 500);
            });
        };
        setupListener();

        return () => {
            if (unlistenFn) unlistenFn();
            if (debounceTimer) clearTimeout(debounceTimer);
        };
    }, [loadRoot]);

    // TEMPORARY: Simple input to open folder
    const [pathInput, setPathInput] = useState('');
    const openSpecificPath = async () => {
        try {
            await invoke('open_workspace', { path: pathInput });
            loadRoot();
        } catch (e) {
            console.error(e);
            setError(String(e));
        }
    };

    return (
        <div className="h-full bg-[var(--bg-panel)] border-r border-[var(--border-subtle)] flex flex-col text-[var(--fg-secondary)]">
            {/* Explorer Section */}
            <div className="flex-1 flex flex-col min-h-0">
                <div className="h-9 px-4 flex items-center bg-[var(--bg-panel)] border-b border-[var(--border-subtle)] text-[10px] uppercase tracking-wider font-semibold select-none justify-between text-[var(--fg-tertiary)] shrink-0">
                    <span>Explorer</span>
                    <button onClick={() => { loadRoot(); setRefreshKey(prev => prev + 1); }} className="hover:text-[var(--fg-primary)]" title="Refresh">
                        <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" /></svg>
                    </button>
                </div>

                <div className="flex-1 overflow-y-auto pt-2 scrollbar-thin scrollbar-thumb-zinc-800">
                    {roots.length === 0 ? (
                        <div className="p-4 flex flex-col gap-2">
                            <p className="text-xs text-[var(--fg-tertiary)] italic text-center">No workspace open.</p>
                            <div className="flex gap-1">
                                <input
                                    className="bg-[var(--bg-surface)] border border-[var(--border-subtle)] text-xs p-1 w-full rounded-sm text-[var(--fg-primary)]"
                                    placeholder="/path/to/folder"
                                    value={pathInput}
                                    onChange={e => setPathInput(e.target.value)}
                                    onKeyDown={e => e.key === 'Enter' && openSpecificPath()}
                                />
                                <button onClick={openSpecificPath} className="p-1.5 bg-[var(--bg-surface)] hover:bg-[var(--bg-surface-hover)] rounded-sm text-[var(--fg-primary)]">
                                    <ChevronRight className="w-3 h-3" />
                                </button>
                            </div>
                            {error && <p className="text-[10px] text-[var(--accent-error)] break-all">{error}</p>}
                        </div>
                    ) : (
                        <ErrorBoundary>
                            <FileExplorer
                                onFileSelect={(path) => onFileSelect(path)}
                                activeFile={activeFile}
                                roots={roots}
                                refreshKey={refreshKey}
                            />
                        </ErrorBoundary>
                    )}
                </div>
            </div>

            {/* Resizer */}
            <div className="h-[1px] bg-[var(--border-subtle)] flex-shrink-0" />

            {/* Outline Section */}
            {activeFile && (
                <div style={{ height: `${outlineHeight}px` }} className="flex flex-col min-h-0 border-t border-[var(--border-subtle)]">
                    <div className="h-7 px-4 flex items-center bg-[var(--bg-panel)] border-b border-[var(--border-subtle)] text-[10px] uppercase tracking-wider font-semibold select-none text-[var(--fg-tertiary)] shrink-0">
                        <span>Outline</span>
                    </div>
                    <div className="flex-1 overflow-hidden">
                        <OutlinePanel
                            filePath={activeFile}
                            onNavigate={(path, line, character) => onFileSelect(path, line, character)}
                        />
                    </div>
                </div>
            )}
        </div>
    );
};
