import React, { useState, useEffect, useCallback, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Maximize2, ChevronDown } from 'lucide-react';

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
    const [isMaximized, setIsMaximized] = useState(false);
    const [isFullscreen, setIsFullscreen] = useState(false);
    const [fileMenuOpen, setFileMenuOpen] = useState(false);
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
                    console.log('[TitleBar] Toggling fullscreen:', !currentFullscreen);
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
            console.log('[TitleBar] Minimizing window...');
            await appWindow.minimize();
        } catch (err) {
            console.error('[TitleBar] Failed to minimize:', err);
        }
    };

    const handleMaximizeRestore = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        try {
            console.log('[TitleBar] Toggling maximize...');
            await appWindow.toggleMaximize();
        } catch (err) {
            console.error('[TitleBar] Failed to toggle maximize:', err);
        }
    };

    const handleClose = async (e: React.MouseEvent) => {
        e.stopPropagation();
        e.preventDefault();
        try {
            console.log('[TitleBar] Closing window...');
            await appWindow.close();
        } catch (err) {
            console.error('[TitleBar] Failed to close:', err);
        }
    };

    return (
        <div
            className="h-9 bg-[var(--bg-app)] flex items-center justify-between px-1 select-none border-b border-[var(--border-subtle)] relative z-50"
            data-tauri-drag-region
        >
            {/* Left: File Menu */}
            {!isFullscreen && (
                <div className="flex items-center h-full" ref={fileMenuRef}>
                    <div className="relative">
                        <button
                            onClick={handleFileMenuClick}
                            className={`flex items-center gap-1 px-3 h-9 text-[11px] font-medium transition-colors ${
                                fileMenuOpen 
                                    ? 'bg-[var(--bg-surface)] text-[var(--fg-primary)]' 
                                    : 'text-[var(--fg-tertiary)] hover:bg-[var(--bg-surface)] hover:text-[var(--fg-secondary)]'
                            }`}
                        >
                            File
                            <ChevronDown className={`w-3 h-3 transition-transform ${fileMenuOpen ? 'rotate-180' : ''}`} />
                        </button>

                        {/* File Menu Dropdown */}
                        {fileMenuOpen && (
                            <div className="absolute top-full left-0 mt-0.5 min-w-[180px] py-1.5 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-xl z-[100]"
                                style={{ boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4)' }}
                            >
                                <button
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] transition-colors"
                                >
                                    <span>New File</span>
                                    <span className="text-[10px] text-[var(--fg-tertiary)] font-mono">Ctrl+N</span>
                                </button>
                                <button
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] transition-colors"
                                >
                                    <span>Open Folder...</span>
                                    <span className="text-[10px] text-[var(--fg-tertiary)] font-mono">Ctrl+O</span>
                                </button>
                                <div className="my-1.5 mx-2 h-px bg-[var(--border-subtle)]" />
                                <button
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] transition-colors"
                                >
                                    <span>Save</span>
                                    <span className="text-[10px] text-[var(--fg-tertiary)] font-mono">Ctrl+S</span>
                                </button>
                                <button
                                    onClick={() => { setFileMenuOpen(false); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] transition-colors"
                                >
                                    <span>Save As...</span>
                                    <span className="text-[10px] text-[var(--fg-tertiary)] font-mono">Ctrl+Shift+S</span>
                                </button>
                                <div className="my-1.5 mx-2 h-px bg-[var(--border-subtle)]" />
                                <button
                                    onClick={() => { setFileMenuOpen(false); appWindow.close(); }}
                                    className="w-full flex items-center justify-between px-3 py-1.5 text-[12px] text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] transition-colors"
                                >
                                    <span>Exit</span>
                                    <span className="text-[10px] text-[var(--fg-tertiary)] font-mono">Alt+F4</span>
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
                <div className="w-4 h-4 rounded bg-gradient-to-br from-[var(--fg-secondary)] to-[var(--fg-tertiary)] opacity-60" />

                {/* App name */}
                <span
                    className="text-[11px] font-medium text-[var(--fg-tertiary)] tracking-wider uppercase"
                    data-tauri-drag-region
                >
                    Zaguán Blade
                </span>

                {/* Fullscreen indicator */}
                {isFullscreen && (
                    <span className="text-[9px] text-[var(--fg-tertiary)] opacity-50 ml-2">
                        (F11 to exit)
                    </span>
                )}
            </div>

            {/* Right: Window controls - hidden in fullscreen for immersive experience */}
            {!isFullscreen && (
                <div className="flex items-center h-full">
                    {/* Minimize */}
                    <button
                        onClick={handleMinimize}
                        className="window-control-btn h-full w-11 flex items-center justify-center text-[var(--fg-tertiary)] hover:bg-[var(--bg-surface)] hover:text-[var(--fg-secondary)] transition-all duration-150"
                        title="Minimize"
                    >
                        <Minus className="w-4 h-4" strokeWidth={1.5} />
                    </button>

                    {/* Maximize/Restore */}
                    <button
                        onClick={handleMaximizeRestore}
                        className="window-control-btn h-full w-11 flex items-center justify-center text-[var(--fg-tertiary)] hover:bg-[var(--bg-surface)] hover:text-[var(--fg-secondary)] transition-all duration-150"
                        title={isMaximized ? "Restore" : "Maximize"}
                    >
                        {isMaximized ? (
                            <Maximize2 className="w-3.5 h-3.5" strokeWidth={1.5} />
                        ) : (
                            <Square className="w-3.5 h-3.5" strokeWidth={1.5} />
                        )}
                    </button>

                    {/* Close */}
                    <button
                        onClick={handleClose}
                        className="window-control-btn h-full w-11 flex items-center justify-center text-[var(--fg-tertiary)] hover:bg-[#c42b1c] hover:text-white transition-all duration-150"
                        title="Close"
                    >
                        <X className="w-4 h-4" strokeWidth={1.5} />
                    </button>
                </div>
            )}
        </div>
    );
};
