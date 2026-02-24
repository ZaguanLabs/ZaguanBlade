import React, { useMemo, useState } from 'react';
import type { GitFileStatus, GitStatusSummary, CommitPreflightResult } from '../hooks/useGitStatus';
import { Sparkles, GitCommit, Upload, ChevronDown, ChevronRight, Plus, Minus, RefreshCw, AlertTriangle } from 'lucide-react';
import { GitGraph } from './GitGraph';

interface GitPanelProps {
    status: GitStatusSummary | null;
    files: GitFileStatus[];
    error: string | null;
    filesError: string | null;
    lastRefreshedAt: number | null;
    onRefresh: () => Promise<void> | void;
    onStageFile: (path: string) => Promise<void>;
    onUnstageFile: (path: string) => Promise<void>;
    onStageAll: () => Promise<void>;
    onUnstageAll: () => Promise<void>;
    onCommit: (message: string) => Promise<void>;
    onPush: () => Promise<void>;
    onDiff: (path: string, staged: boolean) => Promise<string>;
    onGenerateCommitMessage: () => Promise<string>;
    onCommitPreflight: () => Promise<CommitPreflightResult>;
}

type DiffState = Record<
    string,
    {
        expanded: boolean;
        loading?: boolean;
        error?: string | null;
        staged?: string | null;
        unstaged?: string | null;
    }
>;

