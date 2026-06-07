import React, { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Maximize2, ChevronDown, FileText } from 'lucide-react';
import zbladeAppIcon from '../assets/zblade-app-icon.png';
import { getFileIcon } from '../lib/fileIcons';
import { useContextMenu } from './ui/ContextMenu';

export interface AppBarTab {
    id: string;
    title: string;
    path?: string;
    isEphemeral?: boolean;
    isDirty?: boolean;
    hasVirtualChanges?: boolean;
    isAiEdited?: boolean;
    hasUnreadAiEdit?: boolean;
}

interface AppBarProps {
    tabs?: AppBarTab[];
    activeTabId?: string | null;
    projectName?: string | null;
    onTabClick?: (id: string) => void;
    onTabClose?: (id: string) => void;
    onReorder?: (fromIndex: number, toIndex: number) => void;
    onTabMoveToBeginning?: (id: string) => void;
    onTabMoveToEnd?: (id: string) => void;
    onTabCloseAll?: () => void;
    onTabCloseOthers?: (id: string) => void;
    tabStripMaxWidth?: number;
}

export const AppBar: React.FC<AppBarProps> = ({
    tabs = [],
    activeTabId,
    projectName,
    onTabClick,
    onTabClose,
    onReorder,
    onTabMoveToBeginning,
    onTabMoveToEnd,
    onTabCloseAll,
    onTabCloseOthers,
    tabStripMaxWidth,
}) => {
    const [isMaximized, setIsMaximized] = useState(false);
    const [isFullscreen, setIsFullscreen] = useState(false);
    const [fileMenuOpen, setFileMenuOpen] = useState(false);
    const fileMenuRef = useRef<HTMLDivElement>(null);
    const scrollRef = useRef<HTMLDivElement>(null);
    const activeTabRef = useRef<HTMLDivElement>(null);
    const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
    const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);
    const { t } = useTranslation();
    const { showMenu } = useContextMenu();
    const appWindow = getCurrentWindow();

    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (fileMenuRef.current && !fileMenuRef.current.contains(e.target as Node)) {
                setFileMenuOpen(false);
            }
        };
        if (fileMenuOpen) {
            document.addEventListener('mousedown', handleClickOutside);
        }
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, [fileMenuOpen]);

    const handleFileMenuClick = useCallback((e: React.MouseEvent) => {
        e.stopPropagation();
        setFileMenuOpen(prev => !prev);
    }, []);

    useEffect(() => {
        let unlisten: (() => void) | undefined;
        const setupListener = async () => {
            const maximized = await appWindow.isMaximized();
            setIsMaximized(maximized);
            const fullscreen = await appWindow.isFullscreen();
            setIsFullscreen(fullscreen);
            unlisten = await appWindow.onResized(async () => {
                setIsMaximized(await appWindow.isMaximized());
                setIsFullscreen(await appWindow.isFullscreen());
            });
        };
        setupListener();
        return () => { if (unlisten) unlisten(); };
    }, [appWindow]);

    useEffect(() => {
        const handleKeyDown = async (e: KeyboardEvent) => {
            if (e.key === 'F11') {
                e.preventDefault();
                try {
                    const currentFullscreen = await appWindow.isFullscreen();
                    await appWindow.setFullscreen(!currentFullscreen);
                    setIsFullscreen(!currentFullscreen);
                } catch (err) {
                    console.error('[AppBar] Failed to toggle fullscreen:', err);
                }
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [appWindow]);

    const handleMinimize = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        await appWindow.minimize();
    };

    const handleMaximizeRestore = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        await appWindow.toggleMaximize();
    };

    const handleClose = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        await appWindow.close();
    };

    const handleDragStart = useCallback((e: React.DragEvent, index: number) => {
        setDraggedIndex(index);
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', String(index));
        if (e.currentTarget instanceof HTMLElement) {
            e.currentTarget.style.opacity = '0.5';
        }
    }, []);

    const handleDragEnd = useCallback((e: React.DragEvent) => {
        setDraggedIndex(null);
        setDropTargetIndex(null);
        if (e.currentTarget instanceof HTMLElement) {
            e.currentTarget.style.opacity = '1';
        }
    }, []);

    const handleDragOver = useCallback((e: React.DragEvent, index: number) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        if (draggedIndex !== null && draggedIndex !== index) {
            setDropTargetIndex(index);
        }
    }, [draggedIndex]);

    const handleDragLeave = useCallback(() => {
        setDropTargetIndex(null);
    }, []);

    const handleDrop = useCallback((e: React.DragEvent, toIndex: number) => {
        e.preventDefault();
        const fromIndex = draggedIndex;
        setDraggedIndex(null);
        setDropTargetIndex(null);
        if (fromIndex !== null && fromIndex !== toIndex && onReorder) {
            onReorder(fromIndex, toIndex);
        }
    }, [draggedIndex, onReorder]);

    const handleTabContextMenu = useCallback((e: React.MouseEvent, tab: AppBarTab, index: number) => {
        e.preventDefault();
        e.stopPropagation();
        setFileMenuOpen(false);

        const copyText = (value?: string) => {
            if (!value) {
                return;
            }
            void navigator.clipboard.writeText(value);
        };

        showMenu({ x: e.clientX, y: e.clientY }, [
            {
                id: 'copy-filename',
                label: 'Copy filename',
                onClick: () => copyText(tab.title),
            },
            {
                id: 'copy-full-path',
                label: 'Copy full path',
                disabled: !tab.path,
                onClick: () => copyText(tab.path),
            },
            { id: 'divider-copy-move', label: '', divider: true },
            {
                id: 'move-to-beginning',
                label: 'Move to beginning',
                disabled: !onTabMoveToBeginning || index === 0,
                onClick: () => onTabMoveToBeginning?.(tab.id),
            },
            {
                id: 'move-to-end',
                label: 'Move to end',
                disabled: !onTabMoveToEnd || index === tabs.length - 1,
                onClick: () => onTabMoveToEnd?.(tab.id),
            },
            { id: 'divider-move-close', label: '', divider: true },
            {
                id: 'close-others',
                label: 'Close Others',
                disabled: !onTabCloseOthers || tabs.length <= 1,
                onClick: () => onTabCloseOthers?.(tab.id),
            },
            {
                id: 'close-all',
                label: 'Close All',
                danger: true,
                disabled: !onTabCloseAll || tabs.length === 0,
                onClick: () => onTabCloseAll?.(),
            },
        ]);
    }, [onTabCloseAll, onTabCloseOthers, onTabMoveToBeginning, onTabMoveToEnd, showMenu, tabs.length]);

    useEffect(() => {
        if (activeTabId && activeTabRef.current) {
            activeTabRef.current.scrollIntoView({
                behavior: 'smooth',
                block: 'nearest',
                inline: 'nearest',
            });
        }
    }, [activeTabId]);

    const hasTabs = tabs.length > 0;
    const appBarBrandText = projectName ? `Zaguán Blade - ${projectName}` : 'Zaguán Blade';

    return (
        <div
            className="h-9 flex items-stretch select-none relative z-51 shrink-0"
            style={{
                backgroundColor: 'var(--surface-toolbar)',
                borderBottom: '1px solid var(--separator-default)',
                boxShadow: 'var(--shadow-persistent)',
            }}
        >
            {/* Left: File Menu */}
            {!isFullscreen && (
                <div className="flex items-center shrink-0" ref={fileMenuRef}>
                    <div className="relative">
                        <button
                            onClick={handleFileMenuClick}
                            className={`flex items-center gap-1 px-3 h-9 text-[11px] font-medium transition-colors ${
                                fileMenuOpen
                                    ? 'bg-(--surface-overlay) text-(--fg-primary)'
                                    : 'text-(--fg-tertiary) hover:bg-(--surface-overlay) hover:text-(--fg-secondary)'
                            }`}
                        >
                            {t('app.menu.file')}
                            <ChevronDown className={`w-3 h-3 transition-transform ${fileMenuOpen ? 'rotate-180' : ''}`} />
                        </button>

                        {fileMenuOpen && (
                            <div
                                className="absolute top-full left-0 z-100 mt-0.5 min-w-[180px] rounded-(--radius-popover) border border-(--separator-subtle) bg-(--surface-overlay) py-1.5 shadow-(--shadow-popover)"
                            >
                                <button
                                    onClick={() => {
                                        setFileMenuOpen(false);
                                        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
                                    }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) transition-colors hover:bg-(--row-hover)"
                                >
                                    <span>{t('fileTree.newFile')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Ctrl+N</span>
                                </button>
                                <button
                                    disabled
                                    className="w-full flex cursor-not-allowed items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-tertiary) opacity-55"
                                >
                                    <span>{t('fileTree.openFolder')}...</span>
                                </button>
                                <div className="my-1.5 mx-2 h-px bg-(--separator-subtle)" />
                                <button
                                    disabled
                                    className="w-full flex cursor-not-allowed items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-tertiary) opacity-55"
                                >
                                    <span>{t('common.save')}</span>
                                </button>
                                <button
                                    disabled
                                    className="w-full flex cursor-not-allowed items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-tertiary) opacity-55"
                                >
                                    <span>{t('common.saveAs')}</span>
                                </button>
                                <div className="my-1.5 mx-2 h-px bg-(--separator-subtle)" />
                                <button
                                    onClick={() => { setFileMenuOpen(false); appWindow.close(); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) transition-colors hover:bg-(--row-hover)"
                                >
                                    <span>{t('common.exit')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Alt+F4</span>
                                </button>
                            </div>
                        )}
                    </div>
                </div>
            )}

            {/* Center: Tab strip (when tabs open) or drag filler (no tabs) */}
            {hasTabs ? (
                <div
                    ref={scrollRef}
                    className="flex items-end overflow-x-auto tabs-scrollbar min-w-0"
                    style={tabStripMaxWidth ? { width: tabStripMaxWidth, maxWidth: tabStripMaxWidth } : { flex: 1 }}
                >
                    {tabs.map((tab, index) => {
                        const isActive = activeTabId === tab.id;
                        const { icon, color } = getFileIcon(tab.title, isActive);
                        const isDragging = draggedIndex === index;
                        const isDropTarget = dropTargetIndex === index;

                        return (
                            <div
                                key={tab.id}
                                ref={isActive ? activeTabRef : null}
                                draggable
                                onDragStart={(e) => handleDragStart(e, index)}
                                onDragEnd={handleDragEnd}
                                onDragOver={(e) => handleDragOver(e, index)}
                                onDragLeave={handleDragLeave}
                                onDrop={(e) => handleDrop(e, index)}
                                onClick={() => onTabClick?.(tab.id)}
                                onContextMenu={(e) => handleTabContextMenu(e, tab, index)}
                                className={`
                                    group flex items-center gap-1.5 px-3 h-[33px] cursor-pointer
                                    transition-colors relative whitespace-nowrap shrink-0 text-xs
                                    ${isActive
                                        ? 'text-(--fg-primary)'
                                        : 'text-(--fg-tertiary) hover:text-(--fg-secondary)'
                                    }
                                    ${isDragging ? 'opacity-50' : ''}
                                    ${isDropTarget ? 'border-l-2 border-l-(--accent-ai)' : ''}
                                `}
                                style={{
                                    backgroundColor: isActive
                                        ? 'var(--surface-editor)'
                                        : 'color-mix(in srgb, var(--surface-toolbar) 72%, var(--surface-app))',
                                    borderBottom: 'none',
                                }}
                            >
                                {tab.isEphemeral ? (
                                    <FileText className={`w-3.5 h-3.5 shrink-0 ${isActive ? 'text-(--accent-warning)' : 'text-(--fg-tertiary)'}`} />
                                ) : (
                                    <span className={color}>{icon}</span>
                                )}
                                <span className={isActive ? 'font-medium' : ''}>
                                    {tab.title}
                                </span>
                                {tab.isAiEdited && !tab.hasVirtualChanges && (
                                    <span
                                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${tab.hasUnreadAiEdit ? 'animate-pulse' : ''}`}
                                        style={{ backgroundColor: 'var(--accent-ai)' }}
                                        title={tab.hasUnreadAiEdit ? t('tabs.aiEditedUnread') : t('tabs.aiEdited')}
                                    />
                                )}
                                {tab.hasVirtualChanges && (
                                    <span
                                        className="w-1.5 h-1.5 rounded-full shrink-0 animate-pulse"
                                        style={{ backgroundColor: 'var(--accent-warning)' }}
                                        title={tab.hasUnreadAiEdit ? t('tabs.aiPendingReviewUnread') : t('tabs.aiPendingReview')}
                                    />
                                )}
                                {tab.isDirty && !tab.hasVirtualChanges && (
                                    <span
                                        className="w-1.5 h-1.5 rounded-full shrink-0 group-hover:hidden"
                                        style={{ backgroundColor: 'var(--fg-secondary)' }}
                                    />
                                )}
                                <button
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        onTabClose?.(tab.id);
                                    }}
                                    className={`${isActive ? 'opacity-60' : 'opacity-0'} group-hover:opacity-60 hover:opacity-100! hover:bg-(--surface-overlay) rounded-(--radius-control) p-0.5 transition-all ml-0.5`}
                                >
                                    <X className="w-3 h-3" />
                                </button>
                            </div>
                        );
                    })}
                    {/* Drag region filler */}
                    <div className="flex-1 min-w-4 h-full" data-tauri-drag-region />
                </div>
            ) : (
                <div className="flex-1" data-tauri-drag-region />
            )}

            {!hasTabs && (
                <div className="absolute inset-0 flex items-center justify-center gap-2 pointer-events-none" data-tauri-drag-region>
                    <img src={zbladeAppIcon} alt="" className="w-4 h-4 object-contain opacity-60" draggable={false} />
                    <span
                        className="text-[11px] font-medium tracking-wider uppercase"
                        style={{ color: 'var(--fg-tertiary)' }}
                        data-tauri-drag-region
                    >
                        {appBarBrandText}
                    </span>
                    {isFullscreen && (
                        <span className="text-[9px] opacity-40 ml-2" style={{ color: 'var(--fg-tertiary)' }}>
                            {t('windowControls.exitFullscreen')}
                        </span>
                    )}
                </div>
            )}

            {hasTabs && <div className="flex-1" data-tauri-drag-region />}

            {/* Right: Window controls */}
            {!isFullscreen && (
                <div className="flex items-center gap-1 pr-1 shrink-0">
                    <button
                        onClick={handleMinimize}
                        className="h-7 w-7 rounded-(--radius-control) flex items-center justify-center text-(--fg-tertiary) hover:bg-(--surface-overlay) hover:text-(--fg-secondary) active:scale-95 transition-all duration-150"
                        title={t('windowControls.minimize')}
                    >
                        <Minus className="w-3.5 h-3.5" strokeWidth={1.8} />
                    </button>
                    <button
                        onClick={handleMaximizeRestore}
                        className="h-7 w-7 rounded-(--radius-control) flex items-center justify-center text-(--fg-tertiary) hover:bg-(--surface-overlay) hover:text-(--fg-secondary) active:scale-95 transition-all duration-150"
                        title={isMaximized ? t('windowControls.restore') : t('windowControls.maximize')}
                    >
                        {isMaximized ? (
                            <Maximize2 className="w-3 h-3" strokeWidth={1.8} />
                        ) : (
                            <Square className="w-3 h-3" strokeWidth={1.8} />
                        )}
                    </button>
                    <button
                        onClick={handleClose}
                        className="h-7 w-7 rounded-(--radius-control) flex items-center justify-center text-(--fg-tertiary) hover:bg-(--state-danger) hover:text-(--fg-bright) active:scale-95 transition-all duration-150"
                        title={t('windowControls.close')}
                    >
                        <X className="w-3.5 h-3.5" strokeWidth={1.8} />
                    </button>
                </div>
            )}
        </div>
    );
};
