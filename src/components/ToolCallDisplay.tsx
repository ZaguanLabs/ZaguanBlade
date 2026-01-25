'use client';
import React, { useState, useCallback } from 'react';
import { ToolCall } from '../types/chat';
import { Zap, CheckCircle2, XCircle, Loader2, Copy, Check, ChevronRight, ChevronDown, RotateCcw } from 'lucide-react';

interface ToolCallDisplayProps {
    toolCall: ToolCall;
    status?: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
    result?: string;
    onUndo?: () => void;
}

export const ToolCallDisplay: React.FC<ToolCallDisplayProps> = ({
    toolCall,
    status = 'pending',
    result,
    onUndo
}) => {
    const [copied, setCopied] = useState(false);
    const [isExpanded, setIsExpanded] = useState(false);
    const isRunCommand = toolCall.function.name === 'run_command';

    const handleCopyCommand = useCallback(async (command: string) => {
        try {
            await navigator.clipboard.writeText(command);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy command:', err);
        }
    }, []);
    const getStatusIcon = () => {
        switch (status) {
            case 'executing':
                return <Loader2 className="w-3.5 h-3.5 text-blue-400 animate-spin" />;
            case 'complete':
                return <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />;
            case 'error':
                return <XCircle className="w-3.5 h-3.5 text-red-400" />;
            case 'skipped':
                return <XCircle className="w-3.5 h-3.5 text-yellow-400" />;
            default:
                return <Zap className="w-3.5 h-3.5 text-purple-400" />;
        }
    };

    const getStatusColor = () => {
        switch (status) {
            case 'executing':
                return 'border-blue-500/20 bg-[#0d1117]';
            case 'complete':
                return 'border-emerald-500/20 bg-[#0d1117]';
            case 'error':
                return 'border-red-500/30 bg-[#0d1117]';
            case 'skipped':
                return 'border-yellow-500/30 bg-[#0d1117]';
            default:
                return 'border-zinc-700/50 bg-[#0d1117]';
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
    const pathText = (parsedArgs.path as string || parsedArgs.Path as string || '');
    
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

    // Compact inline display for most tools, expanded for run_command
    if (!isRunCommand) {
        // Minimal inline display for non-command tools
        return (
            <div className="flex items-center gap-2 py-1 text-[11px] text-zinc-500 group/tool">
                {getStatusIcon()}
                <span className="font-medium text-zinc-400">
                    {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                </span>
                {/* Show search query for search tools */}
                {searchQuery && (
                    <span
                        className="text-[10px] text-amber-400/80 truncate max-w-[200px] font-mono"
                        title={searchQuery}
                    >
                        "{searchQuery}"
                    </span>
                )}
                {displayPathText && (
                    <span
                        className="text-[10px] text-zinc-500 truncate max-w-[260px]"
                        title={displayPathText}
                    >
                        {displayPathText}
                    </span>
                )}
                {status === 'executing' && (
                    <span className="text-[9px] text-blue-400 animate-pulse">running...</span>
                )}
                {status === 'complete' && (
                    <span className="text-[9px] text-emerald-500">✓</span>
                )}
                {status === 'error' && (
                    <span className="text-[9px] text-red-400" title={result || 'Unknown error'}>
                        failed {result && result.length < 60 ? `- ${result.replace('tool_error: ', '')}` : ''}
                    </span>
                )}
                {/* Undo Button */}
                {onUndo && status === 'complete' && (
                    <button
                        onClick={(e) => {
                            e.stopPropagation();
                            onUndo();
                        }}
                        className="opacity-0 group-hover/tool:opacity-100 flex items-center gap-1 ml-2 px-1.5 py-0.5 rounded bg-zinc-800 hover:bg-zinc-700 text-[9px] text-zinc-400 hover:text-red-400 transition-all border border-zinc-700"
                        title="Undo changes"
                    >
                        <RotateCcw className="w-2.5 h-2.5" />
                        Undo
                    </button>
                )}
                {/* Expand button for details */}
                <button
                    onClick={() => setIsExpanded(!isExpanded)}
                    className="opacity-0 group-hover/tool:opacity-100 transition-opacity ml-auto"
                >
                    {isExpanded ? (
                        <ChevronDown className="w-3 h-3 text-zinc-600" />
                    ) : (
                        <ChevronRight className="w-3 h-3 text-zinc-600" />
                    )}
                </button>
            </div>
        );
    }

    // Expanded display for run_command with copy button
    return (
        <div className={`border rounded-md overflow-hidden transition-all duration-200 ${getStatusColor()}`}>
            {/* Header */}
            <div className="flex items-center justify-between px-2.5 py-1.5 bg-zinc-900/40">
                <div className="flex items-center gap-2">
                    {getStatusIcon()}
                    <span className="text-[11px] font-medium text-zinc-300">
                        {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                    </span>
                </div>
                <div className="flex items-center gap-2">
                    <span className={`text-[9px] font-mono uppercase tracking-wider ${status === 'complete' ? 'text-emerald-400' :
                        status === 'executing' ? 'text-blue-400' :
                            status === 'error' ? 'text-red-400' :
                                status === 'skipped' ? 'text-yellow-400' : 'text-zinc-500'
                        }`}>
                        {getStatusText()}
                    </span>
                </div>
            </div>

            {/* Command display with copy button */}
            {commandText && (
                <div className="px-2.5 py-2 bg-zinc-950/50 border-t border-zinc-800/30">
                    <div className="flex items-start gap-2">
                        <code className="flex-1 text-[11px] font-mono text-zinc-300 break-all select-text leading-relaxed">
                            {cwdText && <span className="text-zinc-600">{cwdText}$ </span>}
                            {commandText}
                        </code>
                        <button
                            onClick={() => handleCopyCommand(commandText)}
                            className="shrink-0 p-1 rounded hover:bg-zinc-800 transition-colors group/copy"
                            title="Copy command"
                        >
                            {copied ? (
                                <Check className="w-3.5 h-3.5 text-emerald-400" />
                            ) : (
                                <Copy className="w-3.5 h-3.5 text-zinc-500 group-hover/copy:text-zinc-300" />
                            )}
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
};
