import React, { useCallback, useEffect, useId, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ImageUp, Monitor, Plus, ScanLine } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { WindowPicker } from '../../components/WindowPicker';
import { RegionSelector } from '../../components/RegionSelector';
import type { CaptureResult, WindowInfo } from '../../types/screenshot';

export const ScreenshotPickerLauncher: React.FC<{
    disabled?: boolean;
    imagesDisabled?: boolean;
    imageDisabledReason?: string;
    onUploadImage: () => void;
    onCapture: (result: CaptureResult, name: string) => void;
    onError: (message: string) => void;
}> = ({ disabled, imagesDisabled, imageDisabledReason, onUploadImage, onCapture, onError }) => {
    const { t } = useTranslation();
    const menuRef = useRef<HTMLDivElement>(null);
    const menuId = useId();
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
        if (imagesDisabled) {
            onError(imageDisabledReason || t('chat.imageNotSupported'));
            return;
        }
        setMode(nextMode);
        setOpen(true);
        setLoading(true);
        try {
            setWindows(await invoke<WindowInfo[]>('list_capturable_windows'));
        } catch {
            onError(t('screenshot.errors.listWindowsFailed'));
            setOpen(false);
        } finally {
            setLoading(false);
        }
    }, [disabled, imageDisabledReason, imagesDisabled, onError, t]);

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

    const uploadImage = useCallback(() => {
        if (imagesDisabled) {
            onError(imageDisabledReason || t('chat.imageNotSupported'));
            return;
        }
        onUploadImage();
    }, [imageDisabledReason, imagesDisabled, onError, onUploadImage, t]);

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
            onError(t('screenshot.errors.captureWindowFailed'));
        } finally {
            setLoading(false);
        }
    }, [mode, onCapture, onError, t]);

    const confirmRegion = useCallback(async (region: { x: number; y: number; width: number; height: number }) => {
        if (regionSource == null) {
            onError(t('screenshot.errors.noSourceWindow'));
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
            onError(t('screenshot.errors.captureRegionFailed'));
        } finally {
            setRegionSource(null);
            setRegionCapture(null);
        }
    }, [onCapture, onError, regionSource, t]);

    return (
        <>
            <div ref={menuRef} className="relative">
                <button
                    type="button"
                    onClick={() => setMenuOpen((value) => !value)}
                    disabled={disabled}
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    aria-controls={menuOpen ? menuId : undefined}
                    className="inline-flex h-8 items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app) px-2 text-[11px] font-medium text-(--fg-secondary) shadow-(--shadow-sm) transition-colors hover:border-[color-mix(in_srgb,var(--accent-ai)_32%,transparent)] hover:text-(--fg-primary) disabled:opacity-40"
                >
                    <span className="inline-flex h-4 w-4 items-center justify-center rounded-[calc(var(--panel-radius)*0.25)] bg-[color-mix(in_srgb,var(--accent-ai)_14%,transparent)] text-(--accent-ai)">
                        <Plus className="h-3 w-3" aria-hidden="true" />
                    </span>
                    {t('screenshot.launcher.actions')}
                    <ChevronDown className={`h-3 w-3 transition-transform ${menuOpen ? 'rotate-180' : ''}`} aria-hidden="true" />
                </button>
                {menuOpen && (
                    <div
                        id={menuId}
                        role="menu"
                        aria-label={t('screenshot.launcher.actions')}
                        className="absolute bottom-full left-0 z-50 mb-2 w-60 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface) p-1.5 shadow-(--shadow-lg)"
                    >
                        <div className="px-2 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-(--fg-tertiary)">
                            {t('screenshot.featureMenu.captureSection')}
                        </div>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(() => void start('window'))}
                            className="flex w-full items-center gap-2 rounded-[calc(var(--panel-radius)*0.45)] px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app) text-(--accent-ai)">
                                <Monitor className="h-3.5 w-3.5" aria-hidden="true" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">{t('screenshot.launcher.window')}</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">{t('screenshot.launcher.windowDescription')}</span>
                            </span>
                        </button>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(() => void start('region'))}
                            className="flex w-full items-center gap-2 rounded-[calc(var(--panel-radius)*0.45)] px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app) text-(--accent-planning)">
                                <ScanLine className="h-3.5 w-3.5" aria-hidden="true" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">{t('screenshot.launcher.region')}</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">{t('screenshot.launcher.regionDescription')}</span>
                            </span>
                        </button>
                        <div role="separator" className="my-1 border-t border-(--border-subtle)" />
                        <div className="px-2 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-(--fg-tertiary)">
                            {t('screenshot.featureMenu.attachSection')}
                        </div>
                        <button
                            type="button"
                            role="menuitem"
                            onClick={() => runAction(uploadImage)}
                            className="flex w-full items-center gap-2 rounded-[calc(var(--panel-radius)*0.45)] px-2 py-2 text-left text-(--fg-secondary) transition-colors hover:bg-(--bg-app) hover:text-(--fg-primary)"
                        >
                            <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app) text-(--accent-mention)">
                                <ImageUp className="h-3.5 w-3.5" aria-hidden="true" />
                            </span>
                            <span className="min-w-0">
                                <span className="block text-[12px] font-medium">{t('screenshot.launcher.image')}</span>
                                <span className="block truncate text-[10px] text-(--fg-tertiary)">{t('screenshot.launcher.imageDescription')}</span>
                            </span>
                        </button>
                    </div>
                )}
            </div>
            <WindowPicker
                isOpen={open}
                windows={windows}
                loading={loading}
                title={mode === 'region' ? t('screenshot.regionPickerTitle') : t('screenshot.windowPickerTitle')}
                subtitle={mode === 'region' ? t('screenshot.regionPickerSubtitle') : t('screenshot.windowPickerSubtitle')}
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
