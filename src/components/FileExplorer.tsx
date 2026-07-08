import React, { useMemo, useCallback, useState, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
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
import { subscribeBladeNestedEventType } from '../services/bladeEvents';
import { FileEntry } from '../types/blade';
import { Folder, ChevronRight, FileCode, FileText, FileBox, Search, FilePlus, FolderPlus, Pencil, Trash2, Copy, Scissors, Clipboard, Terminal, Loader2 } from 'lucide-react';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { ConfirmModal } from './ui/Modal';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { ScrollArea } from './ui/ScrollArea';

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
    workspaceRoot: string | null;
    refreshKey: number;
}

const TREE_ROOT_ID = '__workspace_tree_root__';
type LoadedTreeChild = { id: string; data: NodeData };

const getIcon = (name: string | undefined, isDir: boolean, expanded: boolean) => {
    if (!name) return <FileBox className="w-3.5 h-3.5 text-(--fg-tertiary)" aria-hidden="true" />;

    if (isDir) {
        return expanded
            ? <Folder className="w-3.5 h-3.5 text-(--fg-secondary) fill-(--fg-secondary)/20" aria-hidden="true" />
            : <Folder className="w-3.5 h-3.5 text-(--fg-tertiary)" aria-hidden="true" />;
    }
    if (name.endsWith('.rs')) return <FileCode className="w-3.5 h-3.5 text-(--syntax-number)" aria-hidden="true" />;
    if (name.endsWith('.tsx') || name.endsWith('.ts')) return <FileCode className="w-3.5 h-3.5 text-(--syntax-type)" aria-hidden="true" />;
    if (name.endsWith('.json') || name.endsWith('.toml')) return <FileCode className="w-3.5 h-3.5 text-(--accent-warning)" aria-hidden="true" />;
    if (name.endsWith('.md')) return <FileText className="w-3.5 h-3.5 text-(--fg-secondary)" aria-hidden="true" />;
    return <FileBox className="w-3.5 h-3.5 text-(--fg-tertiary)" aria-hidden="true" />;
};

const getParentPath = (path: string) => {
    const normalized = path.replace(/\/+$/, '');
    const separatorIndex = normalized.lastIndexOf('/');
    if (separatorIndex <= 0) {
        return normalized.startsWith('/') ? '/' : normalized;
    }
    return normalized.slice(0, separatorIndex);
};

const getBaseName = (path: string) => {
    const normalized = path.replace(/\/+$/, '');
    return normalized.slice(normalized.lastIndexOf('/') + 1);
};

