import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GitFileStatus, GitStatusSummary, CommitPreflightResult } from '../hooks/useGitStatus';
import { Sparkles, GitCommit, Upload, ChevronDown, ChevronRight, Plus, Minus, RefreshCw, AlertTriangle } from 'lucide-react';
import { GitGraph } from './GitGraph';
import { formatUnknownBackendError } from '../utils/backendErrors';

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
    const { t } = useTranslation();
    const [commitMessage, setCommitMessage] = useState('');
    const commitTextareaRef = useRef<HTMLTextAreaElement>(null);
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
    const isPushing = busyAction === 'push';
    const showPushButton = (status?.ahead ?? 0) > 0 || pushSuccess || isPushing;

    const stagedFiles = useMemo(
        () => files.filter(file => file.staged),
        [files]
    );
    const unstagedFiles = useMemo(
        () => files.filter(file => file.unstaged || file.untracked),
        [files]
    );

    const resizeCommitTextarea = useCallback((textarea?: HTMLTextAreaElement | null) => {
        if (!textarea) return;
        textarea.style.height = 'auto';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
    }, []);

    useEffect(() => {
        resizeCommitTextarea(commitTextareaRef.current);
    }, [commitMessage, resizeCommitTextarea]);

    const runAction = async (id: string, action: () => Promise<void>) => {
        setActionError(null);
        setBusyAction(id);
        try {
            await action();
        } catch (e) {
            setActionError(formatUnknownBackendError(e));
        } finally {
            setBusyAction(null);
        }
    };

    const handlePushOnButtonUp = useCallback(async () => {
        if (busyAction === 'push' || pushSuccess) return;

        setActionError(null);
        setBusyAction('push');

        // Yield to the event loop so the "Pushing" state paints immediately on click.
        await new Promise<void>(resolve => setTimeout(resolve, 50));

        try {
            await onPush();
            setPushSuccess(true);
            setTimeout(() => setPushSuccess(false), 2500);
        } catch (e) {
            setActionError(formatUnknownBackendError(e));
        } finally {
            setBusyAction(null);
        }
    }, [busyAction, onPush, pushSuccess]);

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
                    error: formatUnknownBackendError(e),
                },
            }));
        }
    };

    // Get status indicator color based on file state
    const getStatusColor = (file: GitFileStatus) => {
        if (file.untracked) return 'text-(--accent-mention)'; // New/untracked files in green
        if (file.conflicted) return 'text-(--state-danger)';
        if (file.staged) return 'text-(--accent-mention)';
        return 'text-(--accent-warning)'; // Modified unstaged
    };

    const shouldStrikeFileName = (file: GitFileStatus) => {
        const code = (file.statusCode || '').toUpperCase();
        // Git porcelain uses D for deletions and R for renames/moves.
        return code.includes('D') || code.includes('R');
    };

    // File row component for compact display
    const FileRow = ({ file, isStaged }: { file: GitFileStatus; isStaged: boolean }) => (
        <div className="group flex items-center gap-1 py-0.5 px-1 rounded-[calc(var(--panel-radius)*0.4)] hover:bg-(--bg-surface-hover) text-[11px]">
            <span className={`font-mono w-5 shrink-0 ${getStatusColor(file)}`}>
                {file.statusCode}
            </span>
            <span
                className={`truncate flex-1 cursor-pointer ${file.untracked ? 'text-(--accent-mention)/80' : 'text-(--fg-primary)'} ${shouldStrikeFileName(file) ? 'line-through decoration-(--state-danger) text-(--fg-tertiary)' : ''}`}
                onClick={() => toggleDiff(file)}
                title={file.path}
            >
                {file.displayPath || file.path.split('/').pop() || file.path}
            </span>
            <div className="opacity-0 group-hover:opacity-100 flex items-center gap-0.5 transition-opacity">
                {isStaged ? (
                    <button
                        className="p-0.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface) text-(--fg-tertiary) hover:text-(--fg-primary)"
                        onClick={() => runAction(`unstage-${file.path}`, () => onUnstageFile(file.path))}
                        disabled={busyAction === `unstage-${file.path}`}
                        title={t('git.unstage')}
                    >
                        <Minus className="w-3 h-3" />
                    </button>
                ) : (
                    <button
                        className="p-0.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface) text-(--fg-tertiary) hover:text-(--fg-primary)"
                        onClick={() => runAction(`stage-${file.path}`, () => onStageFile(file.path))}
                        disabled={busyAction === `stage-${file.path}`}
                        title={t('git.stage')}
                    >
                        <Plus className="w-3 h-3" />
                    </button>
                )}
            </div>
        </div>
    );

    return (
        <div className="h-full bg-(--bg-panel) border-r border-(--border-subtle) flex flex-col text-(--fg-secondary)">
            {/* Header */}
            <div className="h-9 px-4 flex items-center bg-(--bg-panel) border-b border-(--border-subtle) text-[10px] uppercase tracking-wider font-semibold select-none justify-between text-(--fg-secondary)">
                <span>{t('git.sourceControlTitle')}</span>
                <button
                    onClick={() => runAction('refresh', async () => onRefresh())}
                    className="hover:text-(--fg-primary) transition-colors"
                    title={t('fileTree.refresh')}
                >
                    <RefreshCw className={`w-3 h-3 ${busyAction === 'refresh' ? 'animate-spin' : ''}`} />
                </button>
            </div>

            <div className="flex-1 min-h-0 flex flex-col">
                {!isRepo && (
                    <div className="p-4 text-(--fg-secondary) italic text-xs">{t('git.notRepository')}</div>
                )}

                {isRepo && (
                    <>
                        {/* Top section: commit box + changes — always scrollable */}
                        <div className="flex-1 min-h-0 overflow-y-auto">
                        {/* Commit Box - At the top, always visible */}
                        <div className="p-3 border-b border-(--border-subtle) bg-(--bg-surface)/50">
                            {/* Branch info inline */}
                            <div className="flex items-center justify-between mb-2 text-[10px]">
                                <div className="flex items-center gap-2">
                                    <span className="text-(--fg-secondary)">{t('git.branchLabel')}</span>
                                    <span className="text-(--fg-primary) font-medium">
                                        {status?.branch ?? t('git.detachedHead')}
                                    </span>
                                </div>
                                {(status?.ahead ?? 0) > 0 || (status?.behind ?? 0) > 0 ? (
                                    <div className="flex items-center gap-1 text-(--fg-tertiary)">
                                        {status?.ahead ? <span className="text-(--accent-mention)">↑{status.ahead}</span> : null}
                                        {status?.behind ? <span className="text-(--accent-warning)">↓{status.behind}</span> : null}
                                    </div>
                                ) : null}
                            </div>

                            {/* Commit message textarea */}
                            <textarea
                                ref={commitTextareaRef}
                                rows={5}
                                className="w-full min-h-[96px] max-h-[180px] overflow-y-auto bg-(--bg-surface) border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] p-2.5 text-[11px] text-(--fg-primary) placeholder-(--fg-secondary) resize-none focus:outline-none focus:border-(--accent-ai) focus:ring-1 focus:ring-(--accent-ai)/20 transition-all"
                                placeholder={t('git.commitMessagePlaceholder')}
                                value={commitMessage}
                                onChange={e => {
                                    setCommitMessage(e.target.value);
                                    resizeCommitTextarea(e.currentTarget);
                                }}
                            />

                            {/* Action buttons row */}
                            <div className="flex items-center gap-2 mt-2">
                                {/* Generate button - always visible */}
                                <button
                                    className={`flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-[calc(var(--panel-radius)*0.55)] transition-all font-medium border ${
                                        busyAction === 'generate-message'
                                            ? 'border-(--accent-ai)/40 text-(--accent-ai) animate-generate-pulse'
                                            : changedCount === 0
                                                ? 'border-(--border-subtle) text-(--fg-tertiary) cursor-not-allowed opacity-50'
                                                : 'border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover)'
                                    }`}
                                    disabled={changedCount === 0}
                                    onClick={() =>
                                        runAction('generate-message', async () => {
                                            const message = await onGenerateCommitMessage();
                                            setCommitMessage(message);
                                        })
                                    }
                                    title={t('git.generateCommitMessage')}
                                >
                                    <Sparkles className={`w-3 h-3 ${busyAction === 'generate-message' ? 'animate-spin' : ''}`} />
                                    {busyAction === 'generate-message' ? t('git.generating') : t('git.generate')}
                                </button>

                                <div className="flex-1" />

                                {/* Dynamic Commit/Push button */}
                                {showPushButton ? (
                                    <button
                                        className={`relative flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-[calc(var(--panel-radius)*0.55)] transition-all duration-300 font-medium overflow-hidden ${
                                            pushSuccess ? 'scale-[1.02]' : ''
                                        } ${
                                            isPushing || pushSuccess ? 'cursor-not-allowed' : ''
                                        }`}
                                        style={{
                                            backgroundColor: pushSuccess
                                                ? 'var(--accent-mention)'
                                                : isPushing
                                                    ? 'color-mix(in srgb, var(--accent-ai) 18%, var(--bg-surface))'
                                                    : 'var(--accent-ai)',
                                            color: pushSuccess
                                                ? 'var(--fg-bright)'
                                                : isPushing
                                                    ? 'var(--fg-primary)'
                                                    : 'var(--fg-bright)',
                                            border: isPushing
                                                ? '1px solid color-mix(in srgb, var(--accent-ai) 40%, var(--border-default))'
                                                : '1px solid transparent',
                                            boxShadow: pushSuccess
                                                ? '0 0 0 1px color-mix(in srgb, var(--accent-mention) 24%, transparent), var(--shadow-md)'
                                                : isPushing
                                                    ? 'inset 0 1px 0 color-mix(in srgb, var(--fg-bright) 12%, transparent)'
                                                    : 'var(--shadow-sm)',
                                        }}
                                        disabled={isPushing || pushSuccess}
                                        onClick={(e) => {
                                            e.preventDefault();
                                            void handlePushOnButtonUp();
                                        }}
                                        onKeyUp={(e) => {
                                            if (e.key === 'Enter' || e.key === ' ') {
                                                e.preventDefault();
                                                void handlePushOnButtonUp();
                                            }
                                        }}
                                    >
                                        {/* Push indicator badge */}
                                        {status?.ahead && status.ahead > 0 && !pushSuccess && (
                                            <span
                                                className="absolute -top-1.5 -right-1.5 flex items-center justify-center w-3.5 h-3.5 rounded-full text-[8px] font-bold animate-pulse"
                                                style={{
                                                    backgroundColor: 'var(--bg-surface)',
                                                    color: 'var(--accent-ai)',
                                                    boxShadow: '0 0 0 1px color-mix(in srgb, var(--accent-ai) 22%, transparent)',
                                                }}
                                            >
                                                {status.ahead}
                                            </span>
                                        )}
                                        {/* Animated left-to-right progress fill while pushing */}
                                        {isPushing && (
                                            <div className="absolute inset-0 overflow-hidden rounded-[calc(var(--panel-radius)*0.55)] pointer-events-none">
                                                <div
                                                    className="absolute inset-y-0 left-0"
                                                    style={{
                                                        background: 'linear-gradient(90deg, color-mix(in srgb, var(--accent-ai) 82%, transparent), color-mix(in srgb, var(--accent-mention) 78%, transparent), color-mix(in srgb, var(--accent-purple) 76%, transparent))',
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
                                                    {t('git.pushed')}
                                                </>
                                            ) : isPushing ? (
                                                <>
                                                    <svg className="w-3.5 h-3.5 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><path d="M12 2a10 10 0 0 1 10 10" /></svg>
                                                    {t('git.pushing')}
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
                                        className={`flex items-center gap-1.5 text-[10px] px-3 py-1.5 rounded-[calc(var(--panel-radius)*0.55)] transition-all font-medium ${
                                            busyAction === 'commit' || !canCommit
                                                ? 'bg-(--bg-surface) text-(--fg-tertiary) cursor-not-allowed'
                                                : 'bg-(--accent-ai) text-(--fg-bright) hover:brightness-110'
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
                                                    throw new Error(preflight.errorMessage || t(preflight.errorKey || 'git.notRepository'));
                                                }
                                                if (preflight.hasConflicts) {
                                                    throw new Error(t('git.resolveConflictsBeforeCommit'));
                                                }
                                                if (preflight.isDetached) {
                                                    setPreflightWarning(t('git.detachedHeadWarning'));
                                                }
                                                if (!preflight.canCommit && preflight.errorMessage) {
                                                    throw new Error(formatUnknownBackendError(preflight.errorMessage));
                                                }
                                                await onCommit(commitMessage);
                                                setCommitMessage('');
                                                setPreflightWarning(null);
                                            })
                                        }
                                    >
                                        <GitCommit className={`w-3 h-3 ${busyAction === 'commit' ? 'animate-spin' : ''}`} />
                                        {t('git.commit')}
                                    </button>
                                )}
                            </div>

                            {/* Preflight warning (e.g., detached HEAD) */}
                            {preflightWarning && (
                                <div className="flex items-center gap-1.5 text-[10px] text-(--accent-warning) mt-2 p-2 bg-[color-mix(in_srgb,var(--accent-warning)_10%,transparent)] rounded-[calc(var(--panel-radius)*0.55)]">
                                    <AlertTriangle className="w-3 h-3 shrink-0" />
                                    <span>{preflightWarning}</span>
                                </div>
                            )}

                            {/* Contextual hints */}
                            {stagedCount === 0 && changedCount > 0 && (
                                <div className="text-[10px] text-(--fg-tertiary) mt-2 italic">
                                    {t('git.unstagedCommitHint')}
                                </div>
                            )}
                        </div>

                        {/* Changes section */}
                        <div>
                            {changedCount === 0 ? (
                                <div className="p-4 text-(--fg-secondary) italic text-xs text-center">
                                    {t('git.workingTreeClean')}
                                </div>
                            ) : (
                                <div className="text-xs">
                                    {/* Staged Changes */}
                                    <div className="border-b border-(--border-subtle)">
                                        <button
                                            className="w-full flex items-center justify-between px-3 py-2 hover:bg-(--bg-surface-hover) transition-colors"
                                            onClick={() => setStagedExpanded(!stagedExpanded)}
                                        >
                                            <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-(--fg-secondary) font-semibold">
                                                {stagedExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                                                {t('git.stagedSection')}
                                                <span className="text-(--accent-mention) font-normal normal-case">
                                                    ({stagedFiles.length})
                                                </span>
                                            </div>
                                            {stagedFiles.length > 0 && (
                                                <button
                                                    className="text-[10px] px-2 py-0.5 rounded-[calc(var(--panel-radius)*0.45)] text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface)"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        runAction('unstage-all', () => onUnstageAll());
                                                    }}
                                                    disabled={busyAction === 'unstage-all'}
                                                >
                                                    {t('git.unstageAll')}
                                                </button>
                                            )}
                                        </button>
                                        {stagedExpanded && stagedFiles.length > 0 && (
                                            <div className="px-2 pb-2">
                                                {stagedFiles.map(file => (
                                                    <React.Fragment key={`staged-${file.path}`}>
                                                        <FileRow file={file} isStaged={true} />
                                                        {diffs[file.path]?.expanded && (
                                                            <div className="ml-6 mr-2 mb-1 rounded-[calc(var(--panel-radius)*0.55)] bg-(--bg-app) border border-(--border-subtle) p-2 text-[10px] font-mono whitespace-pre-wrap overflow-x-auto max-h-48 overflow-y-auto">
                                                                {diffs[file.path]?.loading && <div className="text-(--fg-tertiary)">{t('common.loading')}</div>}
                                                                {diffs[file.path]?.error && <div className="text-(--state-danger)">{diffs[file.path]?.error}</div>}
                                                                {!diffs[file.path]?.loading && !diffs[file.path]?.error && (
                                                                    <pre className="text-(--fg-secondary)">{diffs[file.path]?.staged || t('git.noChanges')}</pre>
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
                                            className="w-full flex items-center justify-between px-3 py-2 hover:bg-(--bg-surface-hover) transition-colors"
                                            onClick={() => setUnstagedExpanded(!unstagedExpanded)}
                                        >
                                            <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wider text-(--fg-secondary) font-semibold">
                                                {unstagedExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                                                {t('git.changes')}
                                                <span className="text-(--accent-warning) font-normal normal-case">
                                                    ({unstagedFiles.length})
                                                </span>
                                            </div>
                                            {unstagedFiles.length > 0 && (
                                                <button
                                                    className="text-[10px] px-2 py-0.5 rounded-[calc(var(--panel-radius)*0.45)] text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface)"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        runAction('stage-all', () => onStageAll());
                                                    }}
                                                    disabled={busyAction === 'stage-all'}
                                                >
                                                    {t('git.stageAll')}
                                                </button>
                                            )}
                                        </button>
                                        {unstagedExpanded && unstagedFiles.length > 0 && (
                                            <div className="px-2 pb-2">
                                                {unstagedFiles.map(file => (
                                                    <React.Fragment key={`unstaged-${file.path}`}>
                                                        <FileRow file={file} isStaged={false} />
                                                        {diffs[file.path]?.expanded && !file.untracked && (
                                                            <div className="ml-6 mr-2 mb-1 rounded-[calc(var(--panel-radius)*0.55)] bg-(--bg-app) border border-(--border-subtle) p-2 text-[10px] font-mono whitespace-pre-wrap overflow-x-auto max-h-48 overflow-y-auto">
                                                                {diffs[file.path]?.loading && <div className="text-(--fg-tertiary)">{t('common.loading')}</div>}
                                                                {diffs[file.path]?.error && <div className="text-(--state-danger)">{diffs[file.path]?.error}</div>}
                                                                {!diffs[file.path]?.loading && !diffs[file.path]?.error && (
                                                                    <pre className="text-(--fg-secondary)">{diffs[file.path]?.unstaged || t('git.noChanges')}</pre>
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
                    <div className="p-3 text-[10px] text-(--state-danger) break-all border-t border-(--border-subtle) bg-[color-mix(in_srgb,var(--state-danger)_5%,transparent)]">
                        {error || filesError || actionError}
                    </div>
                )}
            </div>
        </div>
    );
};
