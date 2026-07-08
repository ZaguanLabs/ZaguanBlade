'use client';
import React, { useState } from 'react';
import { Terminal, ChevronDown, ChevronRight, CheckCircle2, XCircle, Clock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useSmoothWheelScroll } from '../hooks/useSmoothWheelScroll';

const ORPHANED_ANSI_FINAL_BYTES = new Set(['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'S', 'T', 'f', 'h', 'l', 'm', 'n', 'r', 's', 'u']);

function shouldStripOrphanedAnsi(params: string, letter: string): boolean {
    if (!ORPHANED_ANSI_FINAL_BYTES.has(letter)) {
        return false;
    }

    if (params.length === 0) {
        return ['m', 'J', 'K', 'H', 'f'].includes(letter);
    }

    return /^[0-9;?]*$/.test(params);
}

/**
 * Aggressively strip ALL ANSI escape sequences and artifacts from terminal output.
 * This handles both complete sequences (\x1b[...m) AND orphaned bracket sequences
 * where the ESC byte was already stripped upstream (e.g. "[38;5;4m", "[1m", "[0m").
 */
export function stripAllAnsi(output: string): string {
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
        if (shouldStripOrphanedAnsi(params, letter)) return '';
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
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(true);
    const isSuccess = exitCode === 0;
    const handleOutputWheel = useSmoothWheelScroll<HTMLPreElement>();

    return (
        <div
            className="my-3 rounded-[calc(var(--panel-radius)*0.75)] overflow-hidden border"
            style={{
                backgroundColor: 'color-mix(in srgb, var(--bg-surface) 90%, var(--bg-app))',
                borderColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)',
            }}
        >
            {/* Header */}
            <button
                type="button"
                onClick={() => setIsExpanded(!isExpanded)}
                aria-expanded={isExpanded}
                className="w-full flex items-center gap-3 px-4 py-3 transition-colors text-left"
                style={{ backgroundColor: 'color-mix(in srgb, var(--bg-panel) 84%, var(--bg-app))' }}
            >
                <div className={`p-1.5 rounded-[calc(var(--panel-radius)*0.35)] ${isSuccess ? 'bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)]' : 'bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)]'}`}>
                    {isSuccess ? (
                        <CheckCircle2 aria-hidden="true" className="w-4 h-4 text-(--accent-mention)" />
                    ) : (
                        <XCircle aria-hidden="true" className="w-4 h-4 text-(--state-danger)" />
                    )}
                </div>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                        <Terminal aria-hidden="true" className="w-3.5 h-3.5 text-(--fg-tertiary)" />
                        <code className="text-sm font-mono text-(--fg-primary) truncate">
                            {command}
                        </code>
                    </div>
                    {cwd && (
                        <div className="text-xs font-mono text-(--fg-tertiary) truncate">
                            {cwd}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-3 text-xs text-(--fg-tertiary)">
                    {duration !== undefined && (
                        <div className="flex items-center gap-1">
                            <Clock aria-hidden="true" className="w-3 h-3" />
                            <span>{duration}ms</span>
                        </div>
                    )}
                    <div className={`px-2 py-0.5 rounded-[calc(var(--panel-radius)*0.35)] text-xs font-medium ${isSuccess
                        ? 'bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] text-(--accent-mention)'
                        : 'bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] text-(--state-danger)'
                        }`}>
                        {t('chat.commandOutput.exit', { code: exitCode })}
                    </div>
                    {isExpanded ? (
                        <ChevronDown aria-hidden="true" className="w-4 h-4" />
                    ) : (
                        <ChevronRight aria-hidden="true" className="w-4 h-4" />
                    )}
                </div>
            </button>

            {/* Output */}
            {isExpanded && output && (
                <div
                    className="border-t"
                    style={{ borderTopColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)' }}
                >
                    <pre
                        className="p-4 text-xs font-mono text-(--fg-primary) overflow-x-auto max-h-[400px] overflow-y-auto select-text whitespace-pre-wrap wrap-break-word"
                        style={{ backgroundColor: 'color-mix(in srgb, var(--bg-surface) 94%, var(--bg-app))' }}
                        onWheel={handleOutputWheel}
                    >
                        {stripAllAnsi(output)}
                    </pre>
                </div>
            )}

            {/* Empty output message */}
            {isExpanded && !output && (
                <div
                    className="border-t px-4 py-3 text-xs text-(--fg-tertiary) italic"
                    style={{ borderTopColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)' }}
                >
                    {t('chat.commandOutput.noOutput')}
                </div>
            )}
        </div>
    );
};
