import React, { useEffect, useRef, useState } from 'react';
import { ChevronDown, Plus, Monitor, Scan, ImageUp } from 'lucide-react';

interface FeatureMenuProps {
    onScreenshot: (mode: 'window' | 'region') => void;
    onUploadImage: () => void;
    disabled?: boolean;
}

export const FeatureMenu: React.FC<FeatureMenuProps> = ({ onScreenshot, onUploadImage, disabled }) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);

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

    return (
        <div className="relative" ref={containerRef}>
            <button
                type="button"
                onClick={() => !disabled && setIsOpen((prev) => !prev)}
                disabled={disabled}
                className={`flex items-center gap-1 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)] px-1.5 py-1 text-[10px] font-medium text-[var(--fg-secondary)] transition-colors ${disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer hover:border-[var(--accent-primary)]/25 hover:bg-[var(--bg-surface-hover)]/50 hover:text-[var(--fg-primary)]'}`}
            >
                <div className="flex h-4 w-4 items-center justify-center rounded-sm border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
                    <Plus className="h-2.5 w-2.5 text-[var(--accent-primary)]" />
                </div>
                <span>Add</span>
                <ChevronDown className={`h-2.5 w-2.5 text-[var(--fg-tertiary)] transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div
                    className="absolute bottom-full left-0 z-[120] mb-1.5 w-44 overflow-hidden rounded-lg border border-[var(--border-focus)] bg-[var(--bg-surface)] py-0.5 shadow-[0_16px_40px_rgba(0,0,0,0.35)]"
                    style={{ boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4), 0 0 1px rgba(255, 255, 255, 0.1)' }}
                >
                    <div className="px-2 py-1 text-[8px] uppercase tracking-[0.16em] text-[var(--fg-tertiary)]">
                        Capture
                    </div>
                    <button
                        type="button"
                        onClick={() => {
                            onScreenshot('window');
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-[var(--fg-secondary)] transition-colors hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                            <Monitor className="h-3 w-3 text-[var(--accent-primary)]" />
                        </div>
                        <span>Capture Window</span>
                    </button>
                    <button
                        type="button"
                        onClick={() => {
                            onScreenshot('region');
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-[var(--fg-secondary)] transition-colors hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                            <Scan className="h-3 w-3 text-[var(--accent-primary)]" />
                        </div>
                        <span>Capture Region</span>
                    </button>
                    <div className="my-0.5 border-t border-[var(--border-subtle)]/30" />
                    <div className="px-2 py-1 text-[8px] uppercase tracking-[0.16em] text-[var(--fg-tertiary)]">
                        Attach
                    </div>
                    <button
                        type="button"
                        onClick={() => {
                            onUploadImage();
                            setIsOpen(false);
                        }}
                        className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[10px] text-[var(--fg-secondary)] transition-colors hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]"
                    >
                        <div className="flex h-6 w-6 items-center justify-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                            <ImageUp className="h-3 w-3 text-[var(--accent-primary)]" />
                        </div>
                        <span>Upload Image</span>
                    </button>
                </div>
            )}
        </div>
    );
};
