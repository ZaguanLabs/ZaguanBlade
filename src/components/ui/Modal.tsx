import React, { useState, useEffect, useRef } from 'react';
import { X } from 'lucide-react';

/**
 * InputModal - A reusable modal for single input operations
 * 
 * Used for:
 * - New File name input
 * - New Folder name input
 * - Rename file/folder
 */

interface InputModalProps {
    isOpen: boolean;
    title: string;
    placeholder?: string;
    defaultValue?: string;
    confirmLabel?: string;
    confirmVariant?: 'primary' | 'danger';
    onConfirm: (value: string) => void;
    onCancel: () => void;
}

export const InputModal: React.FC<InputModalProps> = ({
    isOpen,
    title,
    placeholder = '',
    defaultValue = '',
    confirmLabel = 'Confirm',
    confirmVariant = 'primary',
    onConfirm,
    onCancel,
}) => {
    const [value, setValue] = useState(defaultValue);
    const inputRef = useRef<HTMLInputElement>(null);

    // Reset value when modal opens
    useEffect(() => {
        if (isOpen) {
            setValue(defaultValue);
            // Focus input after a short delay for animation
            setTimeout(() => inputRef.current?.focus(), 50);
        }
    }, [isOpen, defaultValue]);

    // Handle keyboard shortcuts
    useEffect(() => {
        if (!isOpen) return;

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onCancel();
            } else if (e.key === 'Enter' && value.trim()) {
                onConfirm(value.trim());
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, value, onConfirm, onCancel]);

    if (!isOpen) return null;

    const confirmButtonClasses = confirmVariant === 'danger'
        ? 'bg-[var(--accent-error)] hover:bg-[#ff869e] text-white'
        : 'bg-[var(--accent-primary)] hover:bg-[#89b4fa] text-white';

    return (
        <div className="fixed inset-0 z-[9999] flex items-center justify-center animate-in fade-in duration-[var(--transition-fast)]">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/70 backdrop-blur-md animate-in fade-in duration-[var(--transition-base)]"
                onClick={onCancel}
            />

            {/* Modal */}
            <div className="relative bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-[var(--shadow-xl)] w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-[var(--transition-base)]">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                    <h2 className="text-sm font-semibold text-[var(--fg-primary)]">{title}</h2>
                    <button
                        onClick={onCancel}
                        className="p-1 text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition-all duration-[var(--transition-fast)]"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-4">
                    <input
                        ref={inputRef}
                        type="text"
                        value={value}
                        onChange={(e) => setValue(e.target.value)}
                        placeholder={placeholder}
                        className="w-full px-3 py-2 bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-md text-sm text-[var(--fg-primary)] placeholder-[var(--fg-tertiary)] focus:outline-none focus:border-[var(--accent-primary)] focus:ring-1 focus:ring-[var(--accent-primary)] transition-all duration-[var(--transition-fast)]"
                    />
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--border-subtle)]">
                    <button
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition-all duration-[var(--transition-fast)]"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={() => value.trim() && onConfirm(value.trim())}
                        disabled={!value.trim()}
                        className={`px-3 py-1.5 text-xs font-medium rounded transition-all duration-[var(--transition-fast)] disabled:opacity-50 disabled:cursor-not-allowed ${confirmButtonClasses}`}
                    >
                        {confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
};

/**
 * ConfirmModal - A reusable modal for confirmation dialogs
 * 
 * Used for:
 * - Delete confirmation
 * - Destructive action warnings
 */

interface ConfirmModalProps {
    isOpen: boolean;
    title: string;
    message: string | React.ReactNode;
    confirmLabel?: string;
    confirmVariant?: 'primary' | 'danger';
    onConfirm: () => void;
    onCancel: () => void;
}

export const ConfirmModal: React.FC<ConfirmModalProps> = ({
    isOpen,
    title,
    message,
    confirmLabel = 'Confirm',
    confirmVariant = 'primary',
    onConfirm,
    onCancel,
}) => {
    // Handle keyboard shortcuts
    useEffect(() => {
        if (!isOpen) return;

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onCancel();
            } else if (e.key === 'Enter') {
                onConfirm();
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onConfirm, onCancel]);

    if (!isOpen) return null;

    const confirmButtonClasses = confirmVariant === 'danger'
        ? 'bg-[var(--accent-error)] hover:bg-[#ff869e] text-white'
        : 'bg-[var(--accent-primary)] hover:bg-[#89b4fa] text-white';

    return (
        <div className="fixed inset-0 z-[9999] flex items-center justify-center animate-in fade-in duration-[var(--transition-fast)]">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/70 backdrop-blur-md animate-in fade-in duration-[var(--transition-base)]"
                onClick={onCancel}
            />

            {/* Modal */}
            <div className="relative bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-[var(--shadow-xl)] w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-[var(--transition-base)]">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
                    <h2 className="text-sm font-semibold text-[var(--fg-primary)]">{title}</h2>
                    <button
                        onClick={onCancel}
                        className="p-1 text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition-all duration-[var(--transition-fast)]"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-4">
                    <div className="text-sm text-[var(--fg-secondary)]">
                        {message}
                    </div>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--border-subtle)]">
                    <button
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition-all duration-[var(--transition-fast)]"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={onConfirm}
                        className={`px-3 py-1.5 text-xs font-medium rounded transition-all duration-[var(--transition-fast)] ${confirmButtonClasses}`}
                    >
                        {confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
};
