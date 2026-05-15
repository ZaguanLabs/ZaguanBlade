import React from 'react';
import { Loader2, Square } from 'lucide-react';
import type { ToolActivityState } from '../../types/chat';
import { StatusStripFrame } from './StatusStripFrame';

interface RunStatusDockProps {
    loading: boolean;
    waitingForApproval: boolean;
    error: string | null;
    toolActivity?: ToolActivityState | null;
}

export const RunStatusDock: React.FC<RunStatusDockProps> = ({
    loading,
    waitingForApproval,
    error,
    toolActivity,
}) => {
    if (!loading && !waitingForApproval && !error) {
        return null;
    }

    const label = error
        ? error
        : waitingForApproval
            ? 'Waiting for approval'
            : toolActivity?.action ?? 'Assistant is working';

    return (
        <StatusStripFrame label={error ? 'Attention' : 'Run'} tone={error ? 'danger' : 'ai'}>
            <div className="flex items-center gap-2 text-[12px] text-(--fg-secondary)">
                {error ? <Square className="h-3.5 w-3.5 text-(--state-danger)" /> : <Loader2 className="h-3.5 w-3.5 animate-spin text-(--accent-ai)" />}
                <span className="min-w-0 truncate">{label}</span>
            </div>
        </StatusStripFrame>
    );
};
