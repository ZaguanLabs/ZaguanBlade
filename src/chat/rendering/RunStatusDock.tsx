import React from 'react';
import { Loader2, Square } from 'lucide-react';
import type { ToolActivityState } from '../../types/chat';

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
        <div className="shrink-0 border-t border-(--border-subtle) bg-(--bg-app) px-3 py-2">
            <div className="flex items-center gap-2 rounded-lg border border-(--border-subtle) bg-(--bg-surface)/70 px-3 py-2 text-[12px] text-(--fg-secondary)">
                {error ? <Square className="h-3.5 w-3.5 text-red-300" /> : <Loader2 className="h-3.5 w-3.5 animate-spin text-(--accent-primary)" />}
                <span className="min-w-0 truncate">{label}</span>
            </div>
        </div>
    );
};
