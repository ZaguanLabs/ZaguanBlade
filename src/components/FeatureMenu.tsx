import React, { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Plus, Monitor, Scan, ImageUp } from 'lucide-react';

interface FeatureMenuProps {
    onScreenshot: (mode: 'window' | 'region') => void;
    onUploadImage: () => void;
    disabled?: boolean;
}

export const FeatureMenu: React.FC<FeatureMenuProps> = ({ onScreenshot, onUploadImage, disabled }) => {
    const { t } = useTranslation();
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const menuId = useId();

    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };

        if (isOpen) {
            document.addEventListener('mousedown', handleClickOutside);
        }

        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [isOpen]);

    useEffect(() => {
        if (!isOpen) {
            return;
        }

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                setIsOpen(false);
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen]);

    return (
        <div className="relative" ref={containerRef}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen((prev) => !prev)}
                disabled={disabled}
                aria-haspopup="menu"
                aria-expanded={isOpen}
                aria-controls={isOpen ? menuId : undefined}
                className={`flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.4)] border border-(--border-subtle) bg-(--bg-app) px-1.5 py-1 text-[10px] font-medium text-(--fg-secondary) transition-colors ${disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer hover:border-(--accent-ai)/25 hover:bg-(--bg-surface-hover)/50 hover:text-(--fg-primary)'}`}
            >
                <div className="flex h-4 w-4 items-center justify-center rounded-[calc(var(--panel-radius)*0.25)] border border-(--border-subtle) bg-(--bg-surface)">
                    <Plus className="h-2.5 w-2.5 text-(--accent-ai)" />
                </div>
                <span>{t('common.add')}</span>
                <ChevronDown className={`h-2.5 w-2.5 text-(--fg-tertiary) transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div
                    id={menuId}
                    role="menu"
                    aria-label={t('common.add')}
                    className="absolute bottom-full left-0 z-120 mb-1.5 w-44 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-focus) bg-(--bg-surface) py-0.5 shadow-(--shadow-lg)"
                >
                    <div className="px-2 py-1 text-[8px] uppercase tracking-[0.16em] text-(--fg-tertiary)">
                        {t('screenshot.featureMenu.captureSection')}
                    </div>
                    <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                            onScreenshot('window');
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app)">
                            <Monitor className="h-3 w-3 text-(--accent-ai)" />
                        </div>
                        <span>{t('screenshot.featureMenu.captureWindow')}</span>
                    </button>
                    <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                            onScreenshot('region');
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app)">
                            <Scan className="h-3 w-3 text-(--accent-ai)" />
                        </div>
                        <span>{t('screenshot.featureMenu.captureRegion')}</span>
                    </button>
                    <div role="separator" className="my-0.5 border-t border-(--border-subtle)/30" />
                    <div className="px-2 py-1 text-[8px] uppercase tracking-[0.16em] text-(--fg-tertiary)">
                        {t('screenshot.featureMenu.attachSection')}
                    </div>
                    <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                            onUploadImage();
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app)">
                            <ImageUp className="h-3 w-3 text-(--accent-ai)" />
                        </div>
                        <span>{t('screenshot.featureMenu.uploadImage')}</span>
                    </button>
                </div>
            )}
        </div>
    );
};
