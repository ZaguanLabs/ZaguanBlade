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
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
            <div className="bg-zinc-950 border border-red-500/30 rounded-sm max-w-lg w-full shadow-2xl overflow-hidden flex flex-col">
                {/* Header */}
                <div className="bg-red-500/10 px-4 py-3 border-b border-red-500/20 flex items-center gap-3">
                    <ShieldAlert className="w-5 h-5 text-red-500" />
                    <h2 className="text-sm font-semibold text-red-100 uppercase tracking-wide">
                        Execution Approval Required
                    </h2>
                </div>

                {/* Content */}
                <div className="p-5 space-y-4">
                    <p className="text-zinc-400 text-sm">
                        The system is requesting to execute the following commands in your workspace:
                    </p>

                    <div className="bg-black border border-zinc-800 p-3 rounded-sm font-mono text-xs text-zinc-300 max-h-60 overflow-y-auto whitespace-pre-wrap">
                        {commands.map((cmd, i) => (
                            <div key={i} className="mb-2 last:mb-0 border-l-2 border-emerald-500/50 pl-2">
                                {cmd}
                            </div>
                        ))}
                    </div>

                    <div className="text-[11px] text-zinc-500 italic">
                        Review carefully. This action cannot be undone.
                    </div>
                </div>

                {/* Actions */}
                <div className="p-4 bg-zinc-900/50 border-t border-zinc-900 flex justify-end gap-3">
                    <button
                        onClick={onCancel}
                        className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 text-xs font-medium rounded-sm transition-colors flex items-center gap-2"
                    >
                        <X className="w-3.5 h-3.5" />
                        Deny
                    </button>
                    <button
                        onClick={onConfirm}
                        className="px-4 py-2 bg-red-600 hover:bg-red-500 text-white text-xs font-medium rounded-sm transition-colors flex items-center gap-2 shadow-[0_0_10px_rgba(220,38,38,0.3)]"
                    >
                        <Check className="w-3.5 h-3.5" />
                        Authorize Execution
                    </button>
                </div>
            </div>
        </div>
    );
};
