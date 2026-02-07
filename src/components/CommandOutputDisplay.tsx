'use client';
import React, { useState } from 'react';
import { Terminal, ChevronDown, ChevronRight, CheckCircle2, XCircle, Clock } from 'lucide-react';
/**
 * Aggressively strip ALL ANSI escape sequences and artifacts from terminal output.
 * This handles both complete sequences (\x1b[...m) AND orphaned bracket sequences
 * where the ESC byte was already stripped upstream (e.g. "[38;5;4m", "[1m", "[0m").
 */
function stripAllAnsi(output: string): string {
    let result = output;
    // 1. Strip complete ANSI escape sequences (ESC + bracket/other sequences)
    //    CSI: \x1b[ ... letter
    //    OSC: \x1b] ... BEL or ST
    //    Other: \x1b + various
    result = result.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, '');
    result = result.replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '');
    result = result.replace(/\x1b[PX^_][^\x1b]*\x1b\\/g, '');
    result = result.replace(/\x1b[\x20-\x7e]/g, '');
    // 2. Strip orphaned CSI bracket sequences where ESC byte is already gone
    //    These look like: [0m, [1m, [38;5;4m, [0;1m, [38;5;2m, [39;49m etc.
    result = result.replace(/\[([0-9;?]*)([A-Za-z])/g, (match, params, letter) => {
        // Only strip if it looks like an ANSI CSI parameter sequence
        // (digits, semicolons, question marks followed by a single letter)
        if (/^[0-9;?]*$/.test(params)) return '';
        return match;
    });
    // 3. Strip any remaining bare ESC bytes
    result = result.replace(/\x1b/g, '');
    // 4. Strip BEL characters
    result = result.replace(/\x07/g, '');
    // 5. Strip stray control characters (keep \n, \r, \t)
    result = result.replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, '');
    // 6. Clean up excessive blank lines
    result = result.replace(/\n{3,}/g, '\n\n');
    return result.trim();
}

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
                    <pre className="p-4 text-xs font-mono text-zinc-300 overflow-x-auto max-h-[400px] overflow-y-auto bg-[#1e1e1e] select-text whitespace-pre-wrap break-words">
                        {stripAllAnsi(output)}
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
