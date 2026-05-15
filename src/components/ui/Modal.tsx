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
        ? 'bg-(--state-danger) text-(--fg-bright) hover:brightness-110'
        : 'bg-(--accent-ai) text-(--fg-bright) hover:brightness-110';

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center animate-in fade-in duration-(--transition-fast)">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/70 animate-in fade-in duration-(--transition-base)"
                onClick={onCancel}
            />

            {/* Modal */}
            <div className="relative bg-(--bg-surface) border border-(--border-focus) rounded-(--panel-radius) shadow-(--shadow-xl) w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-(--transition-base) overflow-hidden">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-(--border-subtle)">
                    <h2 className="text-sm font-semibold text-(--fg-primary)">{title}</h2>
                    <button
                        onClick={onCancel}
                        className="p-1 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) rounded-[calc(var(--panel-radius)*0.5)] transition-all duration-(--transition-fast)"
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
                        className="w-full px-3 py-2 bg-(--bg-app) border border-(--border-subtle) rounded-[calc(var(--panel-radius)*0.65)] text-sm text-(--fg-primary) placeholder-(--fg-tertiary) focus:outline-none focus:border-(--accent-ai) focus:ring-1 focus:ring-(--accent-ai) transition-all duration-(--transition-fast)"
                    />
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-(--border-subtle)">
                    <button
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) rounded-[calc(var(--panel-radius)*0.55)] transition-all duration-(--transition-fast)"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={() => value.trim() && onConfirm(value.trim())}
                        disabled={!value.trim()}
                        className={`px-3 py-1.5 text-xs font-medium rounded-[calc(var(--panel-radius)*0.55)] transition-all duration-(--transition-fast) disabled:opacity-50 disabled:cursor-not-allowed ${confirmButtonClasses}`}
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
        ? 'bg-(--state-danger) text-(--fg-bright) hover:brightness-110'
        : 'bg-(--accent-ai) text-(--fg-bright) hover:brightness-110';

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center animate-in fade-in duration-(--transition-fast)">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/70 animate-in fade-in duration-(--transition-base)"
                onClick={onCancel}
            />

            {/* Modal */}
            <div className="relative bg-(--bg-surface) border border-(--border-focus) rounded-(--panel-radius) shadow-(--shadow-xl) w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-(--transition-base) overflow-hidden">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-(--border-subtle)">
                    <h2 className="text-sm font-semibold text-(--fg-primary)">{title}</h2>
                    <button
                        onClick={onCancel}
                        className="p-1 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) rounded-[calc(var(--panel-radius)*0.5)] transition-all duration-(--transition-fast)"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-4">
                    <div className="text-sm text-(--fg-secondary)">
                        {message}
                    </div>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-(--border-subtle)">
                    <button
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) rounded-[calc(var(--panel-radius)*0.55)] transition-all duration-(--transition-fast)"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={onConfirm}
                        className={`px-3 py-1.5 text-xs font-medium rounded-[calc(var(--panel-radius)*0.55)] transition-all duration-(--transition-fast) ${confirmButtonClasses}`}
                    >
                        {confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
};
