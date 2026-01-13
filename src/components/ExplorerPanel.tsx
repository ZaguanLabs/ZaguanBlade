'use client';
import React, { useState, useEffect } from 'react';
import { BladeDispatcher } from '../services/blade';
import { BladeEvent, FileEntry } from '../types/blade';
import { listen } from '@tauri-apps/api/event';
// import { FileEntry } from '../types/explorer'; // Replaced by blade types
// import { FileEntry } from '../types/explorer'; // Replaced by blade types
import { FileExplorer } from './FileExplorer';
import { ErrorBoundary } from './ErrorBoundary';
import { Folder, FileBox, ChevronRight, FileCode, FileText } from 'lucide-react';

interface ExplorerPanelProps {
    onFileSelect: (path: string) => void;
    activeFile: string | null;
}

const FileItem: React.FC<{
    entry: FileEntry;
    depth: number;
    onSelect: (path: string) => void;
    activeFile: string | null;
    refreshKey: number;
}> = ({ entry, depth, onSelect, activeFile, refreshKey }) => {
    const [expanded, setExpanded] = useState(false);
    const [children, setChildren] = useState<FileEntry[] | null>(null);

    // Clear cached children when refreshKey changes
    useEffect(() => {
        setChildren(null);
        setExpanded(false);
    }, [refreshKey]);

    // Listener for Listing events
    useEffect(() => {
        if (!expanded && !entry.is_dir) return;

        let unlisten: (() => void) | undefined;
        const setupListener = async () => {
            unlisten = await listen<BladeEvent>('sys-event', (event) => {
                const bladeEvent = event.payload;
                if (bladeEvent.type === 'File') {
                    const fileEvent = bladeEvent.payload;
                    if (fileEvent.type === 'Listing' && fileEvent.payload.path === entry.path) {
                        setChildren(fileEvent.payload.entries);
                    }
                }
            });
        };

        if (expanded) {
            setupListener();
        }

        return () => {
            if (unlisten) unlisten();
        };
    }, [expanded, entry.path, entry.is_dir]);

    const toggleExpand = async () => {
        if (!entry.is_dir) {
            onSelect(entry.path);
            return;
        }

        if (expanded) {
            setExpanded(false);
        } else {
            setExpanded(true);
            if (!children) {
                if (!children) {
                    BladeDispatcher.file({
                        type: 'List',
                        payload: { path: entry.path }
                    }).catch(e => console.error("Failed to dispatch list:", e));
                }
            }
        }
    };

    const isActive = activeFile === entry.path;

    // Icon Selection
    const getIcon = () => {
        if (entry.is_dir) return expanded ? <Folder className="w-4 h-4 text-zinc-400 fill-zinc-400/20" /> : <Folder className="w-4 h-4 text-zinc-500" />;
        if (entry.name.endsWith('.rs')) return <FileCode className="w-4 h-4 text-orange-600/80" />;
        if (entry.name.endsWith('.tsx') || entry.name.endsWith('.ts')) return <FileCode className="w-4 h-4 text-blue-500/80" />;
        if (entry.name.endsWith('.json')) return <FileCode className="w-4 h-4 text-yellow-500/80" />;
        if (entry.name.endsWith('.md')) return <FileText className="w-4 h-4 text-zinc-400" />;
        return <FileBox className="w-4 h-4 text-zinc-600" />;
    };

    return (
        <div>
            <div
                className={`flex items-center gap-1.5 py-1 px-2 cursor-pointer select-none text-xs font-mono transition-colors ${isActive ? 'bg-emerald-500/10 text-emerald-400' : 'hover:bg-zinc-800/50 text-zinc-400'}`}
                style={{ paddingLeft: `${depth * 12 + 8}px` }}
                onClick={toggleExpand}
            >
                {entry.is_dir && (
                    <span className={`transition-transform ${expanded ? 'rotate-90' : ''}`}>
                        <ChevronRight className="w-3 h-3 text-zinc-600" />
                    </span>
                )}
                {!entry.is_dir && <span className="w-3" />} {/* Spacing for files */}

                {getIcon()}
                <span className="truncate">{entry.name}</span>
            </div>

            {expanded && children && (
                <div>
                    {children.map((child) => (
                        <FileItem
                            key={child.path}
                            entry={child}
                            depth={depth + 1}
                            onSelect={onSelect}
                            activeFile={activeFile}
                            refreshKey={refreshKey}
                        />
                    ))}
                </div>
            )}
        </div>
    );
};

export const ExplorerPanel: React.FC<ExplorerPanelProps> = ({ onFileSelect, activeFile }) => {
    const [roots, setRoots] = useState<FileEntry[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [refreshKey, setRefreshKey] = useState(0);

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
        const setupListener = async () => {
            unlistenFn = await listen('refresh-explorer', () => {
                console.log('[EXPLORER] Refresh event received');
                setRefreshKey(prev => prev + 1);
                loadRoot();
            });
        };
        setupListener();

        return () => {
            if (unlistenFn) unlistenFn();
        };
    }, [loadRoot]);

    // TEMPORARY: Simple input to open folder
    const [pathInput, setPathInput] = useState('');
    const openSpecificPath = async () => {
        try {
            // Still using invoke for 'open_folder' as it is a workspace state change, not just a File Read
            // But we need to check if 'invoke' import is available.
            // We removed it from top. We need to re-add it or use @tauri-apps/api/core dynamically
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('open_workspace', { path: pathInput });
            loadRoot();
        } catch (e) {
            console.error(e);
            setError(String(e));
        }
    };

    return (
        <div className="h-full bg-[var(--bg-panel)] border-r border-[var(--border-subtle)] flex flex-col text-[var(--fg-secondary)]">
            <div className="h-9 px-4 flex items-center bg-[var(--bg-panel)] border-b border-[var(--border-subtle)] text-[10px] uppercase tracking-wider font-semibold select-none justify-between text-[var(--fg-tertiary)]">
                <span>Explorer</span>
                <button onClick={loadRoot} className="hover:text-[var(--fg-primary)]" title="Refresh">
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
                    // New Headless File Explorer
                    // We need to import FileExplorer first
                    // New Headless File Explorer
                    // We need to import FileExplorer first
                    <ErrorBoundary>
                        <FileExplorer
                            onFileSelect={onFileSelect}
                            activeFile={activeFile}
                            roots={roots}
                            refreshKey={refreshKey}
                        />
                    </ErrorBoundary>
                )}
            </div>
        </div>
    );
};