export const FileExplorer: React.FC<FileExplorerProps> = ({ onFileSelect, activeFile, roots, workspaceRoot, refreshKey }) => {
    const { t } = useTranslation();

    // Use Ref for cache to persist data across renders.
    const itemCache = React.useRef(new Map<string, NodeData>());
    // Track pending requests to deduplicate File(List) calls for the same path
    const pendingRequests = React.useRef(new Map<string | null, Promise<LoadedTreeChild[]>>());
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

    // Get workspace root path
    const getWorkspaceRoot = useCallback(() => {
        return workspaceRoot;
    }, [workspaceRoot]);

    const cacheEntry = useCallback((entry: FileEntry) => {
        const nodeData = {
            id: entry.path,
            name: entry.name,
            is_dir: entry.is_dir,
            data: entry
        };
        itemCache.current.set(entry.path, nodeData);
        return nodeData;
    }, []);

    const getWorkspaceRootData = useCallback((rootPath: string): NodeData => ({
        id: rootPath,
        name: getBaseName(rootPath) || rootPath,
        is_dir: true
    }), []);

    const getFallbackItemData = useCallback((itemId: string): NodeData => {
        const rootEntry = roots.find(entry => entry.path === itemId);
        if (rootEntry) {
            return cacheEntry(rootEntry);
        }

        return {
            id: itemId,
            name: getBaseName(itemId) || itemId,
            // When metadata is temporarily unavailable during async invalidation,
            // never classify an existing path as a file. A stale/unknown folder is
            // safer than opening a directory in the editor.
            is_dir: true
        };
    }, [cacheEntry, roots]);

    const resolveDropTargetFolder = useCallback((target: any) => {
        if (!target || !('item' in target) || !target.item) {
            return null;
        }

        const targetId = target.item.getId();
        const currentWorkspaceRoot = getWorkspaceRoot();

        if (targetId === TREE_ROOT_ID) {
            return currentWorkspaceRoot;
        }

        if ('childIndex' in target) {
            if (targetId === currentWorkspaceRoot) {
                return currentWorkspaceRoot;
            }
            return targetId;
        }

        if (target.item.isFolder()) {
            return targetId;
        }

        return getParentPath(targetId);
    }, [getWorkspaceRoot]);

    const moveItemToFolder = useCallback(async (sourcePath: string, targetFolder: string) => {
        const sourceName = getBaseName(sourcePath);
        const sourceParent = getParentPath(sourcePath);
        const newPath = `${targetFolder}/${sourceName}`;

        if (!sourceName || sourcePath === newPath || sourceParent === targetFolder) {
            return;
        }

        if (targetFolder === sourcePath || targetFolder.startsWith(sourcePath + '/')) {
            console.warn('[Explorer] Cannot move folder into itself');
            return;
        }

        await BladeDispatcher.file({
            type: 'Rename',
            payload: { old_path: sourcePath, new_path: newPath }
        });
        console.debug('[Explorer] Moved via drag:', sourcePath, '->', newPath);
    }, []);

    // Sync roots to cache
    React.useEffect(() => {
        if (workspaceRoot) {
            itemCache.current.set(workspaceRoot, getWorkspaceRootData(workspaceRoot));
        }
        roots.forEach(r => {
            cacheEntry(r);
        });
    }, [cacheEntry, getWorkspaceRootData, roots, workspaceRoot]);

    // Track the last expanded activeFile to prevent repeated expansions for the same file
    // Declared here (before useTree) so it can be reset when refreshKey changes
    const lastExpandedFileRef = React.useRef<string | null>(null);

    // Helper function to invalidate a folder's children in the tree
    const invalidateFolderChildren = useCallback((tree: any, folderPath: string) => {
        try {
            const item = tree.getItemInstance(folderPath);
            if (item && typeof item.invalidateChildrenIds === 'function') {
                pendingRequests.current.delete(folderPath === workspaceRoot ? null : folderPath);
                // Use optimistic invalidation to avoid loading flicker
                item.invalidateChildrenIds(true);
                console.debug('[Explorer] Invalidated children for:', folderPath);
                return true;
            }
        } catch (err) {
            console.warn('[Explorer] Failed to invalidate:', folderPath, err);
        }
        return false;
    }, [workspaceRoot]);

    const invalidateItemData = useCallback((tree: any, path: string) => {
        try {
            const item = tree.getItemInstance(path);
            if (item && typeof item.invalidateItemData === 'function') {
                item.invalidateItemData(true);
                return true;
            }
        } catch (err) {
            console.warn('[Explorer] Failed to invalidate item data:', path, err);
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
            console.debug(`[Explorer] Created ${isDir ? 'folder' : 'file'}:`, fullPath);
            
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
            console.debug('[Explorer] Deleted:', path);
        } catch (err) {
            console.error('[Explorer] Failed to delete:', err);
        }
    };
    const handlePaste = useCallback(async (targetFolder: string) => {
        if (!clipboard) return;

        const newPath = `${targetFolder}/${clipboard.name}`;
        try {
            if (clipboard.operation === 'cut') {
                // Move (rename)
                await BladeDispatcher.file({
                    type: 'Rename',
                    payload: { old_path: clipboard.path, new_path: newPath }
                });
                console.debug('[Explorer] Moved:', clipboard.path, '->', newPath);
                setClipboard(null); // Clear clipboard after cut
            } else {
                // Copy - need to read and write
                // For now, just copy the path to system clipboard as a fallback
                console.debug('[Explorer] Copy not yet implemented, path:', clipboard.path);
            }
        } catch (err) {
            console.error('[Explorer] Failed to paste:', err);
        }
    }, [clipboard]);

    // Helper to start inline new item creation in a folder
    const startNewItem = useCallback((folderPath: string, isDir: boolean) => {
        setNewItem({ parentPath: folderPath, isDir, name: '' });
        try {
            const item = treeRef.current?.getItemInstance(folderPath);
            if (item && !item.isExpanded()) {
                item.expand();
            }
        } catch {
            return;
        }
    }, []);

    // Context menu builder for files/folders
    const getFileContextMenu = useCallback((itemId: string, isFolder: boolean, itemName: string): ContextMenuItem[] => {
        const isWorkspaceRoot = itemId === workspaceRoot;
        const parentPath = isFolder ? itemId : itemId.substring(0, itemId.lastIndexOf('/'));
        const targetFolder = isFolder ? itemId : parentPath;

        const items: ContextMenuItem[] = [
            {
                id: 'new-file',
                label: t('fileTree.newFile'),
                icon: <FilePlus className="w-4 h-4" />,
                shortcut: 'Ctrl+N',
                onClick: () => startNewItem(targetFolder, false)
            },
            {
                id: 'new-folder',
                label: t('fileTree.newFolder'),
                icon: <FolderPlus className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+N',
                onClick: () => startNewItem(targetFolder, true)
            },
            { id: 'div-1', label: '', divider: true }
        ];

        if (isWorkspaceRoot) {
            items.push(
                {
                    id: 'paste',
                    label: t('fileTree.paste'),
                    icon: <Clipboard className="w-4 h-4" />,
                    shortcut: 'Ctrl+V',
                    disabled: !clipboard,
                    onClick: () => {
                        handlePaste(itemId);
                    }
                },
                { id: 'div-2', label: '', divider: true },
                {
                    id: 'copy-path',
                    label: t('fileTree.copyPath'),
                    icon: <Copy className="w-4 h-4" />,
                    shortcut: 'Ctrl+Shift+C',
                    onClick: async () => {
                        try {
                            await navigator.clipboard.writeText(itemId);
                            console.debug('[Context] Copied path:', itemId);
                        } catch (err) {
                            console.error('[Context] Failed to copy path:', err);
                        }
                    }
                },
                {
                    id: 'open-in-terminal',
                    label: t('fileTree.openInTerminal'),
                    icon: <Terminal className="w-4 h-4" />,
                    onClick: async () => {
                        console.debug('[Context] Open terminal at:', itemId);
                        await emit('open-terminal', { path: itemId });
                    }
                }
            );

            return items;
        }

        // Cut, Copy, Paste
        items.push(
            {
                id: 'cut',
                label: t('fileTree.cut'),
                icon: <Scissors className="w-4 h-4" />,
                shortcut: 'Ctrl+X',
                onClick: () => {
                    setClipboard({ path: itemId, name: itemName, operation: 'cut' });
                    console.debug('[Context] Cut:', itemId);
                }
            },
            {
                id: 'copy',
                label: t('fileTree.copy'),
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                onClick: () => {
                    setClipboard({ path: itemId, name: itemName, operation: 'copy' });
                    console.debug('[Context] Copy:', itemId);
                }
            },
            {
                id: 'paste',
                label: t('fileTree.paste'),
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
                label: t('fileTree.rename'),
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
                label: t('fileTree.delete'),
                icon: <Trash2 className="w-4 h-4" />,
                shortcut: 'Delete',
                danger: true,
                onClick: () => {
                    setConfirmModal({
                        isOpen: true,
                        title: t('fileTree.delete'),
                        message: t('fileTree.deleteConfirmMessage', { name: itemName }),
                        targetPath: itemId
                    });
                }
            },
            { id: 'div-3', label: '', divider: true },
            {
                id: 'copy-path',
                label: t('fileTree.copyPath'),
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+C',
                onClick: async () => {
                    try {
                        await navigator.clipboard.writeText(itemId);
                        console.debug('[Context] Copied path:', itemId);
                    } catch (err) {
                        console.error('[Context] Failed to copy path:', err);
                    }
                }
            },
            {
                id: 'open-in-terminal',
                label: t('fileTree.openInTerminal'),
                icon: <Terminal className="w-4 h-4" />,
                onClick: async () => {
                    // For folders, open terminal at the folder path
                    // For files, open terminal at the parent directory
                    const targetPath = isFolder ? itemId : parentPath;
                    console.debug('[Context] Open terminal at:', targetPath);
                    await emit('open-terminal', { path: targetPath });
                }
            }
        );

        return items;
    }, [clipboard, handlePaste, startNewItem, t, workspaceRoot]);

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
                label: t('fileTree.newFile'),
                icon: <FilePlus className="w-4 h-4" />,
                shortcut: 'Ctrl+N',
                onClick: () => startNewItem(workspaceRoot, false)
            },
            {
                id: 'new-folder',
                label: t('fileTree.newFolder'),
                icon: <FolderPlus className="w-4 h-4" />,
                shortcut: 'Ctrl+Shift+N',
                onClick: () => startNewItem(workspaceRoot, true)
            },
            { id: 'div-1', label: '', divider: true },
            {
                id: 'paste',
                label: t('fileTree.paste'),
                icon: <Clipboard className="w-4 h-4" />,
                shortcut: 'Ctrl+V',
                disabled: !clipboard,
                onClick: () => {
                    handlePaste(workspaceRoot);
                }
            }
        ];

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [showMenu, clipboard, getWorkspaceRoot, handlePaste, startNewItem, t]);

    // Keyboard shortcuts: Ctrl+N (new file), Ctrl+Shift+N (new folder)
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Skip if an input/textarea is focused
            if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

            if (e.ctrlKey && e.key === 'n' && !e.shiftKey) {
                e.preventDefault();
                const root = getWorkspaceRoot();
                if (root) startNewItem(root, false);
            } else if (e.ctrlKey && e.shiftKey && e.key === 'N') {
                e.preventDefault();
                const root = getWorkspaceRoot();
                if (root) startNewItem(root, true);
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [getWorkspaceRoot, startNewItem]);

    const tree = useTree<NodeData>({
        rootItemId: TREE_ROOT_ID,
        getItemName: (item) => item.getItemData()?.name || t('common.unknown'),
        isItemFolder: (item) => item.getItemData()?.is_dir || false,

        indent: 12,
        canReorder: true,
        initialState: {
            expandedItems: workspaceRoot ? [workspaceRoot] : []
        },

        features: [
            asyncDataLoaderFeature,
            selectionFeature,
            hotkeysCoreFeature,
            searchFeature,
            renamingFeature,
            dragAndDropFeature
        ],

        onRename: async (item, newName) => {
            if (item.getId() === workspaceRoot || item.getId() === TREE_ROOT_ID) {
                return;
            }

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
                console.debug('[Explorer] Renamed via inline:', oldPath, '->', newPath);
            } catch (err) {
                console.error('[Explorer] Failed to rename:', err);
            }
        },

        onDrop: async (items, target) => {
            const targetFolder = resolveDropTargetFolder(target);
            if (!targetFolder) {
                return;
            }

            for (const item of items) {
                const sourcePath = item.getId();
                if (sourcePath === workspaceRoot || sourcePath === TREE_ROOT_ID) {
                    continue;
                }

                try {
                    await moveItemToFolder(sourcePath, targetFolder);
                } catch (err) {
                    console.error('[Explorer] Failed to move:', err);
                }
            }
        },

        createLoadingItemData: () => ({
            id: 'loading',
            name: t('common.loading'),
            is_dir: false
        }),

        dataLoader: {
            getItem: (itemId) => {
                if (itemId === TREE_ROOT_ID) {
                    return { id: TREE_ROOT_ID, name: 'root', is_dir: true };
                }
                if (itemId === workspaceRoot) {
                    return getWorkspaceRootData(workspaceRoot);
                }
                return itemCache.current.get(itemId) || getFallbackItemData(itemId);
            },
            getChildrenWithData: (itemId) => {
                if (itemId === TREE_ROOT_ID) {
                    if (!workspaceRoot) {
                        return [];
                    }

                    const data = getWorkspaceRootData(workspaceRoot);
                    itemCache.current.set(workspaceRoot, data);
                    return [{ id: workspaceRoot, data }];
                }

                const path = itemId === workspaceRoot ? null : itemId;

                if (itemId === workspaceRoot) {
                    return roots.map(entry => ({
                        id: entry.path,
                        data: cacheEntry(entry)
                    }));
                }

                // Check if there's already a pending request for this path
                const existingRequest = pendingRequests.current.get(path);
                if (existingRequest) {
                    return existingRequest;
                }

                const requestPromise = (async () => {
                    try {
                        const entries = await invoke<FileEntry[]>('list_files', { path });
                        return entries.map((entry) => ({
                            id: entry.path,
                            data: cacheEntry(entry)
                        }));
                    } catch (err) {
                        console.error('[Explorer] Failed to list files:', err);
                        return [];
                    } finally {
                        pendingRequests.current.delete(path);
                    }
                })();

                // Store the pending request
                pendingRequests.current.set(path, requestPromise);
                return requestPromise;
            }
        },
    });

    // Store tree in a ref to avoid dependency issues
    const treeRef = React.useRef(tree);
    treeRef.current = tree;

    React.useEffect(() => {
        if (!workspaceRoot) return;
        invalidateFolderChildren(treeRef.current, workspaceRoot);
    }, [roots, workspaceRoot, invalidateFolderChildren]);

    // Clear pending requests on refresh to force re-fetch while preserving item metadata.
    // Also reset lastExpandedFileRef so the tree re-expands to active file after refresh
    React.useEffect(() => {
        console.debug('[Explorer] Refreshing tree view (Key: ' + refreshKey + ')');
        pendingRequests.current.clear();
        lastExpandedFileRef.current = null;

        const currentTree = treeRef.current;
        // Invalidate both the hidden tree root and the visible workspace root.
        invalidateFolderChildren(currentTree, TREE_ROOT_ID);
        if (workspaceRoot) {
            invalidateFolderChildren(currentTree, workspaceRoot);
        }

        // Also invalidate all currently expanded folders to ensure deep refresh
        const items = currentTree.getItems();
        items.forEach(item => {
            if (item.isFolder() && item.isExpanded()) {
                invalidateFolderChildren(currentTree, item.getId());
            }
        });
    }, [refreshKey, invalidateFolderChildren, workspaceRoot]);

    React.useEffect(() => {
        if (!workspaceRoot) return;

        const workspaceRootItem = treeRef.current.getItemInstance(workspaceRoot);
        if (workspaceRootItem && workspaceRootItem.isFolder() && !workspaceRootItem.isExpanded()) {
            workspaceRootItem.expand();
        }
    }, [workspaceRoot]);

    // Listen for file system events to update tree dynamically
    React.useEffect(() => {
        const invalidateParentFolders = (pathsToInvalidate: string[]) => {
            const currentTree = treeRef.current;
            const workspaceRoot = getWorkspaceRoot();
            pathsToInvalidate.forEach(path => {
                const parentPath = path.substring(0, path.lastIndexOf('/'));
                pendingRequests.current.delete(parentPath);

                if (parentPath && parentPath !== workspaceRoot) {
                    invalidateFolderChildren(currentTree, parentPath);
                } else {
                    if (workspaceRoot) {
                        invalidateFolderChildren(currentTree, workspaceRoot);
                    }
                }
            });
        };

        const unsubscribeCreated = subscribeBladeNestedEventType('File', 'Created', (payload) => {
            cacheEntry({
                name: getBaseName(payload.path) || payload.path,
                path: payload.path,
                is_dir: payload.is_dir,
                children: null
            });
            invalidateItemData(treeRef.current, payload.path);
            invalidateParentFolders([payload.path]);
        });
        const unsubscribeDeleted = subscribeBladeNestedEventType('File', 'Deleted', (payload) => {
            itemCache.current.delete(payload.path);
            invalidateParentFolders([payload.path]);
        });
        const unsubscribeRenamed = subscribeBladeNestedEventType('File', 'Renamed', (payload) => {
            const previous = itemCache.current.get(payload.old_path);
            itemCache.current.delete(payload.old_path);

            if (previous) {
                const name = getBaseName(payload.new_path) || payload.new_path;
                itemCache.current.set(payload.new_path, {
                    ...previous,
                    id: payload.new_path,
                    name,
                    data: previous.data
                        ? {
                            ...previous.data,
                            path: payload.new_path,
                            name
                        }
                        : undefined
                });
                invalidateItemData(treeRef.current, payload.new_path);
            }

            invalidateParentFolders([payload.old_path, payload.new_path]);
        });

        return () => {
            unsubscribeCreated();
            unsubscribeDeleted();
            unsubscribeRenamed();
        };
    }, [cacheEntry, getWorkspaceRoot, invalidateFolderChildren, invalidateItemData]);

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
                if (fileItem) {
                    tree.getItems().forEach(item => {
                        if (item.getId() !== activeFile && item.isSelected()) {
                            item.deselect();
                        }
                    });

                    if (!fileItem.isSelected()) {
                        fileItem.select();
                    }
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
            <div className="px-2 py-2 border-b border-(--border-subtle) flex items-center gap-2">
                <Search className="w-3 h-3 text-(--fg-tertiary)" aria-hidden="true" />
                <input
                    type="text"
                    aria-label={t('fileTree.searchPlaceholder')}
                    placeholder={t('fileTree.searchPlaceholder')}
                    className="bg-transparent border-none outline-none text-xs w-full text-(--fg-primary) placeholder-(--fg-tertiary)"
                    onChange={(e) => tree.setSearch(e.target.value)}
                />
            </div>

            <ScrollArea
                {...tree.getContainerProps()}
                className="flex-1 text-xs select-none outline-none"
                onContextMenu={handleBackgroundContextMenu}
            >
                {tree.getItems().map(item => {
                    const isWorkspaceRoot = workspaceRoot === item.getId();
                    const isActiveFile = !item.isFolder() && activeFile === item.getId();
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
                                    ${isActiveFile
                                        ? 'bg-(--bg-selection) text-(--accent-secondary)'
                                        : item.isSelected()
                                            ? 'bg-(--bg-selection)/60 text-(--fg-primary)'
                                        : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover)'
                                    }
                                    ${item.isFocused() ? 'ring-1 ring-inset ring-(--border-focus)' : ''}
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
                                        className="absolute top-0 bottom-0 w-px bg-(--border-subtle)/20"
                                        style={{ left: `${i * 12 + 11}px` }}
                                    />
                                ))}

                                <span className={`transition-transform duration-200 ${item.isExpanded() ? 'rotate-90' : ''}`}>
                                    {item.isFolder() ? <ChevronRight className="w-3 h-3 text-(--fg-tertiary) group-hover:text-(--fg-secondary) transition-colors" aria-hidden="true" /> : <span className="w-3" />}
                                </span>

                                {getIcon(item.getItemName(), item.isFolder(), item.isExpanded())}

                                {/* Loading indicator */}
                                {item.isLoading?.() && (
                                    <Loader2 aria-hidden="true" className="w-3 h-3 text-(--fg-tertiary) animate-spin" />
                                )}

                                {/* Inline rename input or item name */}
                                {item.isRenaming?.() ? (
                                    <input
                                        {...(item.getRenameInputProps?.() || {})}
                                        aria-label={t('fileTree.rename')}
                                        className="bg-(--bg-surface) border border-(--border-focus) rounded-[calc(var(--panel-radius)*0.4)] px-1 text-xs text-(--fg-primary) outline-none flex-1 min-w-0"
                                        autoFocus
                                    />
                                ) : (
                                    <span className={`truncate opacity-90 group-hover:opacity-100 transition-opacity ${isWorkspaceRoot ? 'font-bold text-(--fg-primary)' : ''}`}>
                                        {item.getItemName()}
                                    </span>
                                )}
                            </div>
                            
                            {/* Inline new item input - shows as first child of expanded folder */}
                            {showNewItemInput && (
                                <div
                                    className="flex items-center gap-1.5 py-1 px-2 relative bg-(--bg-surface-hover)"
                                    style={{ paddingLeft: `${(item.getItemMeta().level + 1) * 12 + 8}px` }}
                                >
                                    {/* Indentation Guides */}
                                    {Array.from({ length: item.getItemMeta().level + 1 }).map((_, i) => (
                                        <div
                                            key={i}
                                            className="absolute top-0 bottom-0 w-px bg-(--border-subtle)/20"
                                            style={{ left: `${i * 12 + 11}px` }}
                                        />
                                    ))}
                                    
                                    <span className="w-3" />
                                    {getIcon(newItem.name || (newItem.isDir ? 'folder' : 'file'), newItem.isDir, false)}
                                    
                                    <input
                                        ref={newItemInputRef}
                                        type="text"
                                        aria-label={newItem.isDir ? t('fileTree.folderNamePlaceholder') : t('fileTree.fileNamePlaceholder')}
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
                                        placeholder={newItem.isDir ? t('fileTree.folderNamePlaceholder') : t('fileTree.fileNamePlaceholder')}
                                        className="bg-(--bg-surface) border border-(--border-focus) rounded-[calc(var(--panel-radius)*0.4)] px-1 text-xs text-(--fg-primary) outline-none flex-1 min-w-0"
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
                    className="h-0.5 bg-(--accent-ai) pointer-events-none relative"
                >
                    <div className="absolute left-0 top-[-3px] h-2 w-2 bg-(--bg-panel) border-2 border-(--accent-ai) rounded-full" />
                </div>

                {tree.getItems().length === 0 && (
                    <div role="status" aria-live="polite" className="p-4 text-(--fg-tertiary) italic">
                        {roots.length > 0 ? t('fileTree.loadingTree') : t('fileTree.waitingForWorkspace')}
                    </div>
                )}
            </ScrollArea>

            {/* Confirm Modal for Delete */}
            <ConfirmModal
                isOpen={confirmModal.isOpen}
                title={confirmModal.title}
                message={confirmModal.message}
                confirmLabel={t('fileTree.delete')}
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
