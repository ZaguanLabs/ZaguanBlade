import React, { useId } from 'react';
import { Check, Pencil, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

export const ScreenshotCapturePreview: React.FC<{
    dataUrl: string;
    name: string;
    onAdd: () => void;
    onEdit: () => void;
    onCancel: () => void;
}> = ({ dataUrl, name, onAdd, onEdit, onCancel }) => {
    const { t } = useTranslation();
    const titleId = useId();
    const descriptionId = useId();

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center bg-(--bg-app)/85 p-6">
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={descriptionId}
                className="flex max-h-[92vh] w-full max-w-6xl flex-col overflow-hidden rounded-(--panel-radius) border border-(--border-focus) bg-(--bg-surface) shadow-(--shadow-xl)"
            >
                <div className="flex shrink-0 items-center justify-between border-b border-(--border-subtle) px-4 py-3">
                    <div className="min-w-0">
                        <div id={titleId} className="text-sm font-semibold text-(--fg-primary)">{t('screenshot.preview.title')}</div>
                        <div id={descriptionId} className="truncate text-xs text-(--fg-tertiary)">{name}</div>
                    </div>
                    <button
                        type="button"
                        onClick={onCancel}
                        aria-label={t('common.close')}
                        className="rounded-[calc(var(--panel-radius)*0.35)] p-1 text-(--fg-tertiary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                    >
                        <X aria-hidden="true" className="h-4 w-4" />
                    </button>
                </div>
                <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto bg-(--bg-app) p-4">
                    <img
                        src={dataUrl}
                        alt={t('screenshot.preview.alt')}
                        draggable={false}
                        className="max-h-[calc(92vh-148px)] max-w-full select-none rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-panel) shadow-(--shadow-md)"
                    />
                </div>
                <div className="flex shrink-0 items-center justify-end gap-2 border-t border-(--border-subtle) px-4 py-3">
                    <button
                        type="button"
                        onClick={onCancel}
                        className="rounded-[calc(var(--panel-radius)*0.45)] px-3 py-1.5 text-xs font-medium text-(--fg-secondary) transition hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                    >
                        {t('common.cancel')}
                    </button>
                    <button
                        type="button"
                        onClick={onEdit}
                        className="inline-flex items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app) px-3 py-1.5 text-xs font-medium text-(--fg-secondary) transition hover:border-[color-mix(in_srgb,var(--accent-ai)_34%,transparent)] hover:text-(--fg-primary)"
                    >
                        <Pencil aria-hidden="true" className="h-3.5 w-3.5" />
                        {t('common.edit')}
                    </button>
                    <button
                        type="button"
                        onClick={onAdd}
                        className="inline-flex items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] bg-(--accent-ai) px-3 py-1.5 text-xs font-medium text-(--fg-bright) transition hover:opacity-90"
                    >
                        <Check aria-hidden="true" className="h-3.5 w-3.5" />
                        {t('common.add')}
                    </button>
                </div>
            </div>
        </div>
    );
};
