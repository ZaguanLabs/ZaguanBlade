'use client';
import React, { useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { ToolCall } from '../types/chat';
import { Zap, CheckCircle2, XCircle, Loader2, Copy, Check, ChevronRight, ChevronDown, RotateCcw, StopCircle } from 'lucide-react';

const COMPLETE_FADE_DELAY_MS = 250;
const COMPLETE_VISUAL_HOLD_MS = 1100;

interface ToolCallDisplayProps {
    toolCall: ToolCall;
    status?: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
    result?: string;
    onStopCommand?: () => void;
    onUndo?: () => void;
    onOpenFile?: (path: string) => void;
}

export const ToolCallDisplay: React.FC<ToolCallDisplayProps> = ({
    toolCall,
    status = 'pending',
    result,
    onStopCommand,
    onUndo,
    onOpenFile
}) => {
    const { t } = useTranslation();
    const [copied, setCopied] = useState(false);
    const [isExpanded, setIsExpanded] = useState(false);
    const [shouldFadeComplete, setShouldFadeComplete] = useState(status === 'complete');
    const previousStatusRef = useRef(status);
    const isRunCommand = toolCall.function.name === 'run_command';

    useEffect(() => {
        const previousStatus = previousStatusRef.current;
        previousStatusRef.current = status;

        if (status !== 'complete') {
            setShouldFadeComplete(false);
            return;
        }

        if (previousStatus === 'complete') {
            setShouldFadeComplete(true);
            return;
        }

        setShouldFadeComplete(false);
        const timerId = window.setTimeout(() => {
            setShouldFadeComplete(true);
        }, COMPLETE_VISUAL_HOLD_MS);

        return () => {
            window.clearTimeout(timerId);
        };
    }, [status]);

    const handleCopyCommand = useCallback(async (command: string) => {
        try {
            await navigator.clipboard.writeText(command);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy command:', err);
        }
    }, []);

    const getStatusTone = () => {
        switch (status) {
            case 'executing':
                return 'var(--accent-primary)';
            case 'complete':
                return 'var(--accent-green)';
            case 'error':
                return 'var(--accent-error)';
            case 'skipped':
                return 'var(--accent-warning)';
            default:
                return 'var(--accent-purple)';
        }
    };

    const getStatusIcon = () => {
        const style = { color: getStatusTone() };
        switch (status) {
            case 'executing':
                return <Loader2 className="w-3.5 h-3.5 animate-spin" style={style} />;
            case 'complete':
                return <CheckCircle2 className="w-3.5 h-3.5" style={style} />;
            case 'error':
                return <XCircle className="w-3.5 h-3.5" style={style} />;
            case 'skipped':
                return <XCircle className="w-3.5 h-3.5" style={style} />;
            default:
                return <Zap className="w-3.5 h-3.5" style={style} />;
        }
    };

    const getStatusText = () => {
        switch (status) {
            case 'executing':
                return 'Executing';
            case 'complete':
                return 'Complete';
            case 'error':
                return 'Failed';
            case 'skipped':
                return 'Skipped';
            default:
                return 'Pending';
        }
    };

    const isComplete = status === 'complete';
    const isVisuallyComplete = isComplete && shouldFadeComplete;

    // Get friendly tool name
    const getFriendlyToolName = (name: string, args?: Record<string, unknown>): string => {
        // Special handling for apply_patch to show patch count
        if (name === 'apply_patch' && args) {
            const patches = args.patches as Array<unknown> | undefined;
            if (patches && patches.length > 1) {
                return `Applying ${patches.length} Code Changes`;
            }
        }

        const nameMap: Record<string, string> = {
            'apply_patch': 'Applying Code Changes',
            'edit_file': 'Editing File',
            'read_file': 'Reading File',
            'write_file': 'Writing File',
            'list_files': 'Listing Files',
            'grep_search': 'Searching Code',
            'run_command': 'Running Command',
            'create_file': 'Creating File',
            'delete_file': 'Deleting File',
            'list_directory': 'Listing Directory',
            'get_workspace_structure': 'Analyzing Workspace',
            'codebase_search': 'Searching Codebase',
            'get_editor_state': 'Getting Editor State',
            'read_file_range': 'Reading File Range',
            'find_files': 'Finding Files',
            'find_files_glob': 'Finding Files (Glob)',
            'glob': 'Glob Search',
            'find_by_name': 'Find Files by Name',
            'view_file_outline': 'Viewing File Outline',
            'search_web': 'Searching Web',
            'read_url_content': 'Reading URL',
            'browser_subagent': 'Browser Agent',
            'command_status': 'Checking Command',
            'send_command_input': 'Sending Input',
            'read_terminal': 'Reading Terminal',
            'list_dir': 'Listing Directory',
            'view_file': 'Viewing File',
            'view_code_item': 'Viewing Code Item',
            'generate_image': 'Generating Image',
            'multi_replace_file_content': 'Multi-Edit File',
            'replace_file_content': 'Replacing Content',
            'write_to_file': 'Writing to File',
            'list_resources': 'Listing Resources',
            'read_resource': 'Reading Resource'
        };
        return nameMap[name] || name;
    };

    // Parse arguments to display them nicely
    let parsedArgs: Record<string, unknown> = {};
    try {
        parsedArgs = JSON.parse(toolCall.function.arguments);
    } catch {
        parsedArgs = { raw: toolCall.function.arguments };
    }

    // For run_command, extract the command for display and copy
    const commandText = isRunCommand ? (parsedArgs.command as string || parsedArgs.CommandLine as string || '') : '';
    const cwdText = isRunCommand ? (parsedArgs.cwd as string || parsedArgs.Cwd as string || '') : '';
    const pathText = (
        parsedArgs.path as string
        || parsedArgs.Path as string
        || parsedArgs.file_path as string
        || parsedArgs.filePath as string
        || parsedArgs.filepath as string
        || parsedArgs.filename as string
        || parsedArgs.TargetFile as string
        || parsedArgs.target_file as string
        || parsedArgs.absolute_path as string
        || ''
    );
    
    // For search tools, extract the search query
    const searchQuery = (parsedArgs.pattern as string || parsedArgs.query as string || parsedArgs.regex as string || parsedArgs.Query as string || '');
    const filenameOnlyTools = new Set([
        'read_file',
        'read_file_range',
        'write_file',
        'apply_patch',
        'edit_file',
        'create_file',
        'delete_file',
        'list_files',
        'get_workspace_structure',
        'view_file',
        'view_file_outline',
        'write_to_file',
        'replace_file_content',
        'multi_replace_file_content'
    ]);
    const getLastPathSegments = (value: string, count: number) => {
        const parts = value.split(/[/\\]/).filter(Boolean);
        return parts.slice(-count).join('/');
    };
    const displayPathText = toolCall.function.name === 'list_directory'
        ? getLastPathSegments(pathText, 2) || pathText
        : filenameOnlyTools.has(toolCall.function.name)
            ? pathText.split(/[/\\]/).pop() || pathText
            : pathText;
    const detailItems = [
        pathText ? { label: 'Path', value: pathText } : null,
        searchQuery ? { label: 'Query', value: searchQuery } : null,
        result ? { label: status === 'error' ? 'Error' : 'Result', value: result } : null,
    ].filter((item): item is { label: string; value: string } => !!item);

    // Compact inline display for most tools, expanded for run_command
    if (!isRunCommand) {
        return (
            <div
                className={`group/tool border-l-2 pl-2.5 text-[11px] transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-45' : 'opacity-100'}`}
                style={{ borderLeftColor: getStatusTone() }}
            >
                <div className="flex items-start gap-2 py-1">
                    <div className="flex h-5 w-5 shrink-0 items-center justify-center text-(--fg-tertiary)">
                        {getStatusIcon()}
                    </div>
                    <div className="min-w-0 flex-1 space-y-0.5">
                        <div className="flex items-start gap-2">
                            <div className="min-w-0 flex flex-1 items-center gap-2">
                                <span className={`shrink-0 text-[11px] font-medium ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                                    {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                                </span>
                                {displayPathText && (
                                    <button
                                        type="button"
                                        onClick={() => onOpenFile?.(pathText || displayPathText)}
                                        disabled={!onOpenFile}
                                        className={`min-w-0 flex-1 truncate text-left text-[10px] transition-colors ${onOpenFile
                                            ? isVisuallyComplete
                                                ? 'text-(--fg-tertiary) hover:text-(--fg-secondary)'
                                                : 'text-(--fg-secondary) hover:text-(--fg-primary)'
                                            : 'text-(--fg-tertiary)'
                                            }`}
                                        title={pathText || displayPathText}
                                    >
                                        {displayPathText}
                                    </button>
                                )}
                                {!displayPathText && searchQuery && (
                                    <span
                                        className="min-w-0 flex-1 truncate text-[10px] font-mono text-(--fg-tertiary)"
                                        title={searchQuery}
                                    >
                                        {searchQuery}
                                    </span>
                                )}
                            </div>
                            <div className="ml-auto flex shrink-0 items-center gap-1 pl-2">
                                <span className={`text-[9px] font-semibold uppercase tracking-[0.14em] ${status === 'complete'
                                    ? 'text-(--accent-green)'
                                    : status === 'executing'
                                        ? 'text-(--accent-primary)'
                                        : status === 'error'
                                            ? 'text-(--accent-error)'
                                            : status === 'skipped'
                                                ? 'text-(--accent-warning)'
                                                : 'text-(--accent-purple)'
                                    }`}>
                                    {getStatusText()}
                                </span>
                                {detailItems.length > 0 && (
                                    <button
                                        onClick={() => setIsExpanded(!isExpanded)}
                                        className="rounded p-0.5 text-(--fg-tertiary) transition-colors hover:text-(--fg-primary)"
                                        title={isExpanded ? 'Hide details' : 'Show details'}
                                    >
                                        {isExpanded ? (
                                            <ChevronDown className="w-3 h-3" />
                                        ) : (
                                            <ChevronRight className="w-3 h-3" />
                                        )}
                                    </button>
                                )}
                            </div>
                        </div>
                        <div className="flex flex-wrap items-center gap-1.5 pl-0.5">
                            {searchQuery && displayPathText && (
                                <span
                                    className="min-w-0 max-w-full truncate text-[10px] font-mono text-(--fg-tertiary)"
                                    title={searchQuery}
                                >
                                    {searchQuery}
                                </span>
                            )}
                            {onUndo && status === 'complete' && (
                                <button
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        onUndo();
                                    }}
                                    className="flex items-center gap-1 rounded px-1 py-0.5 text-[9px] text-(--fg-tertiary) transition-colors hover:text-(--accent-error)"
                                    title="Undo changes"
                                >
                                    <RotateCcw className="w-2.5 h-2.5" />
                                    Undo
                                </button>
                            )}
                        </div>
                    </div>
                </div>
                {isExpanded && detailItems.length > 0 && (
                    <div
                        className="ml-7 border-l pl-3 pb-1 pt-1"
                        style={{ borderLeftColor: 'color-mix(in srgb, var(--border-default) 82%, transparent)' }}
                    >
                        <div className="space-y-1.5">
                            {detailItems.map((item) => (
                                <div key={item.label} className="space-y-0.5">
                                    <div className="text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                                        {item.label}
                                    </div>
                                    <div className="wrap-break-word text-[11px] leading-4 text-(--fg-primary)">
                                        {item.value}
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                )}
            </div>
        );
    }

    return (
        <div
            className={`border-l-2 pl-2.5 transition-opacity duration-1000 ease-out ${isVisuallyComplete ? 'opacity-45' : 'opacity-100'}`}
            style={{ borderLeftColor: getStatusTone() }}
        >
            <div className="flex items-center justify-between gap-2 py-1">
                <div className="flex min-w-0 items-center gap-2">
                    <div className="flex h-5 w-5 items-center justify-center text-(--fg-tertiary)">
                        {getStatusIcon()}
                    </div>
                    <div className="min-w-0">
                        <span className={`block truncate text-[11px] font-medium ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                            {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                        </span>
                    </div>
                </div>
                <div className="flex items-center gap-1.5">
                    {isRunCommand && status === 'executing' && onStopCommand && (
                        <button
                            onClick={(e) => {
                                e.stopPropagation();
                                onStopCommand();
                            }}
                            className="inline-flex h-5 w-5 items-center justify-center rounded text-(--accent-error) transition-colors hover:opacity-80"
                            title={t('toolCall.stopCommand')}
                            aria-label={t('toolCall.stopCommand')}
                        >
                            <StopCircle className="h-3.5 w-3.5" />
                        </button>
                    )}
                    <span className={`text-[9px] font-semibold uppercase tracking-[0.14em] ${status === 'complete' ? 'text-(--accent-green)' :
                        status === 'executing' ? 'text-(--accent-primary)' :
                            status === 'error' ? 'text-(--accent-error)' :
                                status === 'skipped' ? 'text-(--accent-warning)' : 'text-(--accent-purple)'
                        }`}>
                        {getStatusText()}
                    </span>
                </div>
            </div>

            {commandText && (
                <div
                    className="ml-7 border-l pl-3 pb-1 pt-1"
                    style={{ borderLeftColor: 'color-mix(in srgb, var(--border-default) 82%, transparent)' }}
                >
                    <div className="flex items-start gap-2">
                        <code className={`flex-1 break-all text-[12px] font-mono leading-5 select-text ${isVisuallyComplete ? 'text-(--fg-tertiary)' : 'text-(--fg-primary)'}`}>
                            {cwdText && <span className="text-(--fg-tertiary)">{cwdText}$ </span>}
                            {commandText}
                        </code>
                        <button
                            onClick={() => handleCopyCommand(commandText)}
                            className="group/copy shrink-0 rounded p-1 text-(--fg-tertiary) transition-colors hover:text-(--fg-primary)"
                            title={t('toolCall.copyCommand')}
                        >
                            {copied ? (
                                <Check className="w-3.5 h-3.5 text-emerald-400" />
                            ) : (
                                <Copy className="w-3.5 h-3.5 text-(--fg-tertiary) group-hover/copy:text-(--fg-primary)" />
                            )}
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
};
