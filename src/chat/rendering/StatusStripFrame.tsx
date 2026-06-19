import React from 'react';

interface StatusStripFrameProps {
    label: string;
    count?: number;
    tone?: 'default' | 'ai' | 'danger';
    outerBackground?: boolean;
    children: React.ReactNode;
}

export const StatusStripFrame: React.FC<StatusStripFrameProps> = ({
    label,
    count,
    tone = 'default',
    outerBackground = true,
    children,
}) => {
    const toneClass = tone === 'danger'
        ? 'text-(--state-danger)'
        : tone === 'ai'
            ? 'text-(--accent-ai)'
            : 'text-(--fg-tertiary)';
    const outerClassName = outerBackground
        ? 'shrink-0 bg-(--bg-panel) px-2 pt-1.5'
        : 'shrink-0 px-2 pt-1.5';

    return (
        <div className={outerClassName}>
            <div className="rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/55 px-2.5 py-2 shadow-(--shadow-sm)">
                <div className="mb-1.5 flex items-center gap-2 text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                    <span className={toneClass}>{label}</span>
                    {typeof count === 'number' && (
                        <span className="rounded-full border border-(--border-subtle) bg-(--bg-app) px-1.5 py-0.5 text-[9px] leading-none text-(--fg-tertiary)">
                            {count}
                        </span>
                    )}
                </div>
                <div className="space-y-1">
                    {children}
                </div>
            </div>
        </div>
    );
};
