import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ChevronRight, Copy, Check, ExternalLink, GitCommit } from 'lucide-react';

export interface GitLogEntry {
    hash: string;
    shortHash: string;
    authorName: string;
    authorEmail: string;
    date: string;
    relativeDate: string;
    subject: string;
    refs: string[];
    parents: string[];
    insertions: number;
    deletions: number;
    filesChanged: number;
}

// Assign stable colors to branches based on ref patterns
const GRAPH_COLORS = [
    '#f97316', // orange
    '#a855f7', // purple
    '#3b82f6', // blue
    '#10b981', // emerald
    '#ec4899', // pink
    '#eab308', // yellow
    '#06b6d4', // cyan
    '#ef4444', // red
];

function getCommitColor(entry: GitLogEntry, index: number): string {
    // HEAD commit gets orange (like the screenshot)
    if (entry.refs.some(r => r.includes('HEAD'))) return GRAPH_COLORS[0];
    // Merge commits get purple
    if (entry.parents.length > 1) return GRAPH_COLORS[1];
    // Use index-based cycling for the graph line
    return GRAPH_COLORS[index % GRAPH_COLORS.length];
}

function getGravatarUrl(email: string, size = 32): string {
    // Simple hash for gravatar - we'll use a deterministic color avatar instead
    // since we can't do MD5 in the browser without a library
    return `https://www.gravatar.com/avatar/?d=mp&s=${size}`;
}

interface CommitTooltipProps {
    entry: GitLogEntry;
    remoteUrl: string | null;
    onCopy: (hash: string) => void;
    copied: boolean;
}

