import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ChevronRight, Copy, Check, ExternalLink, GitCommit } from 'lucide-react';
import { formatUnknownBackendError } from '../utils/backendErrors';

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

const PRIMARY_BRANCH_CANDIDATES = ['main', 'master', 'trunk'];
const LANE_X_START = 9;
const LANE_STEP = 14;
const GRAPH_LEFT_PADDING = 6;
const GRAPH_RIGHT_PADDING = 10;

interface GraphRenderRow {
    lane: number;
    color: string;
    beforeSegments: Array<{ lane: number; color: string }>;
    afterSegments: Array<{ lane: number; color: string }>;
}

function collectLaneSegments(
    activeLanes: Array<string | null>,
    laneColors: Map<number, string>
): Array<{ lane: number; color: string }> {
    return activeLanes
        .map((expectedHash, laneIdx) => {
            if (!expectedHash) return null;
            return {
                lane: laneIdx,
                color: laneColors.get(laneIdx) ?? GRAPH_COLORS[laneIdx % GRAPH_COLORS.length],
            };
        })
        .filter((segment): segment is { lane: number; color: string } => segment !== null);
}

function normalizeBranchRef(refValue: string): string {
    return refValue
        .replace('HEAD -> ', '')
        .replace('origin/', '')
        .trim();
}

function extractBranchRefs(entry: GitLogEntry): string[] {
    return entry.refs
        .filter((refValue) => !refValue.startsWith('tag: '))
        .map(normalizeBranchRef)
        .filter(Boolean);
}

function detectPrimaryBranch(entries: GitLogEntry[]): string | null {
    for (const candidate of PRIMARY_BRANCH_CANDIDATES) {
        const hasCandidate = entries.some((entry) =>
            extractBranchRefs(entry).some((refValue) => refValue === candidate)
        );
        if (hasCandidate) return candidate;
    }
    return null;
}

function getFirstEmptyLane(activeLanes: Array<string | null>): number {
    const emptyIdx = activeLanes.findIndex((value) => value === null);
    if (emptyIdx !== -1) return emptyIdx;
    activeLanes.push(null);
    return activeLanes.length - 1;
}

function buildGraphRows(entries: GitLogEntry[]): GraphRenderRow[] {
    const primaryBranch = detectPrimaryBranch(entries);
    const rows: GraphRenderRow[] = [];
    const activeLanes: Array<string | null> = [];
    const laneColors = new Map<number, string>();
    const branchColors = new Map<string, string>();
    let nextColorIndex = 1;

    if (primaryBranch) {
        branchColors.set(primaryBranch, GRAPH_COLORS[0]);
    }

    for (const entry of entries) {
        const beforeSegments = collectLaneSegments(activeLanes, laneColors);

        let lane = activeLanes.findIndex((expectedHash) => expectedHash === entry.shortHash);
        if (lane === -1) {
            lane = getFirstEmptyLane(activeLanes);
        }

        const branchRefs = extractBranchRefs(entry);
        const branchRef = branchRefs[0] ?? null;
        const isPrimary = primaryBranch !== null && branchRefs.some((refValue) => refValue === primaryBranch);

        let color = laneColors.get(lane);
        if (!color) {
            if (isPrimary) {
                color = GRAPH_COLORS[0];
            } else if (branchRef && branchColors.has(branchRef)) {
                color = branchColors.get(branchRef)!;
            } else {
                color = GRAPH_COLORS[nextColorIndex % GRAPH_COLORS.length];
                nextColorIndex += 1;
            }
            laneColors.set(lane, color);
        }

        if (branchRef && !branchColors.has(branchRef)) {
            branchColors.set(branchRef, color);
        }
        if (isPrimary) {
            laneColors.set(lane, GRAPH_COLORS[0]);
            color = GRAPH_COLORS[0];
        }

        const firstParent = entry.parents[0] ?? null;
        activeLanes[lane] = firstParent;

        for (const parentHash of entry.parents.slice(1)) {
            let parentLane = activeLanes.findIndex((expectedHash) => expectedHash === parentHash);
            if (parentLane === -1) {
                parentLane = getFirstEmptyLane(activeLanes);
                activeLanes[parentLane] = parentHash;
            }
            if (!laneColors.has(parentLane)) {
                laneColors.set(parentLane, GRAPH_COLORS[nextColorIndex % GRAPH_COLORS.length]);
                nextColorIndex += 1;
            }
        }

        const afterSegments = collectLaneSegments(activeLanes, laneColors);

        rows.push({
            lane,
            color,
            beforeSegments,
            afterSegments,
        });
    }

    return rows;
}

function getLocalAvatar(name: string, email: string, size = 32): string {
    const initials = name
        .split(' ')
        .map(p => p[0] ?? '')
        .join('')
        .slice(0, 2)
        .toUpperCase();
    // Deterministic hue from email
    let hash = 0;
    for (let i = 0; i < email.length; i++) hash = (hash * 31 + email.charCodeAt(i)) >>> 0;
    const hue = hash % 360;
    const bg = `hsl(${hue},40%,28%)`;
    const fg = `hsl(${hue},60%,80%)`;
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}"><rect width="${size}" height="${size}" rx="${size / 2}" fill="${bg}"/><text x="50%" y="50%" dy="0.35em" text-anchor="middle" font-family="Inter,sans-serif" font-size="${Math.round(size * 0.42)}" fill="${fg}">${initials}</text></svg>`;
    return `data:image/svg+xml;base64,${btoa(svg)}`;
}

