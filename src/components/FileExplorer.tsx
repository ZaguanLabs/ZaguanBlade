import React, { useMemo, useCallback, useState, useRef, useEffect } from 'react';
import { useTree } from '@headless-tree/react';
import {
    selectionFeature,
    asyncDataLoaderFeature,
    hotkeysCoreFeature,
    searchFeature,
    renamingFeature,
    dragAndDropFeature
} from '@headless-tree/core';
import { BladeDispatcher } from '../services/blade';
import { FileEntry } from '../types/blade';
import { Folder, ChevronRight, FileCode, FileText, FileBox, Search, FilePlus, FolderPlus, Pencil, Trash2, Copy, Scissors, Clipboard, Terminal, Loader2 } from 'lucide-react';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { ConfirmModal } from './ui/Modal';
import { listen, emit } from '@tauri-apps/api/event';

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
    // Track pending requests to deduplicate File(List) calls for the same path
    const pendingRequests = React.useRef(new Map<string | null, Promise<string[]>>());
    const { showMenu } = useContextMenu();

    // State for inline new item creation
    const [newItem, setNewItem] = useState<{
        parentPath: string;
        isDir: boolean;
        name: string;
    } | null>(null);
    const newItemInputRef = useRef<HTMLInputElement>(null);
    
    // Focus the new item input when it appears
    useEffect(() => {
        if (newItem && newItemInputRef.current) {
            newItemInputRef.current.focus();
        }
    }, [newItem]);

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

    // Track the last expanded activeFile to prevent repeated expansions for the same file
    // Declared here (before useTree) so it can be reset when refreshKey changes
    const lastExpandedFileRef = React.useRef<string | null>(null);

    // Helper function to invalidate a folder's children in the tree
    const invalidateFolderChildren = useCallback((tree: any, folderPath: string) => {
        try {
            const item = tree.getItemInstance(folderPath);
            if (item && typeof item.invalidateChildrenIds === 'function') {
                // Clear from local cache first
                pendingRequests.current.delete(folderPath === 'root' ? null : folderPath);
                // Use optimistic invalidation to avoid loading flicker
                item.invalidateChildrenIds(true);
                console.log('[Explorer] Invalidated children for:', folderPath);
                return true;
            }
        } catch (err) {
            console.warn('[Explorer] Failed to invalidate:', folderPath, err);
        }
        return false;
    }, []);



    // File operation handlers
    const handleCreateFile = async (name: string, parentPath: string, isDir: boolean) => {
        const fullPath = `${parentPath}/${name}`;
        try {
            await BladeDispatcher.file({
                type: 'Create',
                payload: { path: fullPath, is_dir: isDir }
            });
            console.log(`[Explorer] Created ${isDir ? 'folder' : 'file'}:`, fullPath);
            
            // Auto-open newly created files in the editor (not directories)
            if (!isDir && onFileSelect) {
                onFileSelect(fullPath);
            }
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
                        // Start inline new file creation
                        setNewItem({ parentPath: itemId, isDir: false, name: '' });
                        // Expand the folder to show the new item input
                        const item = treeRef.current?.getItemInstance(itemId);
                        if (item && !item.isExpanded()) {
                            item.expand();
                        }
                    }
                },
                {
                    id: 'new-folder',
                    label: 'New Folder',
                    icon: <FolderPlus className="w-4 h-4" />,
                    shortcut: 'Ctrl+Shift+N',
                    onClick: () => {
                        // Start inline new folder creation
                        setNewItem({ parentPath: itemId, isDir: true, name: '' });
                        // Expand the folder to show the new item input
                        const item = treeRef.current?.getItemInstance(itemId);
                        if (item && !item.isExpanded()) {
                            item.expand();
                        }
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
                    // Trigger inline rename via headless-tree
                    const item = treeRef.current?.getItemInstance(itemId);
                    if (item && typeof item.startRenaming === 'function') {
                        item.startRenaming();
                    }
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
                    // Start inline new file creation at workspace root
                    setNewItem({ parentPath: workspaceRoot, isDir: false, name: '' });
                }
            },
            {
                id: 'new-folder',
                label: 'New Folder',
                icon: <FolderPlus className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+N',
                onClick: () => {
                    // Start inline new folder creation at workspace root
                    setNewItem({ parentPath: workspaceRoot, isDir: true, name: '' });
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

        indent: 12,
        canReorder: true,

        features: [
            asyncDataLoaderFeature,
            selectionFeature,
            hotkeysCoreFeature,
            searchFeature,
            renamingFeature,
            dragAndDropFeature
        ],

        onRename: async (item, newName) => {
            // Validate newName - don't rename if empty or unchanged
            if (!newName || !newName.trim()) {
                return;
            }
            const trimmedName = newName.trim();
            const oldPath = item.getId();
            const oldName = oldPath.substring(oldPath.lastIndexOf('/') + 1);
            if (oldName === trimmedName) {
                return;
            }
            const parentPath = oldPath.substring(0, oldPath.lastIndexOf('/'));
            const newPath = `${parentPath}/${trimmedName}`;
            try {
                await BladeDispatcher.file({
                    type: 'Rename',
                    payload: { old_path: oldPath, new_path: newPath }
                });
                console.log('[Explorer] Renamed via inline:', oldPath, '->', newPath);
            } catch (err) {
                console.error('[Explorer] Failed to rename:', err);
            }
        },

        onDrop: async (items, target) => {
            // Handle drag and drop - move files/folders
            for (const item of items) {
                const sourcePath = item.getId();
                const sourceName = item.getItemName();
                
                // Determine target folder
                let targetFolder: string;
                if ('item' in target && target.item) {
                    targetFolder = target.item.getId();
                } else {
                    continue; // Skip if no valid target
                }
                
                const newPath = `${targetFolder}/${sourceName}`;
                
                // Don't move to same location
                if (sourcePath === newPath) continue;
                
                // Don't move into itself
                if (newPath.startsWith(sourcePath + '/')) {
                    console.warn('[Explorer] Cannot move folder into itself');
                    continue;
                }
                
                try {
                    await BladeDispatcher.file({
                        type: 'Rename',
                        payload: { old_path: sourcePath, new_path: newPath }
                    });
                    console.log('[Explorer] Moved via drag:', sourcePath, '->', newPath);
                } catch (err) {
                    console.error('[Explorer] Failed to move:', err);
                }
            }
        },

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

                // Check if there's already a pending request for this path
                const existingRequest = pendingRequests.current.get(path);
                if (existingRequest) {
                    return existingRequest;
                }

                const requestPromise = new Promise<string[]>((resolve, reject) => {
                    let resolved = false;

                    listen<any>('sys-event', (eventRaw) => {
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
                        }
                    }).then(unlisten => {
                        BladeDispatcher.file({ type: 'List', payload: { path: path } });

                        setTimeout(() => {
                            unlisten();
                            // Clean up pending request when done
                            pendingRequests.current.delete(path);
                            if (!resolved) {
                                resolve([]);
                            }
                        }, 5000);
                    }).catch((err) => {
                        pendingRequests.current.delete(path);
                        reject(err);
                    });
                });

                // Store the pending request
                pendingRequests.current.set(path, requestPromise);
                return requestPromise;
            }
        },
    });

    // Store tree in a ref to avoid dependency issues
    const treeRef = React.useRef(tree);
    treeRef.current = tree;

    // Clear cache and pending requests on refresh to force re-fetch
    // Also reset lastExpandedFileRef so the tree re-expands to active file after refresh
    React.useEffect(() => {
        console.log('[Explorer] Refreshing tree view (Key: ' + refreshKey + ')');
        itemCache.current.clear();
        pendingRequests.current.clear();
        lastExpandedFileRef.current = null;

        const currentTree = treeRef.current;
        // Invalidate root first using the correct API
        invalidateFolderChildren(currentTree, 'root');

        // Also invalidate all currently expanded folders to ensure deep refresh
        const items = currentTree.getItems();
        items.forEach(item => {
            if (item.isFolder() && item.isExpanded()) {
                invalidateFolderChildren(currentTree, item.getId());
            }
        });
    }, [refreshKey, invalidateFolderChildren]);

    // Listen for file system events to update tree dynamically
    React.useEffect(() => {
        let unlisten: (() => void) | undefined;
        const setup = async () => {
            unlisten = await listen<any>('sys-event', (eventRaw) => {
                let evt = eventRaw.payload;
                if (evt.event) evt = evt.event;

                if (evt.type === 'File') {
                    const filePayload = evt.payload;
                    const pathsToInvalidate: string[] = [];

                    if (filePayload.type === 'Created' || filePayload.type === 'Deleted') {
                        pathsToInvalidate.push(filePayload.payload.path);
                    } else if (filePayload.type === 'Renamed') {
                        pathsToInvalidate.push(filePayload.payload.old_path);
                        pathsToInvalidate.push(filePayload.payload.new_path);
                    }

                    // Invalidate parent folders using the correct API
                    const currentTree = treeRef.current;
                    pathsToInvalidate.forEach(path => {
                        const parentPath = path.substring(0, path.lastIndexOf('/'));
                        // Clear from local cache
                        itemCache.current.delete(path);
                        pendingRequests.current.delete(parentPath);
                        
                        if (parentPath) {
                            invalidateFolderChildren(currentTree, parentPath);
                        } else {
                            invalidateFolderChildren(currentTree, 'root');
                        }
                    });
                }
            });
        };
        setup();
        return () => { if (unlisten) unlisten(); };
    }, [invalidateFolderChildren]);

    // Auto-expand and select active file in the tree
    // NOTE: We intentionally exclude 'tree' from dependencies to prevent infinite loops.
    // The tree reference changes frequently as state updates, which would cause this effect
    // to re-run endlessly. Instead, we access tree directly since it's stable within the
    // component's render cycle.
    React.useEffect(() => {
        if (!activeFile) {
            // Clear selection when no file is active
            const selectedItems = tree.getItems().filter(item => item.isSelected());
            selectedItems.forEach(item => {
                if (item.isSelected()) {
                    item.deselect();
                }
            });
            lastExpandedFileRef.current = null;
            return;
        }

        // Skip if we've already expanded and selected this file
        if (lastExpandedFileRef.current === activeFile) {
            return;
        }

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

                // Mark this file as expanded to prevent re-running
                lastExpandedFileRef.current = activeFile;
            } catch (err) {
                console.error('[FileExplorer] Failed to expand/select active file:', err);
            }
        };

        // Small delay to ensure tree is ready
        setTimeout(expandAndSelect, 150);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [activeFile]);

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
                className="flex-1 overflow-y-auto text-xs select-none outline-none"
                onContextMenu={handleBackgroundContextMenu}
            >
                {tree.getItems().map(item => {
                    // Check if we should show the new item input as first child of this folder
                    const showNewItemInput = newItem && 
                        item.isFolder() && 
                        item.getId() === newItem.parentPath && 
                        item.isExpanded();
                    
                    return (
                        <React.Fragment key={item.getId()}>
                            <div
                                {...item.getProps()}
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
                                    // Don't handle click if renaming
                                    if (item.isRenaming?.()) return;
                                    
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

                                {/* Loading indicator */}
                                {item.isLoading?.() && (
                                    <Loader2 className="w-3 h-3 text-[var(--fg-tertiary)] animate-spin" />
                                )}

                                {/* Inline rename input or item name */}
                                {item.isRenaming?.() ? (
                                    <input
                                        {...(item.getRenameInputProps?.() || {})}
                                        className="bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded px-1 text-xs text-[var(--fg-primary)] outline-none flex-1 min-w-0"
                                        autoFocus
                                    />
                                ) : (
                                    <span className="truncate opacity-90 group-hover:opacity-100 transition-opacity">
                                        {item.getItemName()}
                                    </span>
                                )}
                            </div>
                            
                            {/* Inline new item input - shows as first child of expanded folder */}
                            {showNewItemInput && (
                                <div
                                    className="flex items-center gap-1.5 py-1 px-2 relative bg-[var(--bg-surface-hover)]"
                                    style={{ paddingLeft: `${(item.getItemMeta().level + 1) * 12 + 8}px` }}
                                >
                                    {/* Indentation Guides */}
                                    {Array.from({ length: item.getItemMeta().level + 1 }).map((_, i) => (
                                        <div
                                            key={i}
                                            className="absolute top-0 bottom-0 w-px bg-[var(--border-subtle)]/20"
                                            style={{ left: `${i * 12 + 11}px` }}
                                        />
                                    ))}
                                    
                                    <span className="w-3" />
                                    {getIcon(newItem.name || (newItem.isDir ? 'folder' : 'file'), newItem.isDir, false)}
                                    
                                    <input
                                        ref={newItemInputRef}
                                        type="text"
                                        value={newItem.name}
                                        onChange={(e) => setNewItem(prev => prev ? { ...prev, name: e.target.value } : null)}
                                        onKeyDown={(e) => {
                                            if (e.key === 'Enter' && newItem.name.trim()) {
                                                handleCreateFile(newItem.name.trim(), newItem.parentPath, newItem.isDir);
                                                setNewItem(null);
                                            } else if (e.key === 'Escape') {
                                                setNewItem(null);
                                            }
                                        }}
                                        onBlur={() => {
                                            // Small delay to allow click events to fire first
                                            setTimeout(() => setNewItem(null), 150);
                                        }}
                                        placeholder={newItem.isDir ? 'folder name...' : 'file name...'}
                                        className="bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded px-1 text-xs text-[var(--fg-primary)] outline-none flex-1 min-w-0"
                                        autoFocus
                                    />
                                </div>
                            )}
                        </React.Fragment>
                    );
                })}

                {/* Drag line for drag-and-drop */}
                <div 
                    style={tree.getDragLineStyle?.()} 
                    className="h-0.5 bg-[var(--accent-primary)] pointer-events-none relative"
                >
                    <div className="absolute left-0 top-[-3px] h-2 w-2 bg-[var(--bg-panel)] border-2 border-[var(--accent-primary)] rounded-full" />
                </div>

                {tree.getItems().length === 0 && (
                    <div className="p-4 text-[var(--fg-tertiary)] italic">
                        {roots.length > 0 ? "Loading tree..." : "Waiting for workspace..."}
                    </div>
                )}
            </div>

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
