import React from 'react';
import { X, Monitor } from 'lucide-react';
import type { WindowInfo } from '../types/screenshot';

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
    title = 'Capture Window',
    subtitle = 'Select a window to capture',
    onSelect,
    onCancel,
}) => {
    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div
                className="absolute inset-0 bg-black/70 backdrop-blur-sm"
                onClick={onCancel}
            />
            <div className="relative w-full max-w-xl mx-4 bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-xl shadow-2xl">
                <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                    <div>
                        <div className="text-sm font-semibold text-[var(--fg-primary)]">{title}</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">{subtitle}</div>
                    </div>
                    <button
                        onClick={onCancel}
                        className="p-1 text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>
                <div className="max-h-[360px] overflow-y-auto p-2">
                    {loading && (
                        <div className="px-3 py-6 text-xs text-[var(--fg-tertiary)] text-center">
                            Loading windows...
                        </div>
                    )}
                    {!loading && windows.length === 0 && (
                        <div className="px-3 py-6 text-xs text-[var(--fg-tertiary)] text-center">
                            No capturable windows found.
                        </div>
                    )}
                    {!loading && windows.map((window) => (
                        <button
                            key={window.id}
                            type="button"
                            onClick={() => onSelect(window.id)}
                            className="w-full flex items-start gap-3 px-3 py-2 rounded-lg text-left hover:bg-[var(--bg-surface-hover)] transition"
                        >
                            <div className="mt-0.5">
                                <Monitor className="w-4 h-4 text-[var(--accent-primary)]" />
                            </div>
                            <div className="flex-1 min-w-0">
                                <div className="text-sm text-[var(--fg-primary)] truncate">
                                    {window.title || window.app_name || 'Unknown'}
                                </div>
                                <div className="text-xs text-[var(--fg-tertiary)] truncate">
                                    {window.app_name ? `${window.app_name} • ` : ''}{window.width}×{window.height}
                                </div>
                            </div>
                        </button>
                    ))}
                </div>
            </div>
        </div>
    );
};
