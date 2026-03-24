import React, { useEffect, useMemo, useState } from 'react';
import { Bot, Clock3, ListChecks, Loader2, PauseCircle, ShieldAlert, Sparkles, Square, Wand2 } from 'lucide-react';
import type { StructuredAction, TodoItem } from '../types/events';
import type { QueuedRequest, ToolActivityState } from '../types/chat';

interface AgentRunStatusBarProps {
    loading: boolean;
    pendingActions: StructuredAction[] | null;
    toolActivity?: ToolActivityState | null;
    activeTodos: TodoItem[];
    queuedRequests: QueuedRequest[];
    onStop?: () => void;
    onJumpToApproval?: () => void;
    onJumpToActiveStep?: () => void;
    onJumpToTaskPanel?: () => void;
    onJumpToQueue?: () => void;
}

type RunPhaseKey = 'approval' | 'tools' | 'thinking' | 'tasks' | 'queue';

function phaseLabel(phase: RunPhaseKey): string {
    const labels: Record<RunPhaseKey, string> = {
        approval: 'Awaiting approval',
        tools: 'Using tools',
        thinking: 'Thinking',
        tasks: 'Task plan',
        queue: 'Queued',
    };
    return labels[phase];
}

function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    if (totalSeconds < 60) {
        return `${totalSeconds}s`;
    }
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}m ${seconds.toString().padStart(2, '0')}s`;
}

function friendlyToolName(name: string): string {
    const labels: Record<string, string> = {
        apply_patch: 'Applying patch',
        edit_file: 'Editing file',
        read_file: 'Reading file',
        write_to_file: 'Writing file',
        grep_search: 'Searching code',
        code_search: 'Researching codebase',
        find_by_name: 'Finding files',
        list_dir: 'Listing directory',
        read_terminal: 'Reading terminal',
        command_status: 'Checking command',
        run_command: 'Running command',
        ask_user_question: 'Waiting for your choice',
        todo_list: 'Updating plan',
        browser_preview: 'Opening preview',
        read_url_content: 'Reading URL',
        search_web: 'Searching web',
    };
    return labels[name] ?? name.split('_').join(' ');
}

export const AgentRunStatusBar: React.FC<AgentRunStatusBarProps> = ({
    loading,
    pendingActions,
    toolActivity,
    activeTodos,
    queuedRequests,
    onStop,
    onJumpToApproval,
    onJumpToActiveStep,
    onJumpToTaskPanel,
    onJumpToQueue,
}) => {
    const [now, setNow] = useState(() => Date.now());

    useEffect(() => {
        const shouldTick = loading || !!toolActivity;
        if (!shouldTick) {
            return;
        }
        const timerId = window.setInterval(() => setNow(Date.now()), 1000);
        return () => window.clearInterval(timerId);
    }, [loading, toolActivity]);

    const inProgressTodo = useMemo(
        () => activeTodos.find((todo) => todo.status === 'in_progress') ?? null,
        [activeTodos],
    );

    const pendingApprovalCount = pendingActions?.length ?? 0;
    const queuedCount = queuedRequests.length;
    const hasVisibleState = loading || !!toolActivity || pendingApprovalCount > 0 || !!inProgressTodo || queuedCount > 0;

    const statusModel = useMemo(() => {
        if (pendingApprovalCount > 0) {
            return {
                phase: 'approval' as RunPhaseKey,
                tone: 'amber' as const,
                icon: ShieldAlert,
                title: pendingApprovalCount === 1 ? 'Waiting for approval' : `Waiting on ${pendingApprovalCount} approvals`,
                detail: 'Review the pending command actions to let the run continue.',
            };
        }
        if (toolActivity) {
            return {
                phase: 'tools' as RunPhaseKey,
                tone: 'indigo' as const,
                icon: Wand2,
                title: friendlyToolName(toolActivity.toolName),
                detail: toolActivity.filePath
                    ? `${toolActivity.action} ${toolActivity.filePath}`
                    : toolActivity.action,
            };
        }
        if (loading) {
            return {
                phase: 'thinking' as RunPhaseKey,
                tone: 'emerald' as const,
                icon: Sparkles,
                title: 'Agent is working',
                detail: 'Streaming a response and deciding the next step.',
            };
        }
        if (inProgressTodo) {
            return {
                phase: 'tasks' as RunPhaseKey,
                tone: 'sky' as const,
                icon: ListChecks,
                title: 'Task plan active',
                detail: inProgressTodo.activeForm,
            };
        }
        if (queuedCount > 0) {
            return {
                phase: 'queue' as RunPhaseKey,
                tone: 'zinc' as const,
                icon: PauseCircle,
                title: queuedCount === 1 ? '1 follow-up queued' : `${queuedCount} follow-ups queued`,
                detail: 'Queued requests will be ready to send from the composer.',
            };
        }
        return null;
    }, [inProgressTodo, loading, pendingApprovalCount, queuedCount, toolActivity]);

    const toolElapsed = toolActivity ? formatDuration(now - toolActivity.startedAt) : null;

    if (!hasVisibleState || !statusModel) {
        return null;
    }

    const toneClasses: Record<typeof statusModel.tone, { border: string; bg: string; icon: string; badge: string; text: string; subtext: string; button: string }> = {
        amber: {
            border: 'border-amber-500/20',
            bg: 'bg-amber-500/8',
            icon: 'text-amber-300',
            badge: 'border-amber-500/20 bg-amber-500/10 text-amber-200',
            text: 'text-amber-100',
            subtext: 'text-amber-200/80',
            button: 'border-amber-400/25 bg-amber-500/10 text-amber-100 hover:bg-amber-500/16',
        },
        indigo: {
            border: 'border-indigo-500/20',
            bg: 'bg-indigo-500/8',
            icon: 'text-indigo-300',
            badge: 'border-indigo-500/20 bg-indigo-500/10 text-indigo-200',
            text: 'text-indigo-100',
            subtext: 'text-indigo-200/80',
            button: 'border-indigo-400/25 bg-indigo-500/10 text-indigo-100 hover:bg-indigo-500/16',
        },
        emerald: {
            border: 'border-emerald-500/20',
            bg: 'bg-emerald-500/8',
            icon: 'text-emerald-300',
            badge: 'border-emerald-500/20 bg-emerald-500/10 text-emerald-200',
            text: 'text-emerald-100',
            subtext: 'text-emerald-200/80',
            button: 'border-emerald-400/25 bg-emerald-500/10 text-emerald-100 hover:bg-emerald-500/16',
        },
        sky: {
            border: 'border-sky-500/20',
            bg: 'bg-sky-500/8',
            icon: 'text-sky-300',
            badge: 'border-sky-500/20 bg-sky-500/10 text-sky-200',
            text: 'text-sky-100',
            subtext: 'text-sky-200/80',
            button: 'border-sky-400/25 bg-sky-500/10 text-sky-100 hover:bg-sky-500/16',
        },
        zinc: {
            border: 'border-(--border-subtle)',
            bg: 'bg-(--bg-surface)/60',
            icon: 'text-(--fg-secondary)',
            badge: 'border-(--border-subtle) bg-(--bg-app)/70 text-(--fg-secondary)',
            text: 'text-(--fg-primary)',
            subtext: 'text-(--fg-secondary)',
            button: 'border-(--border-subtle) bg-(--bg-app)/70 text-(--fg-secondary) hover:bg-(--bg-surface-hover)',
        },
    };

    const tone = toneClasses[statusModel.tone];
    const StatusIcon = statusModel.icon;
    const canJumpToActiveStep = (!!toolActivity || loading) && !!onJumpToActiveStep;
    const canJumpToApproval = pendingApprovalCount > 0 && !!onJumpToApproval;
    const canJumpToTasks = !!inProgressTodo && !!onJumpToTaskPanel;
    const canJumpToQueue = queuedCount > 0 && !!onJumpToQueue;
    const phaseItems = [
        {
            key: 'thinking' as RunPhaseKey,
            label: phaseLabel('thinking'),
            icon: Sparkles,
            isPresent: loading,
            isActive: statusModel.phase === 'thinking',
            onClick: canJumpToActiveStep ? onJumpToActiveStep : undefined,
        },
        {
            key: 'tools' as RunPhaseKey,
            label: phaseLabel('tools'),
            icon: Wand2,
            isPresent: !!toolActivity,
            isActive: statusModel.phase === 'tools',
            onClick: canJumpToActiveStep ? onJumpToActiveStep : undefined,
        },
        {
            key: 'approval' as RunPhaseKey,
            label: phaseLabel('approval'),
            icon: ShieldAlert,
            isPresent: pendingApprovalCount > 0,
            isActive: statusModel.phase === 'approval',
            onClick: canJumpToApproval ? onJumpToApproval : undefined,
        },
        {
            key: 'tasks' as RunPhaseKey,
            label: phaseLabel('tasks'),
            icon: ListChecks,
            isPresent: !!inProgressTodo,
            isActive: statusModel.phase === 'tasks',
            onClick: canJumpToTasks ? onJumpToTaskPanel : undefined,
        },
        {
            key: 'queue' as RunPhaseKey,
            label: phaseLabel('queue'),
            icon: PauseCircle,
            isPresent: queuedCount > 0,
            isActive: statusModel.phase === 'queue',
            onClick: canJumpToQueue ? onJumpToQueue : undefined,
        },
    ];
    const visiblePhaseItems = phaseItems.filter((phaseItem) => phaseItem.isPresent);

    return (
        <div className={`border-t ${tone.border} ${tone.bg} backdrop-blur-sm`}>
            <div className="flex items-start gap-3 px-3 py-2.5">
                <div className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl border ${tone.border} ${tone.bg}`}>
                    {loading && !toolActivity && pendingApprovalCount === 0 ? (
                        <Loader2 className={`h-4 w-4 animate-spin ${tone.icon}`} />
                    ) : (
                        <StatusIcon className={`h-4 w-4 ${tone.icon}`} />
                    )}
                </div>
                <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                        <span className={`inline-flex items-center rounded-md border px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.16em] ${tone.badge}`}>
                            {phaseLabel(statusModel.phase)}
                        </span>
                        <span className={`text-[11px] font-semibold uppercase tracking-[0.16em] ${tone.text}`}>
                            {statusModel.title}
                        </span>
                        {toolElapsed && (
                            <span className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-medium ${tone.badge}`}>
                                <Clock3 className="h-3 w-3" />
                                {toolElapsed}
                            </span>
                        )}
                        {!toolElapsed && loading && (
                            <span className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-medium ${tone.badge}`}>
                                <Bot className="h-3 w-3" />
                                Live
                            </span>
                        )}
                        {pendingApprovalCount > 0 && (
                            <span className={`inline-flex items-center rounded-md border px-1.5 py-0.5 text-[10px] font-medium ${tone.badge}`}>
                                {pendingApprovalCount} pending
                            </span>
                        )}
                    </div>
                    <div className={`mt-1 text-[12px] leading-5 ${tone.subtext}`}>
                        {statusModel.detail}
                    </div>
                    {visiblePhaseItems.length > 1 && (
                        <div className="mt-2 flex flex-wrap items-center gap-1.5">
                            {visiblePhaseItems.map((phaseItem) => {
                            const PhaseIcon = phaseItem.icon;
                            const isInteractive = !!phaseItem.onClick;
                            const className = phaseItem.isActive
                                ? `border ${tone.border} ${tone.bg} ${tone.text} shadow-sm`
                                : 'border border-(--border-subtle) bg-(--bg-app)/70 text-(--fg-secondary) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)';

                            if (isInteractive) {
                                return (
                                    <button
                                        key={phaseItem.key}
                                        type="button"
                                        onClick={phaseItem.onClick}
                                        className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-[10px] font-medium transition-all duration-200 ${className}`}
                                    >
                                        <PhaseIcon className="h-3 w-3" />
                                        <span>{phaseItem.label}</span>
                                    </button>
                                );
                            }

                            return (
                                <span
                                    key={phaseItem.key}
                                    className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-[10px] font-medium transition-all duration-200 ${className}`}
                                >
                                    <PhaseIcon className="h-3 w-3" />
                                    <span>{phaseItem.label}</span>
                                </span>
                            );
                        })}
                        </div>
                    )}
                </div>
                {onStop && (loading || !!toolActivity) && pendingApprovalCount === 0 && (
                    <button
                        type="button"
                        onClick={onStop}
                        className={`inline-flex shrink-0 items-center gap-1 rounded-lg border px-2 py-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] transition-colors ${tone.button}`}
                    >
                        <Square className="h-3 w-3" />
                        Stop
                    </button>
                )}
            </div>
        </div>
    );
};
