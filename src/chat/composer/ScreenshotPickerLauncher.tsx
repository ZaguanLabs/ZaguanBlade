import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ImageUp, Monitor, Plus, ScanLine } from 'lucide-react';
import { WindowPicker } from '../../components/WindowPicker';
import { RegionSelector } from '../../components/RegionSelector';
import type { CaptureResult, WindowInfo } from '../../types/screenshot';

export const ScreenshotPickerLauncher: React.FC<{
    disabled?: boolean;
    onUploadImage: () => void;
    onCapture: (result: CaptureResult, name: string) => void;
    onError: (message: string) => void;
}> = ({ disabled, onUploadImage, onCapture, onError }) => {
    const menuRef = useRef<HTMLDivElement>(null);
    const [menuOpen, setMenuOpen] = useState(false);
    const [open, setOpen] = useState(false);
    const [loading, setLoading] = useState(false);
    const [mode, setMode] = useState<'window' | 'region'>('window');
    const [windows, setWindows] = useState<WindowInfo[]>([]);
    const [regionSource, setRegionSource] = useState<number | null>(null);
    const [regionCapture, setRegionCapture] = useState<{ dataUrl: string; width: number; height: number } | null>(null);

    const start = useCallback(async (nextMode: 'window' | 'region') => {
        if (disabled) {
            return;
        }
        setMode(nextMode);
        setOpen(true);
        setLoading(true);
        try {
            setWindows(await invoke<WindowInfo[]>('list_capturable_windows'));
        } catch {
            onError('Failed to list windows for capture.');
            setOpen(false);
        } finally {
            setLoading(false);
        }
    }, [disabled, onError]);

    useEffect(() => {
        if (!menuOpen) {
            return;
        }

        const closeMenu = (event: MouseEvent) => {
            if (menuRef.current?.contains(event.target as Node)) {
                return;
            }
            setMenuOpen(false);
        };

        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                setMenuOpen(false);
            }
        };

        document.addEventListener('mousedown', closeMenu);
        document.addEventListener('keydown', closeOnEscape);
        return () => {
            document.removeEventListener('mousedown', closeMenu);
            document.removeEventListener('keydown', closeOnEscape);
        };
    }, [menuOpen]);

    const runAction = useCallback((action: () => void) => {
        setMenuOpen(false);
        action();
    }, []);

    const selectWindow = useCallback(async (windowId: number) => {
        setLoading(true);
        try {
            setOpen(false);
            await new Promise((resolve) => window.setTimeout(resolve, 500));
            const result = await invoke<CaptureResult>('capture_window', { windowId });
            if (mode === 'region') {
                setRegionSource(windowId);
                setRegionCapture({
                    dataUrl: `data:${result.mime_type};base64,${result.data}`,
                    width: result.width,
                    height: result.height,
                });
            } else {
                onCapture(result, `window-${windowId}.png`);
            }
        } catch {
            onError('Failed to capture window.');
        } finally {
            setLoading(false);
        }
    }, [mode, onCapture, onError]);

    const confirmRegion = useCallback(async (region: { x: number; y: number; width: number; height: number }) => {
        if (regionSource == null) {
            onError('No source window selected.');
            return;
        }
        try {
            const result = await invoke<CaptureResult>('capture_window_region', {
                windowId: regionSource,
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
            });
            onCapture(result, 'region.png');
        } catch {
            onError('Failed to capture region.');
        } finally {
            setRegionSource(null);
            setRegionCapture(null);
        }
    }, [onCapture, onError, regionSource]);

    return (
        <>
            <div ref={menuRef} className="relative">
                <button
                    type="button"
                    onClick={() => setMenuOpen((value) => !value)}
                    disabled={disabled}
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    className="inline-flex h-8 items-center gap-1.5 rounded-md border border-(--border-subtle) bg-(--bg-app) px-2 text-[11px] font-medium text-(--fg-secondary) shadow-(--shadow-sm) transition-colors hover:border-[color-mix(in_srgb,var(--accent-ai)_32%,transparent)] hover:text-(--fg-primary) disabled:opacity-40"
                >
                    <span className="inline-flex h-4 w-4 items-center justify-center rounded-sm bg-[color-mix(in_srgb,var(--accent-ai)_14%,transparent)] text-(--accent-ai)">
                        <Plus className="h-3 w-3" />
                    </span>
                    Actions
                    <ChevronDown className={`h-3 w-3 transition-transform ${menuOpen ? 'rotate-180' : ''}`} />
                </button>
                {menuOpen && (
                    <div
                        role="menu"
                        className="absolute bottom-full left-0 z-50 mb-2 w-60 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface) p-1.5 shadow-(--shadow-lg)"
                    >
                        <div className="px-2 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-(--fg-tertiary)">
                            Capture
                        </div>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(() => void start('window'))}
                            className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-(--border-subtle) bg-(--bg-app) text-(--accent-ai)">
                                <Monitor className="h-3.5 w-3.5" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">Window</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">Choose an app window to attach.</span>
                            </span>
                        </button>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(() => void start('region'))}
                            className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-(--border-subtle) bg-(--bg-app) text-(--accent-planning)">
                                <ScanLine className="h-3.5 w-3.5" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">Region</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">Crop a selected window before attaching.</span>
                            </span>
                        </button>
                        <div className="my-1 border-t border-(--border-subtle)" />
                        <div className="px-2 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-(--fg-tertiary)">
                            Attach
                        </div>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(onUploadImage)}
                            className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-(--border-subtle) bg-(--bg-app) text-(--accent-mention)">
                                <ImageUp className="h-3.5 w-3.5" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">Image</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">Add an image from disk.</span>
                            </span>
                        </button>
                    </div>
                )}
            </div>
            <WindowPicker
                isOpen={open}
                windows={windows}
                loading={loading}
                title={mode === 'region' ? 'Select window for region capture' : 'Select window'}
                subtitle={mode === 'region' ? 'Choose the source window.' : 'Choose the window to capture.'}
                onSelect={selectWindow}
                onCancel={() => setOpen(false)}
            />
            {regionCapture && (
                <RegionSelector
                    isOpen={Boolean(regionCapture)}
                    dataUrl={regionCapture.dataUrl}
                    imageWidth={regionCapture.width}
                    imageHeight={regionCapture.height}
                    onCancel={() => setRegionCapture(null)}
                    onConfirm={confirmRegion}
                />
            )}
        </>
    );
};
