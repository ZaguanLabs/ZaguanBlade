'use client';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Terminal, Play, X, Folder, Loader2 } from 'lucide-react';
import type { StructuredAction } from '../types/events';
import { getStructuredActionDescription } from '../utils/localizedEvents';

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
    const { t } = useTranslation();
    const currentAction = actions[0];
    const [pendingIntent, setPendingIntent] = useState<'run' | 'skip' | null>(null);

    useEffect(() => {
        setPendingIntent(null);
    }, [currentAction?.id]);

    if (!currentAction) return null;
    const isWaiting = pendingIntent !== null;
    
    return (
        <div className="my-2 overflow-hidden rounded-[calc(var(--panel-radius)*0.9)] border border-(--border-subtle) bg-(--bg-surface) shadow-(--shadow-sm)">
            <div className="flex items-center gap-2 border-b border-(--border-subtle)/70 bg-(--bg-app)/40 px-2.5 py-2">
                <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-app)/70">
                    {isWaiting ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin text-(--accent-ai)" />
                    ) : (
                        <Terminal className="h-3.5 w-3.5 text-(--fg-secondary)" />
                    )}
                </div>
                <div className="min-w-0">
                    <span className="block text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-secondary)">
                        {isWaiting ? t('approval.commandQueued') : t('approval.commandApproval')}
                    </span>
                    <span className="block text-[11px] text-(--fg-tertiary)">
                        {pendingIntent === 'run'
                            ? t('approval.waitingForExecution')
                            : pendingIntent === 'skip'
                                ? t('approval.skippingCommand')
                                : t('approval.reviewBeforeExecution')}
                    </span>
                </div>
            </div>

            <div className="space-y-2.5 px-2.5 py-2.5">
                <div className="overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-app)/70 shadow-(--shadow-sm)">
                    {currentAction.cwd && (
                        <div className="flex items-center gap-1.5 border-b border-(--border-subtle)/80 bg-(--bg-surface)/50 px-2.5 py-1.5">
                            <Folder className="h-3 w-3 text-(--fg-tertiary)" />
                            <span className="truncate text-[10px] font-mono text-(--fg-tertiary)">
                                {currentAction.cwd}
                            </span>
                        </div>
                    )}

                    <div className="px-2.5 py-2">
                        <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                            {t('approval.pendingAction')}
                        </div>
                        <code className="block break-all text-[11px] font-mono leading-5 text-(--fg-primary)">
                            {getStructuredActionDescription(currentAction)}
                        </code>
                    </div>
                </div>

                <div className="flex items-center gap-1.5">
                    <button
                        disabled={isWaiting}
                        onClick={() => {
                            setPendingIntent('skip');
                            if (onSkipSingle) {
                                onSkipSingle(currentAction.id);
                                return;
                            }
                            onSkip();
                        }}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-app)/70 px-2.5 py-1.5 text-[11px] font-medium text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:cursor-wait disabled:opacity-60"
                    >
                        {pendingIntent === 'skip' ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                            <X className="h-3 w-3" />
                        )}
                        {pendingIntent === 'skip' ? t('approval.skipping') : t('approval.skip')}
                    </button>
                    <button
                        disabled={isWaiting}
                        onClick={() => {
                            setPendingIntent('run');
                            if (onRunSingle) {
                                onRunSingle(currentAction.id);
                                return;
                            }
                            onRun();
                        }}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-[calc(var(--panel-radius)*0.55)] px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:cursor-wait disabled:opacity-80"
                        style={{
                            border: '1px solid color-mix(in srgb, var(--accent-mention) 28%, var(--border-subtle))',
                            backgroundColor: pendingIntent === 'run'
                                ? 'color-mix(in srgb, var(--accent-ai) 18%, var(--bg-app))'
                                : 'color-mix(in srgb, var(--accent-mention) 14%, var(--bg-app))',
                            color: pendingIntent === 'run'
                                ? 'var(--accent-ai)'
                                : 'var(--accent-mention)',
                        }}
                    >
                        {pendingIntent === 'run' ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                            <Play className="h-3 w-3" />
                        )}
                        {pendingIntent === 'run' ? t('approval.waiting') : t('approval.run')}
                    </button>
                </div>
            </div>
        </div>
    );
};
