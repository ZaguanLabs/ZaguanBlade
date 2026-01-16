
import React, { useMemo, useCallback, useState } from 'react';
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
import { Folder, ChevronRight, FileCode, FileText, FileBox, Search, FilePlus, FolderPlus, Pencil, Trash2, Copy, Scissors, Clipboard, Terminal } from 'lucide-react';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { InputModal, ConfirmModal } from './ui/Modal';
import { emit } from '@tauri-apps/api/event';

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
    const { showMenu } = useContextMenu();

    // Modal state
    const [inputModal, setInputModal] = useState<{
        isOpen: boolean;
        type: 'new-file' | 'new-folder' | 'rename';
        title: string;
        defaultValue: string;
        targetPath: string;
        isDir: boolean;
    }>({ isOpen: false, type: 'new-file', title: '', defaultValue: '', targetPath: '', isDir: false });

    const [confirmModal, setConfirmModal] = useState<{
        isOpen: boolean;
        title: string;
        message: string;
        targetPath: string;
    }>({ isOpen: false, title: '', message: '', targetPath: '' });

    // Clipboard state for Cut/Copy/Paste operations
    const [clipboard, setClipboard] = useState<{
        path: string;
        name: string;
        operation: 'copy' | 'cut';
    } | null>(null);

    // Get workspace root path (first root's parent or first root itself)
    const getWorkspaceRoot = useCallback(() => {
        if (roots.length === 0) return null;
        const firstRoot = roots[0].path;
        // Return the parent directory of the first root, or the root itself
        return firstRoot;
    }, [roots]);

    // Sync roots to cache
    React.useEffect(() => {
        roots.forEach(r => {
            if (!itemCache.current.has(r.path)) {
                itemCache.current.set(r.path, { id: r.path, name: r.name, is_dir: r.is_dir, data: r });
            }
        });
    }, [roots]);

    // File operation handlers
    const handleCreateFile = async (name: string, parentPath: string, isDir: boolean) => {
        const fullPath = `${parentPath}/${name}`;
        try {
            await BladeDispatcher.file({
                type: 'Create',
                payload: { path: fullPath, is_dir: isDir }
            });
            console.log(`[Explorer] Created ${isDir ? 'folder' : 'file'}:`, fullPath);
        } catch (err) {
            console.error('[Explorer] Failed to create:', err);
        }
    };

    const handleDeleteFile = async (path: string) => {
        try {
            await BladeDispatcher.file({
                type: 'Delete',
                payload: { path }
            });
            console.log('[Explorer] Deleted:', path);
        } catch (err) {
            console.error('[Explorer] Failed to delete:', err);
        }
    };

    const handleRenameFile = async (oldPath: string, newName: string) => {
        const parentPath = oldPath.substring(0, oldPath.lastIndexOf('/'));
        const newPath = `${parentPath}/${newName}`;
        try {
            await BladeDispatcher.file({
                type: 'Rename',
                payload: { old_path: oldPath, new_path: newPath }
            });
            console.log('[Explorer] Renamed:', oldPath, '->', newPath);
        } catch (err) {
            console.error('[Explorer] Failed to rename:', err);
        }
    };

    const handlePaste = async (targetFolder: string) => {
        if (!clipboard) return;

        const newPath = `${targetFolder}/${clipboard.name}`;
        try {
            if (clipboard.operation === 'cut') {
                // Move (rename)
                await BladeDispatcher.file({
                    type: 'Rename',
                    payload: { old_path: clipboard.path, new_path: newPath }
                });
                console.log('[Explorer] Moved:', clipboard.path, '->', newPath);
                setClipboard(null); // Clear clipboard after cut
            } else {
                // Copy - need to read and write
                // For now, just copy the path to system clipboard as a fallback
                console.log('[Explorer] Copy not yet implemented, path:', clipboard.path);
            }
        } catch (err) {
            console.error('[Explorer] Failed to paste:', err);
        }
    };

    // Context menu builder for files/folders
    const getFileContextMenu = useCallback((itemId: string, isFolder: boolean, itemName: string): ContextMenuItem[] => {
        const parentPath = isFolder ? itemId : itemId.substring(0, itemId.lastIndexOf('/'));

        const items: ContextMenuItem[] = [];

        if (isFolder) {
            items.push(
                {
                    id: 'new-file',
                    label: 'New File',
                    icon: <FilePlus className="w-4 h-4" />,
                    shortcut: 'Ctrl+N',
                    onClick: () => {
                        setInputModal({
                            isOpen: true,
                            type: 'new-file',
                            title: 'New File',
                            defaultValue: '',
                            targetPath: itemId,
                            isDir: false
                        });
                    }
                },
                {
                    id: 'new-folder',
                    label: 'New Folder',
                    icon: <FolderPlus className="w-4 h-4" />,
                    shortcut: 'Ctrl+Shift+N',
                    onClick: () => {
                        setInputModal({
                            isOpen: true,
                            type: 'new-folder',
                            title: 'New Folder',
                            defaultValue: '',
                            targetPath: itemId,
                            isDir: true
                        });
                    }
                },
                { id: 'div-1', label: '', divider: true }
            );
        }

        // Cut, Copy, Paste
        items.push(
            {
                id: 'cut',
                label: 'Cut',
                icon: <Scissors className="w-4 h-4" />,
                shortcut: 'Ctrl+X',
                onClick: () => {
                    setClipboard({ path: itemId, name: itemName, operation: 'cut' });
                    console.log('[Context] Cut:', itemId);
                }
            },
            {
                id: 'copy',
                label: 'Copy',
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                onClick: () => {
                    setClipboard({ path: itemId, name: itemName, operation: 'copy' });
                    console.log('[Context] Copy:', itemId);
                }
            },
            {
                id: 'paste',
                label: 'Paste',
                icon: <Clipboard className="w-4 h-4" />,
                shortcut: 'Ctrl+V',
                disabled: !clipboard,
                onClick: () => {
                    const targetFolder = isFolder ? itemId : parentPath;
                    handlePaste(targetFolder);
                }
            },
            { id: 'div-2', label: '', divider: true },
            {
                id: 'rename',
                label: 'Rename',
                icon: <Pencil className="w-4 h-4" />,
                shortcut: 'F2',
                onClick: () => {
                    setInputModal({
                        isOpen: true,
                        type: 'rename',
                        title: 'Rename',
                        defaultValue: itemName,
                        targetPath: itemId,
                        isDir: isFolder
                    });
                }
            },
            {
                id: 'delete',
                label: 'Delete',
                icon: <Trash2 className="w-4 h-4" />,
                shortcut: 'Delete',
                danger: true,
                onClick: () => {
                    setConfirmModal({
                        isOpen: true,
                        title: 'Delete',
                        message: `Are you sure you want to delete "${itemName}"? This action cannot be undone.`,
                        targetPath: itemId
                    });
                }
            },
            { id: 'div-3', label: '', divider: true },
            {
                id: 'copy-path',
                label: 'Copy Path',
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+C',
                onClick: async () => {
                    try {
                        await navigator.clipboard.writeText(itemId);
                        console.log('[Context] Copied path:', itemId);
                    } catch (err) {
                        console.error('[Context] Failed to copy path:', err);
                    }
                }
            },
            {
                id: 'open-in-terminal',
                label: 'Open in Terminal',
                icon: <Terminal className="w-4 h-4" />,
                onClick: async () => {
                    // For folders, open terminal at the folder path
                    // For files, open terminal at the parent directory
                    const targetPath = isFolder ? itemId : parentPath;
                    console.log('[Context] Open terminal at:', targetPath);
                    await emit('open-terminal', { path: targetPath });
                }
            }
        );

        return items;
    }, [clipboard]);

    // Context menu for item right-click
    const handleContextMenu = useCallback((e: React.MouseEvent, itemId: string, isFolder: boolean, itemName: string) => {
        e.preventDefault();
        e.stopPropagation();
        const menuItems = getFileContextMenu(itemId, isFolder, itemName);
        showMenu({ x: e.clientX, y: e.clientY }, menuItems, { itemId, isFolder, itemName });
    }, [showMenu, getFileContextMenu]);

    // Background context menu for empty space right-click
    const handleBackgroundContextMenu = useCallback((e: React.MouseEvent) => {
        // Only trigger if clicking on the container itself, not a tree item
        if ((e.target as HTMLElement).closest('[data-tree-item]')) return;

        e.preventDefault();
        e.stopPropagation();

        const workspaceRoot = getWorkspaceRoot();
        if (!workspaceRoot) return;

        const items: ContextMenuItem[] = [
            {
                id: 'new-file',
                label: 'New File',
                icon: <FilePlus className="w-4 h-4" />,
                shortcut: 'Ctrl+N',
                onClick: () => {
                    setInputModal({
                        isOpen: true,
                        type: 'new-file',
                        title: 'New File',
                        defaultValue: '',
                        targetPath: workspaceRoot,
                        isDir: false
                    });
                }
            },
            {
                id: 'new-folder',
                label: 'New Folder',
                icon: <FolderPlus className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+N',
                onClick: () => {
                    setInputModal({
                        isOpen: true,
                        type: 'new-folder',
                        title: 'New Folder',
                        defaultValue: '',
                        targetPath: workspaceRoot,
                        isDir: true
                    });
                }
            },
            { id: 'div-1', label: '', divider: true },
            {
                id: 'paste',
                label: 'Paste',
                icon: <Clipboard className="w-4 h-4" />,
                shortcut: 'Ctrl+V',
                disabled: !clipboard,
                onClick: () => {
                    handlePaste(workspaceRoot);
                }
            }
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [showMenu, clipboard, getWorkspaceRoot]);

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

    // Auto-expand and select active file in the tree
    React.useEffect(() => {
        if (!activeFile) return;

        const expandAndSelect = async () => {
            try {
                // Get all parent folders of the active file
                const pathParts = activeFile.split('/');
                const parentPaths: string[] = [];
                
                // Build parent paths (e.g., /a, /a/b, /a/b/c for file /a/b/c/file.txt)
                for (let i = 1; i < pathParts.length - 1; i++) {
                    const parentPath = pathParts.slice(0, i + 1).join('/');
                    parentPaths.push(parentPath);
                }

                // Expand all parent folders
                for (const parentPath of parentPaths) {
                    const item = tree.getItems().find(item => item.getId() === parentPath);
                    if (item && item.isFolder() && !item.isExpanded()) {
                        await item.expand();
                        // Wait a bit for children to load
                        await new Promise(resolve => setTimeout(resolve, 50));
                    }
                }

                // Select the active file
                const fileItem = tree.getItems().find(item => item.getId() === activeFile);
                if (fileItem && !fileItem.isSelected()) {
                    fileItem.select();
                }
            } catch (err) {
                console.error('[FileExplorer] Failed to expand/select active file:', err);
            }
        };

        // Small delay to ensure tree is ready
        setTimeout(expandAndSelect, 150);
    }, [activeFile, tree]);

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
                onContextMenu={handleBackgroundContextMenu}
            >
                {tree.getItems().map(item => (
                    <div
                        {...item.getProps()}
                        key={item.getId()}
                        data-tree-item
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
                        onContextMenu={(e) => handleContextMenu(e, item.getId(), item.isFolder(), item.getItemName() || '')}
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

            {/* Input Modal for New File/Folder/Rename */}
            <InputModal
                isOpen={inputModal.isOpen}
                title={inputModal.title}
                placeholder={inputModal.type === 'rename' ? 'Enter new name...' : inputModal.type === 'new-file' ? 'Enter file name...' : 'Enter folder name...'}
                defaultValue={inputModal.defaultValue}
                confirmLabel={inputModal.type === 'rename' ? 'Rename' : 'Create'}
                onConfirm={(value) => {
                    if (inputModal.type === 'rename') {
                        handleRenameFile(inputModal.targetPath, value);
                    } else {
                        handleCreateFile(value, inputModal.targetPath, inputModal.isDir);
                    }
                    setInputModal(prev => ({ ...prev, isOpen: false }));
                }}
                onCancel={() => setInputModal(prev => ({ ...prev, isOpen: false }))}
            />

            {/* Confirm Modal for Delete */}
            <ConfirmModal
                isOpen={confirmModal.isOpen}
                title={confirmModal.title}
                message={confirmModal.message}
                confirmLabel="Delete"
                confirmVariant="danger"
                onConfirm={() => {
                    handleDeleteFile(confirmModal.targetPath);
                    setConfirmModal(prev => ({ ...prev, isOpen: false }));
                }}
                onCancel={() => setConfirmModal(prev => ({ ...prev, isOpen: false }))}
            />
        </div>
    );
};