const CommitTooltip: React.FC<CommitTooltipProps> = ({ entry, remoteUrl, onCopy, copied }) => {
    const formattedDate = (() => {
        try {
            const d = new Date(entry.date);
            return d.toLocaleDateString('en-US', {
                year: 'numeric',
                month: 'long',
                day: 'numeric',
                hour: 'numeric',
                minute: '2-digit',
                hour12: true,
            });
        } catch {
            return entry.date;
        }
    })();

    const commitUrl = remoteUrl ? `${remoteUrl}/commit/${entry.hash}` : null;

    return (
        <div className="min-w-[300px] max-w-[420px] p-3 text-[11px]">
            {/* Author line */}
            <div className="flex items-center gap-2 mb-2">
                <img
                    src={getGravatarUrl(entry.authorEmail)}
                    alt=""
                    className="w-6 h-6 rounded-full bg-[var(--bg-surface)]"
                    onError={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                />
                <div className="flex items-center gap-1.5 flex-wrap">
                    <span className="font-semibold text-[var(--fg-primary)]">{entry.authorName}</span>
                    <span className="text-[var(--fg-tertiary)]">⏱</span>
                    <span className="text-[var(--fg-tertiary)]">{entry.relativeDate}</span>
                    <span className="text-[var(--fg-tertiary)]">({formattedDate})</span>
                </div>
            </div>

            {/* Commit message */}
            <div className="text-[var(--fg-primary)] mb-2 leading-relaxed">
                {entry.subject}
            </div>

            {/* Stats */}
            {entry.filesChanged > 0 && (
                <div className="text-[var(--fg-secondary)] mb-2">
                    {entry.filesChanged} file{entry.filesChanged !== 1 ? 's' : ''} changed
                    {entry.insertions > 0 && <>, <span className="text-green-400">{entry.insertions} insertion{entry.insertions !== 1 ? 's' : ''}(+)</span></>}
                    {entry.deletions > 0 && <>, <span className="text-red-400">{entry.deletions} deletion{entry.deletions !== 1 ? 's' : ''}(-)</span></>}
                </div>
            )}

            {/* Hash + Open on GitHub */}
            <div className="flex items-center gap-2 pt-1 border-t border-[var(--border-subtle)]">
                <span className="text-[var(--fg-tertiary)]">◇</span>
                <button
                    className="font-mono text-[var(--accent-primary)] hover:underline cursor-pointer flex items-center gap-1"
                    onClick={(e) => { e.stopPropagation(); onCopy(entry.hash); }}
                    title="Copy full commit hash"
                >
                    {entry.shortHash}
                    {copied ? (
                        <Check className="w-3 h-3 text-green-400" />
                    ) : (
                        <Copy className="w-3 h-3 opacity-60" />
                    )}
                </button>
                {commitUrl && (
                    <>
                        <span className="text-[var(--fg-tertiary)]">|</span>
                        <a
                            href={commitUrl}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-[var(--accent-primary)] hover:underline flex items-center gap-1"
                            onClick={(e) => e.stopPropagation()}
                        >
                            <ExternalLink className="w-3 h-3" />
                            Open on GitHub
                        </a>
                    </>
                )}
            </div>
        </div>
    );
};

interface GitGraphProps {
    expanded: boolean;
    onToggle: () => void;
}

export const GitGraph: React.FC<GitGraphProps> = ({ expanded, onToggle }) => {
    const [entries, setEntries] = useState<GitLogEntry[]>([]);
    const [remoteUrl, setRemoteUrl] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
    const [copiedHash, setCopiedHash] = useState<string | null>(null);
    const [statsCache, setStatsCache] = useState<Record<string, { insertions: number; deletions: number; filesChanged: number }>>({});
    const [tooltipPos, setTooltipPos] = useState<{ top: number; left: number } | null>(null);
    const containerRef = useRef<HTMLDivElement>(null);
    const rowRefs = useRef<Map<number, HTMLDivElement>>(new Map());

    const fetchLog = useCallback(async () => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        setLoading(true);
        setError(null);
        try {
            const [log, url] = await Promise.all([
                invoke<GitLogEntry[]>('git_log', { count: 50 }),
                invoke<string | null>('git_remote_url'),
            ]);
            setEntries(log);
            setRemoteUrl(url);
        } catch (e) {
            setError(String(e));
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        if (expanded && entries.length === 0) {
            fetchLog();
        }
    }, [expanded, entries.length, fetchLog]);

    const handleCopy = useCallback(async (hash: string) => {
        try {
            await navigator.clipboard.writeText(hash);
            setCopiedHash(hash);
            setTimeout(() => setCopiedHash(null), 2000);
        } catch {
            // Fallback
        }
    }, []);

    const handleMouseEnter = useCallback((index: number) => {
        setHoveredIndex(index);
        const row = rowRefs.current.get(index);
        const container = containerRef.current;
        if (row && container) {
            const rowRect = row.getBoundingClientRect();
            const containerRect = container.getBoundingClientRect();
            setTooltipPos({
                top: rowRect.top - containerRect.top + row.offsetHeight + 4,
                left: 24,
            });
        }
        // Lazy-load commit stats
        const entry = entries[index];
        if (entry && !statsCache[entry.hash]) {
            invoke<{ insertions: number; deletions: number; filesChanged: number }>('git_commit_stats', { hash: entry.hash })
                .then((stats) => {
                    setStatsCache((prev) => ({ ...prev, [entry.hash]: stats }));
                })
                .catch(() => {});
        }
    }, [entries, statsCache]);

    const handleMouseLeave = useCallback(() => {
        setHoveredIndex(null);
        setTooltipPos(null);
    }, []);

    // Determine which commits are on the "main line" vs merge parents
    // Simple heuristic: first parent is always the main line
    const getGraphColumn = (_entry: GitLogEntry, _index: number): number => {
        return 0; // Simplified single-column graph for now
    };

    return (
        <div className={`flex flex-col ${expanded ? 'basis-1/2 shrink-0 grow-0 min-h-0' : ''}`}>
            {/* Header */}
            <button
                className="w-full flex items-center gap-1.5 px-3 py-2 hover:bg-[var(--bg-surface-hover)] transition-colors border-t border-[var(--border-subtle)] shrink-0"
                onClick={onToggle}
            >
                <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-[var(--fg-secondary)] font-semibold">
                    {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                    Graph
                </div>
            </button>

            {expanded && (
                <div ref={containerRef} className="relative flex-1 min-h-0 overflow-y-auto">
                    {loading && (
                        <div className="p-3 text-[10px] text-[var(--fg-tertiary)] italic">Loading commits...</div>
                    )}
                    {error && (
                        <div className="p-3 text-[10px] text-[var(--accent-error)]">{error}</div>
                    )}
                    {!loading && !error && entries.length === 0 && (
                        <div className="p-3 text-[10px] text-[var(--fg-tertiary)] italic">No commits found</div>
                    )}
                    {!loading && entries.length > 0 && (
                        <div className="pb-2">
                            {entries.map((entry, index) => {
                                const color = getCommitColor(entry, index);
                                const col = getGraphColumn(entry, index);
                                const isMerge = entry.parents.length > 1;
                                const isHovered = hoveredIndex === index;
                                const isFirst = index === 0;
                                const isLast = index === entries.length - 1;

                                // Parse refs into badges
                                const branchRefs = entry.refs.filter(r => !r.startsWith('tag:'));
                                const tagRefs = entry.refs.filter(r => r.startsWith('tag: ')).map(r => r.replace('tag: ', ''));

                                return (
                                    <div
                                        key={entry.hash}
                                        ref={(el) => { if (el) rowRefs.current.set(index, el); }}
                                        className={`flex items-center gap-0 px-2 py-[3px] cursor-default select-none transition-colors ${
                                            isHovered ? 'bg-[var(--bg-surface-hover)]' : ''
                                        }`}
                                        onMouseEnter={() => handleMouseEnter(index)}
                                        onMouseLeave={handleMouseLeave}
                                    >
                                        {/* Graph column: vertical line + dot */}
                                        <div
                                            className="relative flex-shrink-0"
                                            style={{ width: 20 + col * 16, height: 22 }}
                                        >
                                            {/* Vertical line above */}
                                            {!isFirst && (
                                                <div
                                                    className="absolute left-[9px] top-0 w-[2px]"
                                                    style={{
                                                        height: 10,
                                                        backgroundColor: color,
                                                        opacity: 0.5,
                                                    }}
                                                />
                                            )}
                                            {/* Vertical line below */}
                                            {!isLast && (
                                                <div
                                                    className="absolute left-[9px] bottom-0 w-[2px]"
                                                    style={{
                                                        height: 10,
                                                        backgroundColor: getCommitColor(
                                                            entries[Math.min(index + 1, entries.length - 1)],
                                                            index + 1
                                                        ),
                                                        opacity: 0.5,
                                                    }}
                                                />
                                            )}
                                            {/* Commit dot */}
                                            <div
                                                className="absolute left-[5px] top-1/2 -translate-y-1/2 rounded-full border-2"
                                                style={{
                                                    width: isMerge ? 12 : 10,
                                                    height: isMerge ? 12 : 10,
                                                    borderColor: color,
                                                    backgroundColor: isMerge ? 'transparent' : color,
                                                    left: isMerge ? 4 : 5,
                                                }}
                                            />
                                        </div>

                                        {/* Commit message + refs */}
                                        <div className="flex-1 min-w-0 flex items-center gap-1.5 text-[11px]">
                                            <span className="truncate text-[var(--fg-primary)]" title={entry.subject}>
                                                {entry.subject}
                                            </span>

                                            {/* Branch badges */}
                                            {branchRefs.map((ref_) => {
                                                const isHead = ref_.includes('HEAD');
                                                const displayRef = ref_
                                                    .replace('HEAD -> ', '')
                                                    .replace('origin/', '');
                                                return (
                                                    <span
                                                        key={ref_}
                                                        className={`inline-flex items-center gap-0.5 px-1.5 py-0 rounded-full text-[9px] font-medium whitespace-nowrap ${
                                                            isHead
                                                                ? 'bg-green-600/20 text-green-400 border border-green-600/30'
                                                                : 'bg-blue-600/20 text-blue-400 border border-blue-600/30'
                                                        }`}
                                                    >
                                                        {isHead && <GitCommit className="w-2.5 h-2.5" />}
                                                        {displayRef}
                                                    </span>
                                                );
                                            })}

                                            {/* Tag badges */}
                                            {tagRefs.map((tag) => (
                                                <span
                                                    key={tag}
                                                    className="inline-flex items-center px-1.5 py-0 rounded-full text-[9px] font-medium whitespace-nowrap bg-amber-600/20 text-amber-400 border border-amber-600/30"
                                                >
                                                    {tag}
                                                </span>
                                            ))}
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}

                    {/* Hover tooltip */}
                    {hoveredIndex !== null && tooltipPos && entries[hoveredIndex] && (
                        <div
                            className="absolute z-50 bg-[var(--bg-surface)] border border-[var(--border-default)] rounded-lg shadow-xl"
                            style={{
                                top: tooltipPos.top,
                                left: tooltipPos.left,
                                maxWidth: 'calc(100% - 48px)',
                            }}
                            onMouseEnter={() => setHoveredIndex(hoveredIndex)}
                            onMouseLeave={handleMouseLeave}
                        >
                            <CommitTooltip
                                entry={{
                                    ...entries[hoveredIndex],
                                    ...(statsCache[entries[hoveredIndex].hash] || {}),
                                }}
                                remoteUrl={remoteUrl}
                                onCopy={handleCopy}
                                copied={copiedHash === entries[hoveredIndex].hash}
                            />
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};
