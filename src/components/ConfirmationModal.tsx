'use client';
import React from 'react';
import { ShieldAlert, Check, X } from 'lucide-react';

interface ConfirmationModalProps {
    commands: string[];
    onConfirm: () => void;
    onCancel: () => void;
}

export const ConfirmationModal: React.FC<ConfirmationModalProps> = ({ commands, onConfirm, onCancel }) => {
    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4" style={{ backgroundColor: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}>
            <div
                className="glass-panel max-w-lg w-full shadow-2xl overflow-hidden flex flex-col"
                style={{ borderRadius: 'var(--panel-radius)', boxShadow: 'var(--panel-shadow)' }}
            >
                {/* Header */}
                <div
                    className="px-4 py-3 flex items-center gap-3"
                    style={{ backgroundColor: 'rgba(248,113,113,0.08)', borderBottom: '1px solid rgba(248,113,113,0.2)' }}
                >
                    <ShieldAlert className="w-5 h-5" style={{ color: 'var(--accent-error)' }} />
                    <h2 className="text-sm font-semibold uppercase tracking-wide" style={{ color: 'var(--accent-error)' }}>
                        Execution Approval Required
                    </h2>
                </div>

                {/* Content */}
                <div className="p-5 space-y-4">
                    <p className="text-sm" style={{ color: 'var(--fg-secondary)' }}>
                        The system is requesting to execute the following commands in your workspace:
                    </p>

                    <div
                        className="p-3 font-mono text-xs max-h-60 overflow-y-auto whitespace-pre-wrap"
                        style={{
                            backgroundColor: 'var(--bg-app)',
                            border: '1px solid var(--border-default)',
                            borderRadius: 'calc(var(--panel-radius) / 2)',
                            color: 'var(--fg-primary)',
                        }}
                    >
                        {commands.map((cmd, i) => (
                            <div key={i} className="mb-2 last:mb-0 pl-2" style={{ borderLeft: '2px solid var(--accent-secondary)' }}>
                                {cmd}
                            </div>
                        ))}
                    </div>

                    <div className="text-[11px] italic" style={{ color: 'var(--fg-tertiary)' }}>
                        Review carefully. This action cannot be undone.
                    </div>
                </div>

                {/* Actions */}
                <div
                    className="p-4 flex justify-end gap-3"
                    style={{ borderTop: '1px solid var(--border-subtle)' }}
                >
                    <button
                        onClick={onCancel}
                        className="px-4 py-2 text-xs font-medium rounded transition-colors flex items-center gap-2"
                        style={{ backgroundColor: 'var(--bg-surface)', color: 'var(--fg-secondary)' }}
                    >
                        <X className="w-3.5 h-3.5" />
                        Deny
                    </button>
                    <button
                        onClick={onConfirm}
                        className="px-4 py-2 text-white text-xs font-medium rounded transition-colors flex items-center gap-2"
                        style={{ backgroundColor: 'var(--accent-error)', boxShadow: '0 0 12px rgba(248,113,113,0.3)' }}
                    >
                        <Check className="w-3.5 h-3.5" />
                        Authorize Execution
                    </button>
                </div>
            </div>
        </div>
    );
};
