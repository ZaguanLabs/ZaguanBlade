'use client';
import React, { useState } from 'react';
import { Terminal, Play, X, AlertTriangle, Folder, Settings, Check } from 'lucide-react';
import type { StructuredAction } from '../types/events';

interface CommandApprovalModalProps {
    actions: StructuredAction[];
    onApproveOnce: () => void;
    onApproveAlways: () => void;
    onReject: () => void;
}

export const CommandApprovalModal: React.FC<CommandApprovalModalProps> = ({
    actions,
    onApproveOnce,
    onApproveAlways,
    onReject,
}) => {
    const [isExecuting, setIsExecuting] = useState(false);

    const handleApproveOnce = async () => {
        setIsExecuting(true);
        await onApproveOnce();
        // Modal will be closed by parent when commands complete
    };

    const handleApproveAlways = async () => {
        setIsExecuting(true);
        await onApproveAlways();
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-[#1e1e1e] border border-[#3e3e42] rounded-lg shadow-2xl max-w-2xl w-full mx-4 animate-in zoom-in-95 duration-200">
                {/* Header */}
                <div className="flex items-center gap-3 px-6 py-4 border-b border-[#3e3e42]">
                    <div className="p-2 rounded-lg bg-amber-500/10">
                        <AlertTriangle className="w-5 h-5 text-amber-500" />
                    </div>
                    <div className="flex-1">
                        <h2 className="text-lg font-semibold text-white">Tool Execution Approval</h2>
                        <p className="text-sm text-zinc-400 mt-0.5">
                            The AI wants to execute {actions.length} action{actions.length > 1 ? 's' : ''} on your system
                        </p>
                    </div>
                    <button
                        onClick={onReject}
                        disabled={isExecuting}
                        className="p-2 rounded-lg hover:bg-white/5 text-zinc-400 hover:text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Command List */}
                <div className="px-6 py-4 max-h-[400px] overflow-y-auto">
                    <div className="space-y-3">
                        {actions.map((action, idx) => (
                            <div
                                key={idx}
                                className="bg-[#252526] border border-[#3e3e42] rounded-lg overflow-hidden"
                            >
                                {/* Working Directory */}
                                {action.cwd && (
                                    <div className={`flex items-center gap-2 px-4 py-2 border-b border-[#3e3e42] ${action.cwd_outside_workspace ? 'bg-red-500/10' : 'bg-[#2d2d2d]'}`}>
                                        <Folder className="w-3.5 h-3.5 text-blue-400" />
                                        <span className="text-xs font-mono text-zinc-400">
                                            {action.cwd}
                                        </span>
                                    </div>
                                )}
                                
                                {/* Command / Tool Description */}
                                <div className="px-4 py-3">
                                    <div className="flex items-start gap-3">
                                        {action.is_generic_tool ? (
                                            <Settings className="w-4 h-4 text-blue-400 mt-0.5 shrink-0" />
                                        ) : (
                                            <Terminal className="w-4 h-4 text-emerald-400 mt-0.5 shrink-0" />
                                        )}
                                        <div className="flex-1">
                                            {action.root_command && !action.is_generic_tool && (
                                                <div className="text-[11px] font-mono text-zinc-400 mb-1">
                                                    root: {action.root_command}
                                                </div>
                                            )}
                                            <code className="text-sm font-mono text-white break-all">
                                                {action.description}
                                            </code>
                                            {action.cwd_outside_workspace && (
                                                <div className="text-[11px] text-red-300 mt-2">
                                                    Warning: cwd resolves outside the workspace
                                                </div>
                                            )}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        ))}
                    </div>

                    {/* Warning Message */}
                    <div className="mt-4 p-4 bg-amber-500/5 border border-amber-500/20 rounded-lg">
                        <div className="flex gap-3">
                            <AlertTriangle className="w-4 h-4 text-amber-500 shrink-0 mt-0.5" />
                            <div className="text-sm text-amber-200/80">
                                <p className="font-medium mb-1">Review carefully before approving</p>
                                <p className="text-xs text-amber-200/60">
                                    These actions will be executed with your user permissions. 
                                    Make sure you understand what they do.
                                </p>
                            </div>
                        </div>
                    </div>
                </div>

                {/* Footer Actions */}
                <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-[#3e3e42] bg-[#252526]">
                    <button
                        onClick={onReject}
                        disabled={isExecuting}
                        className="px-4 py-2 rounded-lg text-sm font-medium text-zinc-300 hover:text-white hover:bg-white/5 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        Reject
                    </button>
                    <button
                        onClick={handleApproveAlways}
                        disabled={isExecuting}
                        className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-blue-600 hover:bg-blue-500 text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isExecuting ? (
                            <>
                                <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                                Executing...
                            </>
                        ) : (
                            <>
                                <Check className="w-4 h-4" />
                                Approve Always (Session)
                            </>
                        )}
                    </button>

                    <button
                        onClick={handleApproveOnce}
                        disabled={isExecuting}
                        className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-emerald-600 hover:bg-emerald-500 text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isExecuting ? (
                            <>
                                <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                                Executing...
                            </>
                        ) : (
                            <>
                                <Play className="w-4 h-4" />
                                Approve Once
                            </>
                        )}
                    </button>
                </div>
            </div>
        </div>
    );
};
