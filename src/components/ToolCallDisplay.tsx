'use client';
import React from 'react';
import { ToolCall } from '../types/chat';
import { Zap, CheckCircle2, XCircle, Loader2 } from 'lucide-react';

interface ToolCallDisplayProps {
    toolCall: ToolCall;
    status?: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
    result?: string;
}

export const ToolCallDisplay: React.FC<ToolCallDisplayProps> = ({
    toolCall,
    status = 'pending'
}) => {
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
                return `📝 Applying ${patches.length} Code Changes`;
            }
        }

        const nameMap: Record<string, string> = {
            'apply_patch': '📝 Applying Code Changes',
            'edit_file': '✏️ Editing File',
            'read_file': '📖 Reading File',
            'write_file': '💾 Writing File',
            'list_files': '📂 Listing Files',
            'grep_search': '🔍 Searching Code',
            'run_command': '⚙️ Running Command',
            'create_file': '📄 Creating File',
            'delete_file': '🗑️ Deleting File',
            'list_directory': '📁 Listing Directory',
            'get_workspace_structure': '🗂️ Analyzing Workspace'
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

    // Hide arguments for certain tools
    const shouldHideArgs = ['apply_patch'].includes(toolCall.function.name);

    return (
        <div className={`border rounded-md overflow-hidden transition-all duration-200 ${getStatusColor()}`}>
            {/* Header */}
            <div className="flex items-center justify-between px-3 py-2 bg-zinc-900/40">
                <div className="flex items-center gap-2">
                    {getStatusIcon()}
                    <span className="text-xs font-medium text-zinc-200">
                        {getFriendlyToolName(toolCall.function.name, parsedArgs)}
                    </span>
                    <span className="text-[9px] font-mono text-zinc-600 bg-zinc-900/80 px-1.5 py-0.5 rounded border border-zinc-800/50">
                        {toolCall.id?.slice(0, 8) || 'unknown'}
                    </span>
                </div>
                <span className={`text-[9px] font-mono uppercase tracking-wider font-semibold ${status === 'complete' ? 'text-emerald-400' :
                    status === 'executing' ? 'text-blue-400' :
                        status === 'error' ? 'text-red-400' :
                            status === 'skipped' ? 'text-yellow-400' : 'text-zinc-500'
                    }`}>
                    {getStatusText()}
                </span>
            </div>

            {/* Arguments - Hide for verbose tools */}
            {!shouldHideArgs && Object.keys(parsedArgs).length > 0 && (
                <div className="px-3 py-2.5 space-y-1.5 bg-zinc-950/30">
                    {Object.entries(parsedArgs).map(([key, value]) => {
                        // Truncate very long values
                        let displayValue = typeof value === 'string' ? value : JSON.stringify(value);
                        if (displayValue.length > 200) {
                            displayValue = displayValue.substring(0, 200) + '...';
                        }
                        return (
                            <div key={key} className="flex gap-3 text-xs items-start">
                                <span className="font-mono text-zinc-500 min-w-[80px] shrink-0">{key}:</span>
                                <span className="font-mono text-zinc-300 flex-1 break-all select-text">
                                    {displayValue}
                                </span>
                            </div>
                        );
                    })}
                </div>
            )}

            {/* Results hidden - user only wants to see status indicators */}
        </div>
    );
};
