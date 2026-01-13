
import React, { useMemo } from 'react';
import { useTree } from '@headless-tree/react';
import {
    syncDataLoaderFeature,
    selectionFeature,
    asyncDataLoaderFeature,
    hotkeysCoreFeature,
    searchFeature
} from '@headless-tree/core';
import { BladeDispatcher } from '../services/blade';
import { FileEntry } from '../types/blade';
import { Folder, ChevronRight, FileCode, FileText, FileBox, Search } from 'lucide-react';

// Define the Node type for our tree
interface NodeData {
    id: string; // absolute path
    name: string;
    is_dir: boolean;
    data?: FileEntry;
}

interface FileExplorerProps {
    onFileSelect: (path: string) => void;
    activeFile: string | null;
    roots: FileEntry[];
    refreshKey: number;
}

const getIcon = (name: string | undefined, isDir: boolean, expanded: boolean) => {
    if (!name) return <FileBox className="w-3.5 h-3.5 text-[var(--fg-tertiary)]" />;

    if (isDir) {
        return expanded
            ? <Folder className="w-3.5 h-3.5 text-[var(--fg-secondary)] fill-[var(--fg-secondary)]/20" />
            : <Folder className="w-3.5 h-3.5 text-[var(--fg-tertiary)]" />;
    }
    if (name.endsWith('.rs')) return <FileCode className="w-3.5 h-3.5 text-orange-600/80" />;
    if (name.endsWith('.tsx') || name.endsWith('.ts')) return <FileCode className="w-3.5 h-3.5 text-blue-500/80" />;
    if (name.endsWith('.json') || name.endsWith('.toml')) return <FileCode className="w-3.5 h-3.5 text-yellow-500/80" />;
    if (name.endsWith('.md')) return <FileText className="w-3.5 h-3.5 text-[var(--fg-secondary)]" />;
    return <FileBox className="w-3.5 h-3.5 text-[var(--fg-tertiary)]" />;
};

export const FileExplorer: React.FC<FileExplorerProps> = ({ onFileSelect, activeFile, roots, refreshKey }) => {

    // Use Ref for cache to persist data across renders.
    const itemCache = React.useRef(new Map<string, NodeData>());

    // Sync roots to cache
    React.useEffect(() => {
        roots.forEach(r => {
            if (!itemCache.current.has(r.path)) {
                itemCache.current.set(r.path, { id: r.path, name: r.name, is_dir: r.is_dir, data: r });
            }
        });
    }, [roots]);

    const tree = useTree<NodeData>({
        rootItemId: 'root',
        getItemName: (item) => item.getItemData()?.name || 'Unknown',
        isItemFolder: (item) => item.getItemData()?.is_dir || false,

        features: [
            syncDataLoaderFeature,
            asyncDataLoaderFeature,
            selectionFeature,
            hotkeysCoreFeature,
            searchFeature
        ],

        createLoadingItemData: () => ({
            id: 'loading',
            name: 'Loading...',
            is_dir: false
        }),

        dataLoader: {
            getItem: (itemId) => {
                if (itemId === 'root') return { id: 'root', name: 'root', is_dir: true };
                return itemCache.current.get(itemId) || { id: itemId, name: itemId.split('/').pop() || itemId, is_dir: false };
            },
            getChildren: (itemId) => {
                const path = itemId === 'root' ? null : itemId;

                if (itemId === 'root' && roots.length > 0) {
                    roots.forEach(r => {
                        itemCache.current.set(r.path, { id: r.path, name: r.name, is_dir: r.is_dir, data: r });
                    });
                    return roots.map(r => r.path);
                }

                return new Promise<string[]>((resolve, reject) => {
                    import('@tauri-apps/api/event').then(async ({ listen }) => {
                        let resolved = false;
                        const unlisten = await listen<any>('sys-event', (eventRaw) => {
                            let evt = eventRaw.payload;
                            if (evt.event && evt.id && evt.timestamp) {
                                evt = evt.event;
                            }

                            if (evt.type === 'File' &&
                                evt.payload.type === 'Listing' &&
                                evt.payload.payload.path === path) {
                                resolved = true;
                                const entries = evt.payload.payload.entries;

                                entries.forEach((e: any) => {
                                    itemCache.current.set(e.path, {
                                        id: e.path,
                                        name: e.name,
                                        is_dir: e.is_dir,
                                        data: e
                                    });
                                });

                                resolve(entries.map((e: any) => e.path));
                                unlisten();
                            }
                        });

                        BladeDispatcher.file({ type: 'List', payload: { path: path } });

                        setTimeout(() => {
                            unlisten();
                            if (!resolved) {
                                resolve([]);
                            }
                        }, 5000);
                    });
                });
            }
        },
    });

    return (
        <div className="flex flex-col h-full w-full">
            {/* Search Bar */}
            <div className="px-2 py-2 border-b border-[var(--border-subtle)] flex items-center gap-2">
                <Search className="w-3 h-3 text-[var(--fg-tertiary)]" />
                <input
                    type="text"
                    placeholder="Search..."
                    className="bg-transparent border-none outline-none text-xs w-full text-[var(--fg-primary)] placeholder-[var(--fg-tertiary)]"
                    onChange={(e) => tree.setSearch(e.target.value)}
                />
            </div>

            <div
                {...tree.getContainerProps()}
                className="flex-1 overflow-y-auto text-xs font-mono select-none outline-none"
            >
                {tree.getItems().map(item => (
                    <div
                        {...item.getProps()}
                        key={item.getId()}
                        className={`group flex items-center gap-1.5 py-1 px-2 cursor-pointer relative
                            ${item.isSelected()
                                ? 'bg-[var(--bg-selection)] text-[var(--accent-secondary)]'
                                : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                            }
                            ${item.isFocused() ? 'ring-1 ring-inset ring-[var(--border-focus)]' : ''}
                        `}
                        style={{ paddingLeft: `${(item.getItemMeta().level) * 12 + 8}px` }}
                        onClick={(e) => {
                            if (item.isFolder()) {
                                if (item.isExpanded()) {
                                    item.collapse();
                                } else {
                                    item.expand();
                                }
                            } else {
                                item.select();
                                onFileSelect(item.getId());
                            }
                        }}
                    >
                        {/* Indentation Guides */}
                        {Array.from({ length: item.getItemMeta().level }).map((_, i) => (
                            <div
                                key={i}
                                className="absolute top-0 bottom-0 w-px bg-[var(--border-subtle)]/20"
                                style={{ left: `${i * 12 + 11}px` }}
                            />
                        ))}

                        <span className={`transition-transform duration-200 ${item.isExpanded() ? 'rotate-90' : ''}`}>
                            {item.isFolder() ? <ChevronRight className="w-3 h-3 text-[var(--fg-tertiary)] group-hover:text-[var(--fg-secondary)] transition-colors" /> : <span className="w-3" />}
                        </span>

                        {getIcon(item.getItemName(), item.isFolder(), item.isExpanded())}

                        <span className="truncate opacity-90 group-hover:opacity-100 transition-opacity">
                            {item.getItemName()}
                        </span>
                    </div>
                ))}

                {tree.getItems().length === 0 && (
                    <div className="p-4 text-[var(--fg-tertiary)] italic">
                        {roots.length > 0 ? "Loading tree..." : "Waiting for workspace..."}
                    </div>
                )}
            </div>
        </div>
    );
};
