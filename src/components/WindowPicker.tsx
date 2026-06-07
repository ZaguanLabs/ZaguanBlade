import React, { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Monitor } from 'lucide-react';
import type { WindowInfo } from '../types/screenshot';
import { ScrollArea } from './ui/ScrollArea';
import { Surface } from './ui/Surface';
import { ListRow } from './ui/ListRow';
import { IconButton } from './ui/IconButton';

interface WindowPickerProps {
    isOpen: boolean;
    windows: WindowInfo[];
    loading?: boolean;
    title?: string;
    subtitle?: string;
    onSelect: (windowId: number) => void;
    onCancel: () => void;
}

export const WindowPicker: React.FC<WindowPickerProps> = ({
    isOpen,
    windows,
    loading = false,
    title,
    subtitle,
    onSelect,
    onCancel,
}) => {
    const { t } = useTranslation();
    const resolvedTitle = title ?? t('screenshot.windowPickerTitle');
    const resolvedSubtitle = subtitle ?? t('screenshot.windowPickerSubtitle');
    const titleId = useId();
    const subtitleId = useId();
    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div
                aria-hidden="true"
                className="absolute inset-0 bg-(--bg-app)/75"
                onClick={onCancel}
            />
            <Surface
                variant="modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={subtitleId}
                className="relative w-full max-w-xl mx-4"
            >
                <div className="flex items-center justify-between px-4 py-3 border-b border-(--border-subtle)">
                    <div>
                        <div id={titleId} className="text-sm font-semibold text-(--fg-primary)">{resolvedTitle}</div>
                        <div id={subtitleId} className="text-xs text-(--fg-tertiary)">{resolvedSubtitle}</div>
                    </div>
                    <IconButton
                        aria-label="Close"
                        onClick={onCancel}
                    >
                        <X className="w-4 h-4" />
                    </IconButton>
                </div>
                <ScrollArea className="max-h-[360px] p-2">
                    {loading && (
                        <div className="px-3 py-6 text-xs text-(--fg-tertiary) text-center">
                            {t('screenshot.loadingWindows')}
                        </div>
                    )}
                    {!loading && windows.length === 0 && (
                        <div className="px-3 py-6 text-xs text-(--fg-tertiary) text-center">
                            {t('screenshot.noCapturableWindows')}
                        </div>
                    )}
                    {!loading && windows.map((window) => (
                        <ListRow
                            key={window.id}
                            onClick={() => onSelect(window.id)}
                            className="flex items-start gap-3 px-3 py-2"
                        >
                            <div className="mt-0.5">
                                <Monitor className="w-4 h-4 text-(--accent-ai)" />
                            </div>
                            <div className="flex-1 min-w-0">
                                <div className="text-sm text-(--fg-primary) truncate">
                                    {window.title || window.app_name || t('screenshot.windowUnknown')}
                                </div>
                                <div className="text-xs text-(--fg-tertiary) truncate">
                                    {window.app_name ? `${window.app_name} • ` : ''}{window.width}×{window.height}
                                </div>
                            </div>
                        </ListRow>
                    ))}
                </ScrollArea>
            </Surface>
        </div>
    );
};