interface CommitTooltipProps {
    entry: GitLogEntry;
    remoteUrl: string | null;
    onCopy: (hash: string) => void;
    copied: boolean;
}

const CommitTooltip: React.FC<CommitTooltipProps> = ({ entry, remoteUrl, onCopy, copied }) => {
    const { t } = useTranslation();
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
                    src={getLocalAvatar(entry.authorName, entry.authorEmail)}
                    alt=""
                    className="w-6 h-6 rounded-full bg-[var(--bg-surface)]"
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
                    {t('gitGraph.filesChanged', { count: entry.filesChanged })}
                    {entry.insertions > 0 && <>, <span className="text-green-400">{t('gitGraph.insertions', { count: entry.insertions })}</span></>}
                    {entry.deletions > 0 && <>, <span className="text-red-400">{t('gitGraph.deletions', { count: entry.deletions })}</span></>}
                </div>
            )}

            {/* Hash + Open on GitHub */}
            <div className="flex items-center gap-2 pt-1 border-t border-[var(--border-subtle)]">
                <span className="text-[var(--fg-tertiary)]">◇</span>
                <button
                    className="font-mono text-[var(--accent-primary)] hover:underline cursor-pointer flex items-center gap-1"
                    onClick={(e) => { e.stopPropagation(); onCopy(entry.hash); }}
                    title={t('gitGraph.copyFullCommitHash')}
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
                            {t('gitGraph.openOnGitHub')}
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
    const { t } = useTranslation();
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
            setError(`${t('gitGraph.loadFailed')}: ${formatUnknownBackendError(e)}`);
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

    const graphRows = useMemo(() => buildGraphRows(entries), [entries]);
    const laneCount = useMemo(
        () => graphRows.reduce((max, row) => Math.max(max, row.lane + 1), 1),
        [graphRows]
    );
    const graphColumnWidth =
        GRAPH_LEFT_PADDING +
        GRAPH_RIGHT_PADDING +
        (laneCount > 0 ? LANE_X_START + (laneCount - 1) * LANE_STEP + 4 : 20);

    return (
        <div className={`flex flex-col ${expanded ? 'basis-1/2 shrink-0 grow-0 min-h-0' : ''}`}>
            {/* Header */}
            <button
                className="w-full flex items-center gap-1.5 px-3 py-2 hover:bg-[var(--bg-surface-hover)] transition-colors border-t border-[var(--border-subtle)] shrink-0"
                onClick={onToggle}
            >
                <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-[var(--fg-secondary)] font-semibold">
                    {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                    {t('gitGraph.title')}
                </div>
            </button>

            {expanded && (
                <div ref={containerRef} className="relative flex-1 min-h-0 overflow-y-auto">
                    {loading && (
                        <div className="p-3 text-[10px] text-[var(--fg-tertiary)] italic">{t('gitGraph.loadingCommits')}</div>
                    )}
                    {error && (
                        <div className="p-3 text-[10px] text-[var(--accent-error)]">{error}</div>
                    )}
                    {!loading && !error && entries.length === 0 && (
                        <div className="p-3 text-[10px] text-[var(--fg-tertiary)] italic">{t('gitGraph.noCommits')}</div>
                    )}
                    {!loading && entries.length > 0 && (
                        <div className="pb-2">
                            {entries.map((entry, index) => {
                                const row = graphRows[index];
                                const color = row?.color ?? GRAPH_COLORS[0];
                                const lane = row?.lane ?? 0;
                                const isMerge = entry.parents.length > 1;
                                const isHovered = hoveredIndex === index;

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
                                            style={{ width: graphColumnWidth, height: 22 }}
                                        >
                                            {/* Vertical lanes above current commit */}
                                            {row?.beforeSegments.map((segment) => (
                                                <div
                                                    key={`before-${entry.hash}-${segment.lane}`}
                                                    className="absolute top-0 w-[2px]"
                                                    style={{
                                                        left: LANE_X_START + segment.lane * LANE_STEP,
                                                        height: 10,
                                                        backgroundColor: segment.color,
                                                        opacity: 0.6,
                                                    }}
                                                />
                                            ))}
                                            {/* Vertical lanes below current commit */}
                                            {row?.afterSegments.map((segment) => (
                                                <div
                                                    key={`after-${entry.hash}-${segment.lane}`}
                                                    className="absolute bottom-0 w-[2px]"
                                                    style={{
                                                        left: LANE_X_START + segment.lane * LANE_STEP,
                                                        height: 10,
                                                        backgroundColor: segment.color,
                                                        opacity: 0.6,
                                                    }}
                                                />
                                            ))}
                                            {/* Commit dot */}
                                            <div
                                                className="absolute top-1/2 -translate-y-1/2 rounded-full border-2"
                                                style={{
                                                    width: isMerge ? 12 : 10,
                                                    height: isMerge ? 12 : 10,
                                                    borderColor: color,
                                                    backgroundColor: isMerge ? 'transparent' : color,
                                                    left: LANE_X_START + lane * LANE_STEP - (isMerge ? 5 : 4),
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
