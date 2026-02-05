'use client';
import React from 'react';
import { Terminal, Play, X, Folder, PlayCircle, XCircle } from 'lucide-react';
import type { StructuredAction } from '../types/events';

interface CommandApprovalCardProps {
    actions: StructuredAction[];
    onRun: () => void;
    onSkip: () => void;
    onRunSingle?: (callId: string) => void;
    onSkipSingle?: (callId: string) => void;
}

export const CommandApprovalCard: React.FC<CommandApprovalCardProps> = ({
    actions,
    onRun,
    onSkip,
    onRunSingle,
    onSkipSingle,
}) => {
    const hasIndividualControls = onRunSingle && onSkipSingle && actions.length > 1;
    
    return (
        <div className="border border-blue-500/30 rounded-lg overflow-hidden bg-blue-950/10 my-2">
            {/* Header */}
            <div className="flex items-center gap-2 px-3 py-2 bg-blue-900/20 border-b border-blue-500/20">
                <Terminal className="w-4 h-4 text-blue-400" />
                <span className="text-xs font-medium text-blue-300">
                    Command Approval Required ({actions.length} command{actions.length > 1 ? 's' : ''})
                </span>
            </div>

            {/* Command List */}
            <div className="px-3 py-2 space-y-2">
                {actions.map((action, idx) => (
                    <div key={action.id || idx} className="bg-[#0d1117] border border-zinc-800 rounded overflow-hidden">
                        {/* Working Directory */}
                        {action.cwd && (
                            <div className="flex items-center gap-2 px-3 py-1.5 bg-zinc-900/50 border-b border-zinc-800">
                                <Folder className="w-3 h-3 text-zinc-500" />
                                <span className="text-[10px] font-mono text-zinc-500">
                                    {action.cwd}
                                </span>
                            </div>
                        )}
                        
                        {/* Command with individual controls */}
                        <div className="flex items-center gap-2 px-3 py-2">
                            <code className="flex-1 text-xs font-mono text-zinc-200 break-all">
                                {action.description}
                            </code>
                            
                            {/* Individual Run/Skip buttons */}
                            {hasIndividualControls && (
                                <div className="flex items-center gap-1 shrink-0">
                                    <button
                                        onClick={() => onSkipSingle(action.id)}
                                        className="p-1 rounded text-zinc-500 hover:text-red-400 hover:bg-red-900/20 transition-colors"
                                        title="Skip this command"
                                    >
                                        <XCircle className="w-4 h-4" />
                                    </button>
                                    <button
                                        onClick={() => onRunSingle(action.id)}
                                        className="p-1 rounded text-zinc-500 hover:text-green-400 hover:bg-green-900/20 transition-colors"
                                        title="Run this command"
                                    >
                                        <PlayCircle className="w-4 h-4" />
                                    </button>
                                </div>
                            )}
                        </div>
                    </div>
                ))}
            </div>

            {/* Batch Action Buttons */}
            <div className="flex items-center gap-2 px-3 py-2 bg-zinc-900/30 border-t border-blue-500/20">
                <button
                    onClick={onSkip}
                    className="flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50 transition-colors"
                >
                    <X className="w-3.5 h-3.5" />
                    Skip All
                </button>
                <button
                    onClick={onRun}
                    className="flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-blue-600 hover:bg-blue-500 text-white transition-colors"
                >
                    <Play className="w-3.5 h-3.5" />
                    Run All
                </button>
            </div>
        </div>
    );
};
