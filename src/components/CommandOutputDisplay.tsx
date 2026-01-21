'use client';
import React, { useState } from 'react';
import { Terminal, ChevronDown, ChevronRight, CheckCircle2, XCircle, Clock } from 'lucide-react';

import Ansi from 'ansi-to-react';

interface CommandOutputDisplayProps {
    command: string;
    cwd?: string;
    output: string;
    exitCode: number;
    duration?: number;
}

export const CommandOutputDisplay: React.FC<CommandOutputDisplayProps> = ({
    command,
    cwd,
    output,
    exitCode,
    duration,
}) => {
    const [isExpanded, setIsExpanded] = useState(true);
    const isSuccess = exitCode === 0;

    return (
        <div className="my-3 bg-[#1e1e1e] border border-[#3e3e42] rounded-lg overflow-hidden">
            {/* Header */}
            <button
                onClick={() => setIsExpanded(!isExpanded)}
                className="w-full flex items-center gap-3 px-4 py-3 bg-[#252526] hover:bg-[#2d2d2d] transition-colors text-left"
            >
                <div className={`p-1.5 rounded ${isSuccess ? 'bg-emerald-500/10' : 'bg-red-500/10'}`}>
                    {isSuccess ? (
                        <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                    ) : (
                        <XCircle className="w-4 h-4 text-red-500" />
                    )}
                </div>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                        <Terminal className="w-3.5 h-3.5 text-zinc-400" />
                        <code className="text-sm font-mono text-white truncate">
                            {command}
                        </code>
                    </div>
                    {cwd && (
                        <div className="text-xs font-mono text-zinc-500 truncate">
                            {cwd}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-3 text-xs text-zinc-400">
                    {duration !== undefined && (
                        <div className="flex items-center gap-1">
                            <Clock className="w-3 h-3" />
                            <span>{duration}ms</span>
                        </div>
                    )}
                    <div className={`px-2 py-0.5 rounded text-xs font-medium ${isSuccess
                        ? 'bg-emerald-500/10 text-emerald-400'
                        : 'bg-red-500/10 text-red-400'
                        }`}>
                        Exit {exitCode}
                    </div>
                    {isExpanded ? (
                        <ChevronDown className="w-4 h-4" />
                    ) : (
                        <ChevronRight className="w-4 h-4" />
                    )}
                </div>
            </button>

            {/* Output */}
            {isExpanded && output && (
                <div className="border-t border-[#3e3e42]">
                    <pre className="p-4 text-xs font-mono text-zinc-300 overflow-x-auto max-h-[400px] overflow-y-auto bg-[#1e1e1e] select-text">
                        <Ansi>{output}</Ansi>
                    </pre>
                </div>
            )}

            {/* Empty output message */}
            {isExpanded && !output && (
                <div className="border-t border-[#3e3e42] px-4 py-3 text-xs text-zinc-500 italic">
                    No output
                </div>
            )}
        </div>
    );
};
