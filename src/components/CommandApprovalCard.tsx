'use client';
import React from 'react';
import { Terminal, Play, X, Folder } from 'lucide-react';
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
}) => {
    // Only show the first pending action - one at a time
    const currentAction = actions[0];
    if (!currentAction) return null;
    
    return (
        <div className="my-2 overflow-hidden rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/88 shadow-[0_10px_28px_rgba(0,0,0,0.18)] backdrop-blur-sm">
            <div className="flex items-center gap-2 border-b border-[var(--border-subtle)]/70 bg-white/[0.02] px-2.5 py-2">
                <div className="flex h-6 w-6 items-center justify-center rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-app)]/70">
                    <Terminal className="h-3.5 w-3.5 text-[var(--fg-secondary)]" />
                </div>
                <div className="min-w-0">
                    <span className="block text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--fg-secondary)]">
                        Command approval
                    </span>
                    <span className="block text-[11px] text-[var(--fg-tertiary)]">
                        Review before execution
                    </span>
                </div>
            </div>

            <div className="space-y-2.5 px-2.5 py-2.5">
                <div className="overflow-hidden rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-app)]/70 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]">
                    {currentAction.cwd && (
                        <div className="flex items-center gap-1.5 border-b border-[var(--border-subtle)]/80 bg-black/10 px-2.5 py-1.5">
                            <Folder className="h-3 w-3 text-[var(--fg-tertiary)]" />
                            <span className="truncate text-[10px] font-mono text-[var(--fg-tertiary)]">
                                {currentAction.cwd}
                            </span>
                        </div>
                    )}

                    <div className="px-2.5 py-2">
                        <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-[0.16em] text-[var(--fg-tertiary)]">
                            Pending action
                        </div>
                        <code className="block break-all text-[11px] font-mono leading-5 text-[var(--fg-primary)]">
                            {currentAction.description}
                        </code>
                    </div>
                </div>

                <div className="flex items-center gap-1.5">
                    <button
                        onClick={onSkip}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-app)]/70 px-2.5 py-1.5 text-[11px] font-medium text-[var(--fg-secondary)] transition-colors hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)]"
                    >
                        <X className="h-3 w-3" />
                        Skip
                    </button>
                    <button
                        onClick={onRun}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1.5 text-[11px] font-semibold text-emerald-200 transition-colors hover:bg-emerald-500/14 hover:text-emerald-100"
                    >
                        <Play className="h-3 w-3" />
                        Run
                    </button>
                </div>
            </div>
        </div>
    );
};
