import React, { useState } from 'react';
import { ChevronDown, ChevronRight, Terminal } from 'lucide-react';
import type { ChatWorkEntry } from '../../utils/chatTimeline';

function workEntryToneClass(entry: ChatWorkEntry): string {
    if (entry.status === 'executing' || entry.status === 'pending') {
        return 'bg-(--accent-ai)';
    }
    if (entry.tone === 'error') {
        return 'bg-(--state-danger)';
    }
    return 'bg-(--accent-mention)';
}

interface WorkLogTimelineRowProps {
    entries: ChatWorkEntry[];
    showDetails: boolean;
    detailsLockedOpen: boolean;
    onToggleDetails: () => void;
}

export const WorkLogTimelineRow: React.FC<WorkLogTimelineRowProps> = React.memo(({ entries, showDetails, detailsLockedOpen, onToggleDetails }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const visibleEntries = isExpanded ? entries : entries.slice(0, 4);
    const hiddenCount = entries.length - visibleEntries.length;

    return (
        <div className="px-4 pb-2">
            <div className="ml-8 rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/45 px-3 py-2">
                <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 text-left"
                    onClick={() => setIsExpanded((value) => !value)}
                    aria-expanded={isExpanded}
                >
                    <span className="flex min-w-0 items-center gap-2">
                        <Terminal className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                        <span className="text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                            Work log ({entries.length})
                        </span>
                        {!isExpanded && (
                            <span className="min-w-0 truncate text-[10px] text-(--fg-secondary)">
                                {entries.slice(0, 2).map((entry) => entry.label).join(', ')}
                            </span>
                        )}
                    </span>
                    {isExpanded ? (
                        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                    ) : (
                        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                    )}
                </button>
                {(isExpanded || entries.length > 4) && (
                    <div className="mt-1.5 space-y-1">
                        {visibleEntries.map((entry) => (
                            <div key={entry.id} className="flex min-w-0 items-center gap-2 rounded-[calc(var(--panel-radius)*0.35)] px-1 py-0.5">
                                <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${workEntryToneClass(entry)}`} />
                                <span className="shrink-0 text-[10px] font-medium text-(--fg-secondary)">
                                    {entry.label}
                                </span>
                                {entry.detail && (
                                    <span className="min-w-0 truncate text-[10px] font-mono text-(--fg-tertiary)" title={entry.detail}>
                                        {entry.detail}
                                    </span>
                                )}
                            </div>
                        ))}
                        {!isExpanded && hiddenCount > 0 && (
                            <div className="px-1 text-[10px] text-(--fg-tertiary)">
                                +{hiddenCount} more
                            </div>
                        )}
                    </div>
                )}
                <div className="mt-1.5 flex items-center justify-end">
                    <button
                        type="button"
                        className="rounded-[calc(var(--panel-radius)*0.35)] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-(--fg-tertiary) transition-colors hover:text-(--fg-secondary) disabled:cursor-default disabled:opacity-60"
                        onClick={onToggleDetails}
                        disabled={detailsLockedOpen}
                        aria-pressed={showDetails || detailsLockedOpen}
                    >
                        {detailsLockedOpen ? 'Details visible' : showDetails ? 'Hide details' : 'Show details'}
                    </button>
                </div>
            </div>
        </div>
    );
});

WorkLogTimelineRow.displayName = 'WorkLogTimelineRow';
