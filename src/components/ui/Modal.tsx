import React, { useState, useEffect, useRef, useId } from 'react';
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
    const titleId = useId();
    const inputId = useId();

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
                aria-hidden="true"
                className="absolute inset-0 bg-black/70 animate-in fade-in duration-(--transition-base)"
                onClick={onCancel}
            />

            {/* Modal */}
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                className="relative bg-(--surface-overlay) border border-(--focus-ring) rounded-(--radius-dialog) shadow-(--shadow-dialog) w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-(--transition-base) overflow-hidden"
            >
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-(--separator-subtle)">
                    <h2 id={titleId} className="text-sm font-semibold text-(--fg-primary)">{title}</h2>
                    <button
                        type="button"
                        aria-label="Close"
                        onClick={onCancel}
                        className="p-1 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--row-hover) rounded-(--radius-control) transition-all duration-(--transition-fast)"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-4">
                    <input
                        id={inputId}
                        ref={inputRef}
                        type="text"
                        aria-label={title}
                        value={value}
                        onChange={(e) => setValue(e.target.value)}
                        placeholder={placeholder}
                        className="w-full px-3 py-2 bg-(--surface-app) border border-(--separator-subtle) rounded-(--radius-control) text-sm text-(--fg-primary) placeholder-(--fg-tertiary) focus:outline-none focus:border-(--focus-ring) focus:ring-1 focus:ring-(--focus-ring) transition-all duration-(--transition-fast)"
                    />
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-(--separator-subtle)">
                    <button
                        type="button"
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--row-hover) rounded-(--radius-control) transition-all duration-(--transition-fast)"
                    >
                        Cancel
                    </button>
                    <button
                        type="button"
                        onClick={() => value.trim() && onConfirm(value.trim())}
                        disabled={!value.trim()}
                        className={`px-3 py-1.5 text-xs font-medium rounded-(--radius-control) transition-all duration-(--transition-fast) disabled:opacity-50 disabled:cursor-not-allowed ${confirmButtonClasses}`}
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
    const titleId = useId();
    const messageId = useId();

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
                aria-hidden="true"
                className="absolute inset-0 bg-black/70 animate-in fade-in duration-(--transition-base)"
                onClick={onCancel}
            />

            {/* Modal */}
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={messageId}
                className="relative bg-(--surface-overlay) border border-(--focus-ring) rounded-(--radius-dialog) shadow-(--shadow-dialog) w-full max-w-md mx-4 animate-in fade-in zoom-in-95 duration-(--transition-base) overflow-hidden"
            >
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-(--separator-subtle)">
                    <h2 id={titleId} className="text-sm font-semibold text-(--fg-primary)">{title}</h2>
                    <button
                        type="button"
                        aria-label="Close"
                        onClick={onCancel}
                        className="p-1 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--row-hover) rounded-(--radius-control) transition-all duration-(--transition-fast)"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-4">
                    <div id={messageId} className="text-sm text-(--fg-secondary)">
                        {message}
                    </div>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-(--separator-subtle)">
                    <button
                        type="button"
                        onClick={onCancel}
                        className="px-3 py-1.5 text-xs font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--row-hover) rounded-(--radius-control) transition-all duration-(--transition-fast)"
                    >
                        Cancel
                    </button>
                    <button
                        type="button"
                        onClick={onConfirm}
                        className={`px-3 py-1.5 text-xs font-medium rounded-(--radius-control) transition-all duration-(--transition-fast) ${confirmButtonClasses}`}
                    >
                        {confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
};
