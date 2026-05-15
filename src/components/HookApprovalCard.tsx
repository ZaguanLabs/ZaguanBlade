'use client';
import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Braces, Check, Loader2, ShieldAlert, X } from 'lucide-react';
import type { HookApprovalRequest } from '../types/chat';

interface HookApprovalCardProps {
    request: HookApprovalRequest;
    onApprove: () => void;
    onDeny: () => void;
}

export const HookApprovalCard: React.FC<HookApprovalCardProps> = ({
    request,
    onApprove,
    onDeny,
}) => {
    const { t } = useTranslation();
    const [pendingIntent, setPendingIntent] = useState<'approve' | 'deny' | null>(null);

    useEffect(() => {
        setPendingIntent(null);
    }, [request.approvalId]);

    const isWaiting = pendingIntent !== null;
    const argumentsPreview = useMemo(() => {
        try {
            return JSON.stringify(request.arguments, null, 2);
        } catch {
            return String(request.arguments ?? '');
        }
    }, [request.arguments]);

    return (
        <div className="my-2 overflow-hidden rounded-[calc(var(--panel-radius)*0.9)] border border-(--border-subtle) bg-(--bg-surface) shadow-(--shadow-sm)">
            <div className="flex items-center gap-2 border-b border-(--border-subtle)/70 bg-(--bg-app)/40 px-2.5 py-2">
                <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-app)/70">
                    {isWaiting ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin text-(--accent-ai)" />
                    ) : (
                        <ShieldAlert className="h-3.5 w-3.5 text-(--accent-warning)" />
                    )}
                </div>
                <div className="min-w-0">
                    <span className="block text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-secondary)">
                        {t('approval.hookApproval')}
                    </span>
                    <span className="block text-[11px] text-(--fg-tertiary)">
                        {pendingIntent === 'approve'
                            ? t('approval.waiting')
                            : pendingIntent === 'deny'
                                ? t('approval.skipping')
                                : (request.message || t('approval.hookApprovalDetail'))}
                    </span>
                </div>
            </div>

            <div className="space-y-2.5 px-2.5 py-2.5">
                <div className="overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-app)/70 shadow-(--shadow-sm)">
                    <div className="flex items-center gap-1.5 border-b border-(--border-subtle)/80 bg-(--bg-surface)/50 px-2.5 py-1.5">
                        <Braces className="h-3 w-3 text-(--fg-tertiary)" />
                        <span className="truncate text-[10px] font-mono text-(--fg-tertiary)">
                            {request.toolName}
                        </span>
                        {request.ruleName && (
                            <span className="ml-auto truncate text-[10px] text-(--fg-tertiary)">
                                {request.ruleName}
                            </span>
                        )}
                    </div>

                    <div className="px-2.5 py-2">
                        <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                            {t('approval.pendingAction')}
                        </div>
                        <code className="block break-all whitespace-pre-wrap text-[11px] font-mono leading-5 text-(--fg-primary)">
                            {argumentsPreview}
                        </code>
                    </div>
                </div>

                <div className="flex items-center gap-1.5">
                    <button
                        disabled={isWaiting}
                        onClick={() => {
                            setPendingIntent('deny');
                            onDeny();
                        }}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-[calc(var(--panel-radius)*0.55)] border border-(--border-subtle) bg-(--bg-app)/70 px-2.5 py-1.5 text-[11px] font-medium text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary) disabled:cursor-wait disabled:opacity-60"
                    >
                        {pendingIntent === 'deny' ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                            <X className="h-3 w-3" />
                        )}
                        {pendingIntent === 'deny' ? t('approval.skipping') : t('approval.deny')}
                    </button>
                    <button
                        disabled={isWaiting}
                        onClick={() => {
                            setPendingIntent('approve');
                            onApprove();
                        }}
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-[calc(var(--panel-radius)*0.55)] px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:cursor-wait disabled:opacity-80"
                        style={{
                            border: '1px solid color-mix(in srgb, var(--accent-mention) 28%, var(--border-subtle))',
                            backgroundColor: pendingIntent === 'approve'
                                ? 'color-mix(in srgb, var(--accent-ai) 18%, var(--bg-app))'
                                : 'color-mix(in srgb, var(--accent-mention) 14%, var(--bg-app))',
                            color: pendingIntent === 'approve'
                                ? 'var(--accent-ai)'
                                : 'var(--accent-mention)',
                        }}
                    >
                        {pendingIntent === 'approve' ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                            <Check className="h-3 w-3" />
                        )}
                        {pendingIntent === 'approve' ? t('approval.waiting') : t('approval.approve')}
                    </button>
                </div>
            </div>
        </div>
    );
};
