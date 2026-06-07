import React, { useState, useEffect, useCallback, useId, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Maximize2, ChevronDown } from 'lucide-react';
import zbladeAppIcon from '../assets/zblade-app-icon.png';

/**
 * Custom TitleBar Component
 * 
 * Replaces native OS window decorations with a custom, branded title bar.
 * Features:
 * - Draggable region for window movement
 * - Minimize, Maximize/Restore, and Close buttons
 * - Visual feedback for maximized state
 * - Premium micro-animations
 */
export const TitleBar: React.FC = () => {
    const { t } = useTranslation();
    const [isMaximized, setIsMaximized] = useState(false);
    const [isFullscreen, setIsFullscreen] = useState(false);
    const [fileMenuOpen, setFileMenuOpen] = useState(false);
    const fileMenuId = useId();
    const fileMenuRef = useRef<HTMLDivElement>(null);
    const appWindow = getCurrentWindow();

    // Close file menu when clicking outside
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
        setFileMenuOpen(!fileMenuOpen);
    }, [fileMenuOpen]);

    // Track window maximized and fullscreen state
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            // Get initial state
            const maximized = await appWindow.isMaximized();
            setIsMaximized(maximized);

            const fullscreen = await appWindow.isFullscreen();
            setIsFullscreen(fullscreen);

            // Listen for resize events to update maximized state
            unlisten = await appWindow.onResized(async () => {
                const maximized = await appWindow.isMaximized();
                setIsMaximized(maximized);

                const fullscreen = await appWindow.isFullscreen();
                setIsFullscreen(fullscreen);
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, [appWindow]);

    // F11 fullscreen toggle
    useEffect(() => {
        const handleKeyDown = async (e: KeyboardEvent) => {
            if (e.key === 'F11') {
                e.preventDefault();
                try {
                    const currentFullscreen = await appWindow.isFullscreen();
                    console.debug('[TitleBar] Toggling fullscreen:', !currentFullscreen);
                    await appWindow.setFullscreen(!currentFullscreen);
                    setIsFullscreen(!currentFullscreen);
                } catch (err) {
                    console.error('[TitleBar] Failed to toggle fullscreen:', err);
                }
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [appWindow]);

    const handleMinimize = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        try {
            console.debug('[TitleBar] Minimizing window...');
            await appWindow.minimize();
        } catch (err) {
            console.error('[TitleBar] Failed to minimize:', err);
        }
    };

    const handleMaximizeRestore = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        try {
            console.debug('[TitleBar] Toggling maximize...');
            await appWindow.toggleMaximize();
        } catch (err) {
            console.error('[TitleBar] Failed to toggle maximize:', err);
        }
    };

    const handleClose = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        try {
            console.debug('[TitleBar] Closing window...');
            await appWindow.close();
        } catch (err) {
            console.error('[TitleBar] Failed to close:', err);
        }
    };

    return (
        <div
            className="h-9 bg-(--bg-app) flex items-center justify-between px-1 select-none border-b border-(--border-subtle) relative z-51"
            data-tauri-drag-region
        >
            {/* Left: File Menu */}
            {!isFullscreen && (
                <div className="flex items-center h-full" ref={fileMenuRef}>
                    <div className="relative">
                        <button
                            type="button"
                            onClick={handleFileMenuClick}
                            aria-haspopup="menu"
                            aria-expanded={fileMenuOpen}
                            aria-controls={fileMenuOpen ? fileMenuId : undefined}
                            className={`flex items-center gap-1 px-3 h-9 text-[11px] font-medium transition-colors ${fileMenuOpen
                                    ? 'bg-(--bg-surface) text-(--fg-primary)'
                                    : 'text-(--fg-tertiary) hover:bg-(--bg-surface) hover:text-(--fg-secondary)'
                                }`}
                        >
                            {t('app.menu.file')}
                            <ChevronDown className={`w-3 h-3 transition-transform ${fileMenuOpen ? 'rotate-180' : ''}`} aria-hidden="true" />
                        </button>

                        {/* File Menu Dropdown */}
                        {fileMenuOpen && (
                            <div
                                id={fileMenuId}
                                role="menu"
                                aria-label={t('app.menu.file')}
                                className="absolute top-full left-0 mt-0.5 min-w-[180px] py-1.5 bg-(--bg-surface) border border-(--border-focus) rounded-[calc(var(--panel-radius)*0.75)] shadow-(--shadow-xl) z-100"
                            >
                                <button
                                    type="button"
                                    role="menuitem"
                                    onClick={() => {
                                        setFileMenuOpen(false);
                                        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true }));
                                    }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <span>{t('fileTree.newFile')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Ctrl+N</span>
                                </button>
                                <button
                                    type="button"
                                    role="menuitem"
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <span>{t('titleBar.openFolder')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Ctrl+O</span>
                                </button>
                                <div role="separator" className="my-1.5 mx-2 h-px bg-(--border-subtle)" />
                                <button
                                    type="button"
                                    role="menuitem"
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <span>{t('common.save')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Ctrl+S</span>
                                </button>
                                <button
                                    type="button"
                                    role="menuitem"
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <span>{t('common.saveAs')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Ctrl+Shift+S</span>
                                </button>
                                <div role="separator" className="my-1.5 mx-2 h-px bg-(--border-subtle)" />
                                <button
                                    type="button"
                                    role="menuitem"
                                    onClick={() => { setFileMenuOpen(false); appWindow.close(); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <span>{t('common.exit')}</span>
                                    <span className="text-[10px] text-(--fg-tertiary) font-mono">Alt+F4</span>
                                </button>
                            </div>
                        )}
                    </div>
                </div>
            )}

            {/* Center: App branding (centered) */}
            <div
                className="flex-1 flex items-center justify-center gap-2 h-full"
                data-tauri-drag-region
            >
                {/* Logo/Icon */}
                <img src={zbladeAppIcon} alt="" className="w-5 h-5 object-contain" draggable={false} />

                {/* App name */}
                <span
                    className="text-[11px] font-medium text-(--fg-tertiary) tracking-wider uppercase"
                    data-tauri-drag-region
                >
                    Zaguán Blade
                </span>

                {/* Fullscreen indicator */}
                {isFullscreen && (
                    <span className="text-[9px] text-(--fg-tertiary) opacity-50 ml-2">
                        (F11 to exit)
                    </span>
                )}
            </div>

            {/* Right: Window controls - hidden in fullscreen for immersive experience */}
            {!isFullscreen && (
                <div className="flex items-center h-full gap-1 pr-1">
                    {/* Minimize */}
                    <button
                        type="button"
                        onClick={handleMinimize}
                        className="window-control-btn h-7 w-7 rounded-[calc(var(--panel-radius)*0.45)] flex items-center justify-center text-(--fg-tertiary) hover:bg-(--bg-surface) hover:text-(--fg-secondary) active:scale-95 transition-all duration-(--transition-fast)"
                        title={t('windowControls.minimize')}
                        aria-label={t('windowControls.minimize')}
                    >
                        <Minus className="w-3.5 h-3.5" strokeWidth={1.8} aria-hidden="true" />
                    </button>

                    {/* Maximize/Restore */}
                    <button
                        type="button"
                        onClick={handleMaximizeRestore}
                        className="window-control-btn h-7 w-7 rounded-[calc(var(--panel-radius)*0.45)] flex items-center justify-center text-(--fg-tertiary) hover:bg-(--bg-surface) hover:text-(--fg-secondary) active:scale-95 transition-all duration-(--transition-fast)"
                        title={isMaximized ? t('windowControls.restore') : t('windowControls.maximize')}
                        aria-label={isMaximized ? t('windowControls.restore') : t('windowControls.maximize')}
                    >
                        {isMaximized ? (
                            <Maximize2 className="w-3 h-3" strokeWidth={1.8} aria-hidden="true" />
                        ) : (
                            <Square className="w-3 h-3" strokeWidth={1.8} aria-hidden="true" />
                        )}
                    </button>

                    {/* Close */}
                    <button
                        type="button"
                        onClick={handleClose}
                        className="window-control-btn h-7 w-7 rounded-[calc(var(--panel-radius)*0.45)] flex items-center justify-center text-(--fg-tertiary) hover:bg-(--state-danger) hover:text-(--fg-bright) active:scale-95 transition-all duration-(--transition-fast)"
                        title={t('windowControls.close')}
                        aria-label={t('windowControls.close')}
                    >
                        <X className="w-3.5 h-3.5" strokeWidth={1.8} aria-hidden="true" />
                    </button>
                </div>
            )}
        </div>
    );
};
