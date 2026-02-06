import React, { useEffect, useRef, useState } from 'react';
import { ChevronDown, Plus, Monitor, Scan } from 'lucide-react';

interface FeatureMenuProps {
    onScreenshot: (mode: 'window' | 'region') => void;
    disabled?: boolean;
}

export const FeatureMenu: React.FC<FeatureMenuProps> = ({ onScreenshot, disabled }) => {
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
                className={`flex items-center gap-1.5 px-2 py-1 rounded border border-transparent bg-transparent hover:bg-[var(--bg-surface-hover)]/30 transition-colors text-[10px] font-medium text-[var(--fg-secondary)] ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
            >
                <Plus className="w-3 h-3 text-[var(--accent-primary)]" />
                <span>Feature Menu</span>
                <ChevronDown className={`w-3 h-3 text-[var(--fg-tertiary)] transition-transform duration-200 ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {isOpen && (
                <div
                    className="absolute bottom-full left-0 mb-1 w-52 py-1 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-xl z-50"
                    style={{ boxShadow: '0 8px 30px rgba(0, 0, 0, 0.4), 0 0 1px rgba(255, 255, 255, 0.1)' }}
                >
                    <div className="px-2 py-1 text-[9px] uppercase tracking-wide text-[var(--fg-tertiary)]">
                        Capture
                    </div>
                    <button
                        type="button"
                        onClick={() => {
                            onScreenshot('window');
                            setIsOpen(false);
                        }}
                        className="w-full flex items-center gap-2 px-2 py-1.5 text-left text-xs text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)] transition-colors"
                    >
                        <Monitor className="w-3.5 h-3.5 text-[var(--accent-primary)]" />
                        <span>Capture Window</span>
                    </button>
                    <button
                        type="button"
                        onClick={() => {
                            onScreenshot('region');
                            setIsOpen(false);
                        }}
                        className="w-full flex items-center gap-2 px-2 py-1.5 text-left text-xs text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)] transition-colors"
                    >
                        <Scan className="w-3.5 h-3.5 text-[var(--accent-primary)]" />
                        <span>Capture Region</span>
                    </button>
                </div>
            )}
        </div>
    );
};