export const GitPanel: React.FC<GitPanelProps> = ({
    status,
    files,
    error,
    filesError,
    onRefresh,
    onStageFile,
    onUnstageFile,
    onStageAll,
    onUnstageAll,
    onCommit,
    onPush,
    onDiff,
    onGenerateCommitMessage,
    onCommitPreflight,
}) => {
    const [commitMessage, setCommitMessage] = useState('');
    const [actionError, setActionError] = useState<string | null>(null);
    const [preflightWarning, setPreflightWarning] = useState<string | null>(null);
    const [busyAction, setBusyAction] = useState<string | null>(null);
    const [pushSuccess, setPushSuccess] = useState(false);
    const [diffs, setDiffs] = useState<DiffState>({});
    const [stagedExpanded, setStagedExpanded] = useState(true);
    const [unstagedExpanded, setUnstagedExpanded] = useState(true);
    const [graphExpanded, setGraphExpanded] = useState(false);

    const isRepo = status?.isRepo ?? false;
    const changedCount = status?.changedCount ?? 0;
    const stagedCount = status?.stagedCount ?? 0;
    const canCommit = commitMessage.trim().length > 0 && changedCount > 0;

    const stagedFiles = useMemo(
        () => files.filter(file => file.staged),
        [files]
    );
    const unstagedFiles = useMemo(
        () => files.filter(file => file.unstaged || file.untracked),
        [files]
    );

    const runAction = async (id: string, action: () => Promise<void>) => {
        setActionError(null);
        setBusyAction(id);
        try {
            await action();
        } catch (e) {
            setActionError(String(e));
        } finally {
            setBusyAction(null);
        }
    };

    const toggleDiff = async (file: GitFileStatus) => {
        const key = file.path;
        const current = diffs[key];
        const expanded = current?.expanded ?? false;
        if (expanded) {
            setDiffs(prev => ({
                ...prev,
                [key]: { ...prev[key], expanded: false },
            }));
            return;
        }

        setDiffs(prev => ({
            ...prev,
            [key]: { ...prev[key], expanded: true, loading: true, error: null },
        }));

        try {
            const [stagedDiff, unstagedDiff] = await Promise.all([
                file.staged ? onDiff(file.path, true) : Promise.resolve(''),
                file.untracked || !file.unstaged ? Promise.resolve('') : onDiff(file.path, false),
            ]);

            setDiffs(prev => ({
                ...prev,
                [key]: {
                    expanded: true,
                    loading: false,
                    staged: stagedDiff || null,
                    unstaged: unstagedDiff || null,
                    error: null,
                },
            }));
        } catch (e) {
            setDiffs(prev => ({
                ...prev,
                [key]: {
                    expanded: true,
                    loading: false,
                    error: String(e),
                },
            }));
        }
    };

    // Get status indicator color based on file state
    const getStatusColor = (file: GitFileStatus) => {
        if (file.untracked) return 'text-green-400'; // New/untracked files in green
        if (file.conflicted) return 'text-red-400';
        if (file.staged) return 'text-emerald-400';
        return 'text-amber-400'; // Modified unstaged
    };

    const shouldStrikeFileName = (file: GitFileStatus) => {
        const code = (file.statusCode || '').toUpperCase();
        // Git porcelain uses D for deletions and R for renames/moves.
        return code.includes('D') || code.includes('R');
    };

    // File row component for compact display
    const FileRow = ({ file, isStaged }: { file: GitFileStatus; isStaged: boolean }) => (
        <div className="group flex items-center gap-1 py-0.5 px-1 rounded hover:bg-[var(--bg-surface-hover)] text-[11px]">
            <span className={`font-mono w-5 shrink-0 ${getStatusColor(file)}`}>
                {file.statusCode}
            </span>
            <span
                className={`truncate flex-1 cursor-pointer ${file.untracked ? 'text-green-400/80' : 'text-[var(--fg-primary)]'} ${shouldStrikeFileName(file) ? 'line-through decoration-red-400/70 text-[var(--fg-tertiary)]' : ''}`}
                onClick={() => toggleDiff(file)}
                title={file.path}
            >
                {file.displayPath || file.path.split('/').pop() || file.path}
            </span>
            <div className="opacity-0 group-hover:opacity-100 flex items-center gap-0.5 transition-opacity">
                {isStaged ? (
                    <button
                        className="p-0.5 rounded hover:bg-[var(--bg-surface)] text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)]"
                        onClick={() => runAction(`unstage-${file.path}`, () => onUnstageFile(file.path))}
                        disabled={busyAction === `unstage-${file.path}`}
                        title="Unstage"
                    >
                        <Minus className="w-3 h-3" />
                    </button>
                ) : (
                    <button
                        className="p-0.5 rounded hover:bg-[var(--bg-surface)] text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)]"
                        onClick={() => runAction(`stage-${file.path}`, () => onStageFile(file.path))}
                        disabled={busyAction === `stage-${file.path}`}
                        title="Stage"
                    >
                        <Plus className="w-3 h-3" />
                    </button>
                )}
            </div>
        </div>
    );

    return (
        <div className="h-full bg-[var(--bg-panel)] border-r border-[var(--border-subtle)] flex flex-col text-[var(--fg-secondary)]">
            {/* Header */}
            <div className="h-9 px-4 flex items-center bg-[var(--bg-panel)] border-b border-[var(--border-subtle)] text-[10px] uppercase tracking-wider font-semibold select-none justify-between text-[var(--fg-secondary)]">
                <span>Source Control</span>
                <button
                    onClick={() => runAction('refresh', async () => onRefresh())}
                    className="hover:text-[var(--fg-primary)] transition-colors"
                    title="Refresh"
                >
                    <RefreshCw className={`w-3 h-3 ${busyAction === 'refresh' ? 'animate-spin' : ''}`} />
                </button>
            </div>

            <div className="flex-1 min-h-0 flex flex-col">
                {!isRepo && (
                    <div className="p-4 text-[var(--fg-secondary)] italic text-xs">Not a Git repository.</div>
                )}

                {isRepo && (
                    <>
                        {/* Top section: commit box + changes — always scrollable */}
                        <div className="flex-1 min-h-0 overflow-y-auto">
                        {/* Commit Box - At the top, always visible */}
                        <div className="p-3 border-b border-[var(--border-subtle)] bg-[var(--bg-surface)]/50">
                            {/* Branch info inline */}
                            <div className="flex items-center justify-between mb-2 text-[10px]">
                                <div className="flex items-center gap-2">
                                    <span className="text-[var(--fg-secondary)]">Branch:</span>
                                    <span className="text-[var(--fg-primary)] font-medium">
                                        {status?.branch ?? 'detached'}
                                    </span>
                                </div>
                                {(status?.ahead ?? 0) > 0 || (status?.behind ?? 0) > 0 ? (
                                    <div className="flex items-center gap-1 text-[var(--fg-tertiary)]">
                                        {status?.ahead ? <span className="text-green-400">↑{status.ahead}</span> : null}
                                        {status?.behind ? <span className="text-amber-400">↓{status.behind}</span> : null}
                                    </div>
                                ) : null}
                            </div>

                            {/* Commit message textarea */}
                            <textarea
                                className="w-full min-h-[60px] bg-[var(--bg-surface)] border border-[var(--border-default)] rounded-lg p-2.5 text-[11px] text-[var(--fg-primary)] placeholder-[var(--fg-secondary)] resize-none focus:outline-none focus:border-[var(--accent-primary)]/50 focus:ring-1 focus:ring-[var(--accent-primary)]/20 transition-all"
                                placeholder="Commit message..."
                                value={commitMessage}
                                onChange={e => setCommitMessage(e.target.value)}
                            />

                            {/* Action buttons row */}
                            <div className="flex items-center gap-2 mt-2">
                                {/* Generate button - always visible */}
                                <button
                                    className={`flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-md transition-all font-medium border ${
                                        busyAction === 'generate-message' || changedCount === 0
                                            ? 'border-[var(--border-subtle)] text-[var(--fg-tertiary)] cursor-not-allowed opacity-50'
                                            : 'border-[var(--border-default)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)]'
                                    }`}
                                    disabled={busyAction === 'generate-message' || changedCount === 0}
                                    onClick={() =>
                                        runAction('generate-message', async () => {
                                            const message = await onGenerateCommitMessage();
                                            setCommitMessage(message);
                                        })
                                    }
                                    title="Generate commit message with AI"
                                >
                                    <Sparkles className={`w-3 h-3 ${busyAction === 'generate-message' ? 'animate-pulse' : ''}`} />
                                    Generate
                                </button>

                                <div className="flex-1" />

                                {/* Dynamic Commit/Push button */}
                                {(status?.ahead ?? 0) > 0 || pushSuccess ? (
                                    <button
                                        className={`relative flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-md transition-all duration-300 font-medium overflow-hidden ${
                                            pushSuccess
                                                ? 'bg-emerald-500 text-white shadow-lg shadow-emerald-500/30 scale-[1.02]'
                                                : busyAction === 'push'
                                                    ? 'bg-slate-900 text-slate-100 border border-slate-700/70 cursor-not-allowed shadow-inner'
                                                    : 'bg-green-600 text-white hover:bg-green-500 hover:shadow-md hover:shadow-green-500/20'
                                        }`}
                                        disabled={busyAction === 'push' || pushSuccess}
                                        onClick={() => runAction('push', async () => {
                                            await onPush();
                                            setPushSuccess(true);
                                            setTimeout(() => setPushSuccess(false), 2500);
                                        })}
                                    >
                                        {/* Push indicator badge */}
                                        {status?.ahead && status.ahead > 0 && !pushSuccess && (
                                            <span className="absolute -top-1.5 -right-1.5 flex items-center justify-center w-3.5 h-3.5 bg-white text-green-600 rounded-full text-[8px] font-bold animate-pulse">
                                                {status.ahead}
                                            </span>
                                        )}
                                        {/* Animated left-to-right progress fill while pushing */}
                                        {busyAction === 'push' && (
                                            <div className="absolute inset-0 overflow-hidden rounded-md pointer-events-none">
                                                <div
                                                    className="absolute inset-y-0 left-0 bg-gradient-to-r from-emerald-500/85 via-teal-400/85 to-cyan-400/85"
                                                    style={{
                                                        width: '100%',
                                                        transformOrigin: 'left center',
                                                        animation: 'push-fill-sweep 1.2s ease-out infinite',
                                                    }}
                                                />
                                            </div>
                                        )}

                                        <span className="relative z-10 flex items-center gap-1.5">
                                            {pushSuccess ? (
                                                <>
                                                    <svg className="w-3.5 h-3.5 animate-[push-check_0.3s_ease-out]" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12" /></svg>
                                                    Pushed!
                                                </>
                                            ) : busyAction === 'push' ? (
                                                <>
                                                    <svg className="w-3.5 h-3.5 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><path d="M12 2a10 10 0 0 1 10 10" /></svg>
                                                    Pushing
                                                </>
                                            ) : (
                                                <>
                                                    <Upload className="w-3 h-3" />
                                                    Push {status?.ahead}
                                                </>
                                            )}
                                        </span>
                                    </button>
                                ) : (
                                    <button
                                        className={`flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-md transition-all font-medium ${
                                            busyAction === 'commit' || !canCommit
                                                ? 'bg-[var(--bg-surface)] text-[var(--fg-tertiary)] cursor-not-allowed'
                                                : 'bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/80'
                                        }`}
                                        disabled={busyAction === 'commit' || !canCommit}
                                        onClick={() =>
                                            runAction('commit', async () => {
                                                setPreflightWarning(null);
                                                if (stagedCount === 0 && changedCount > 0) {
                                                    await onStageAll();
                                                }
                                                const preflight = await onCommitPreflight();
                                                if (!preflight.isRepo) {
                                                    throw new Error('Not a Git repository');
                                                }
                                                if (preflight.hasConflicts) {
                                                    throw new Error('Resolve merge conflicts before committing');
                                                }
                                                if (preflight.isDetached) {
                                                    setPreflightWarning('HEAD is detached. Commits may be lost if you switch branches.');
                                                }
                                                if (!preflight.canCommit && preflight.errorMessage) {
                                                    throw new Error(preflight.errorMessage);
                                                }
                                                await onCommit(commitMessage);
                                                setCommitMessage('');
                                                setPreflightWarning(null);
                                            })
                                        }
                                    >
                                        <GitCommit className={`w-3 h-3 ${busyAction === 'commit' ? 'animate-spin' : ''}`} />
                                        Commit
                                    </button>
                                )}
                            </div>

                            {/* Preflight warning (e.g., detached HEAD) */}
                            {preflightWarning && (
                                <div className="flex items-center gap-1.5 text-[10px] text-amber-400 mt-2 p-2 bg-amber-400/10 rounded-md">
                                    <AlertTriangle className="w-3 h-3 shrink-0" />
                                    <span>{preflightWarning}</span>
                                </div>
                            )}

                            {/* Contextual hints */}
                            {stagedCount === 0 && changedCount > 0 && (
                                <div className="text-[10px] text-[var(--fg-tertiary)] mt-2 italic">
                                    Unstaged changes will be staged on commit
                                </div>
                            )}
                        </div>

                        {/* Changes section */}
                        <div>
                            {changedCount === 0 ? (
                                <div className="p-4 text-[var(--fg-secondary)] italic text-xs text-center">
                                    ✓ Working tree clean
                                </div>
                            ) : (
                                <div className="text-xs">
                                    {/* Staged Changes */}
                                    <div className="border-b border-[var(--border-subtle)]">
                                        <button
                                            className="w-full flex items-center justify-between px-3 py-2 hover:bg-[var(--bg-surface-hover)] transition-colors"
                                            onClick={() => setStagedExpanded(!stagedExpanded)}
                                        >
                                            <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-[var(--fg-secondary)] font-semibold">
                                                {stagedExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                                                Staged
                                                <span className="text-emerald-400 font-normal normal-case">
                                                    ({stagedFiles.length})
                                                </span>
                                            </div>
                                            {stagedFiles.length > 0 && (
                                                <button
                                                    className="text-[10px] px-2 py-0.5 rounded text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        runAction('unstage-all', () => onUnstageAll());
                                                    }}
                                                    disabled={busyAction === 'unstage-all'}
                                                >
                                                    Unstage All
                                                </button>
                                            )}
                                        </button>
                                        {stagedExpanded && stagedFiles.length > 0 && (
                                            <div className="px-2 pb-2">
                                                {stagedFiles.map(file => (
                                                    <React.Fragment key={`staged-${file.path}`}>
                                                        <FileRow file={file} isStaged={true} />
                                                        {diffs[file.path]?.expanded && (
                                                            <div className="ml-6 mr-2 mb-1 rounded bg-[var(--bg-app)] border border-[var(--border-subtle)] p-2 text-[10px] font-mono whitespace-pre-wrap overflow-x-auto max-h-48 overflow-y-auto">
                                                                {diffs[file.path]?.loading && <div className="text-[var(--fg-tertiary)]">Loading...</div>}
                                                                {diffs[file.path]?.error && <div className="text-[var(--accent-error)]">{diffs[file.path]?.error}</div>}
                                                                {!diffs[file.path]?.loading && !diffs[file.path]?.error && (
                                                                    <pre className="text-[var(--fg-secondary)]">{diffs[file.path]?.staged || 'No changes'}</pre>
                                                                )}
                                                            </div>
                                                        )}
                                                    </React.Fragment>
                                                ))}
                                            </div>
                                        )}
                                    </div>

                                    {/* Unstaged/Untracked Changes */}
                                    <div>
                                        <button
                                            className="w-full flex items-center justify-between px-3 py-2 hover:bg-[var(--bg-surface-hover)] transition-colors"
                                            onClick={() => setUnstagedExpanded(!unstagedExpanded)}
                                        >
                                            <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-[var(--fg-secondary)] font-semibold">
                                                {unstagedExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                                                Changes
                                                <span className="text-amber-400 font-normal normal-case">
                                                    ({unstagedFiles.length})
                                                </span>
                                            </div>
                                            {unstagedFiles.length > 0 && (
                                                <button
                                                    className="text-[10px] px-2 py-0.5 rounded text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        runAction('stage-all', () => onStageAll());
                                                    }}
                                                    disabled={busyAction === 'stage-all'}
                                                >
                                                    Stage All
                                                </button>
                                            )}
                                        </button>
                                        {unstagedExpanded && unstagedFiles.length > 0 && (
                                            <div className="px-2 pb-2">
                                                {unstagedFiles.map(file => (
                                                    <React.Fragment key={`unstaged-${file.path}`}>
                                                        <FileRow file={file} isStaged={false} />
                                                        {diffs[file.path]?.expanded && !file.untracked && (
                                                            <div className="ml-6 mr-2 mb-1 rounded bg-[var(--bg-app)] border border-[var(--border-subtle)] p-2 text-[10px] font-mono whitespace-pre-wrap overflow-x-auto max-h-48 overflow-y-auto">
                                                                {diffs[file.path]?.loading && <div className="text-[var(--fg-tertiary)]">Loading...</div>}
                                                                {diffs[file.path]?.error && <div className="text-[var(--accent-error)]">{diffs[file.path]?.error}</div>}
                                                                {!diffs[file.path]?.loading && !diffs[file.path]?.error && (
                                                                    <pre className="text-[var(--fg-secondary)]">{diffs[file.path]?.unstaged || 'No changes'}</pre>
                                                                )}
                                                            </div>
                                                        )}
                                                    </React.Fragment>
                                                ))}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            )}
                        </div>
                        </div>

                        {/* Graph section - pinned to bottom */}
                        <GitGraph expanded={graphExpanded} onToggle={() => setGraphExpanded(!graphExpanded)} />
                    </>
                )}

                {/* Error display */}
                {(error || filesError || actionError) && (
                    <div className="p-3 text-[10px] text-[var(--accent-error)] break-all border-t border-[var(--border-subtle)] bg-[var(--accent-error)]/5">
                        {error || filesError || actionError}
                    </div>
                )}
            </div>
        </div>
    );
};
