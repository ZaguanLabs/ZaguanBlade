import React, { useState, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Maximize2 } from 'lucide-react';

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
    const appWindow = getCurrentWindow();

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

    // Hide title bar in fullscreen mode for immersive experience
    if (isFullscreen) {
        return null;
    }

    return (
        <div
            className="h-9 bg-[var(--bg-app)] flex items-center justify-between px-1 select-none border-b border-[var(--border-subtle)] relative z-50"
            data-tauri-drag-region
        >
            {/* Left: App branding */}
            <div
                className="flex items-center gap-3 px-3 h-full"
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
            </div>

            {/* Center: Draggable region (flexible spacer) */}
            <div
                className="flex-1 h-full"
                data-tauri-drag-region
            />

            {/* Right: Window controls */}
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
        </div>
    );
};
