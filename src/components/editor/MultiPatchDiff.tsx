'use client';
import React, { useState } from 'react';
import { Check, X, ChevronDown, ChevronRight, FileCode, Layers } from 'lucide-react';
import type { Change, PatchHunk } from '../../types/change';

interface MultiPatchDiffProps {
    change: Change & { change_type: 'multi_patch' };
    onAcceptAll: () => void;
    onRejectAll: () => void;
    onAcceptHunk?: (index: number) => void;
    onRejectHunk?: (index: number) => void;
}

interface HunkDisplayProps {
    hunk: PatchHunk;
    index: number;
    isExpanded: boolean;
    onToggle: () => void;
    onAccept?: () => void;
    onReject?: () => void;
}

const HunkDisplay: React.FC<HunkDisplayProps> = ({
    hunk,
    index,
    isExpanded,
    onToggle,
    onAccept,
    onReject,
}) => {
    // Truncate old_text for collapsed preview
    const previewText = hunk.old_text.length > 60
        ? hunk.old_text.substring(0, 60) + '...'
        : hunk.old_text;

    const lineInfo = hunk.start_line
        ? `lines ${hunk.start_line}${hunk.end_line ? `-${hunk.end_line}` : ''}`
        : null;

    return (
        <div className="border-b border-zinc-800/60 last:border-b-0">
            {/* Hunk Header */}
            <div
                className="flex items-center justify-between px-3 py-2 bg-zinc-900/40 cursor-pointer hover:bg-zinc-800/40 transition-colors"
                onClick={onToggle}
            >
                <div className="flex items-center gap-2 flex-1 min-w-0">
                    {isExpanded ? (
                        <ChevronDown className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                    ) : (
                        <ChevronRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                    )}
                    <span className="text-xs font-medium text-zinc-400">
                        Hunk {index + 1}
                    </span>
                    {lineInfo && (
                        <span className="text-[10px] font-mono text-zinc-600 bg-zinc-800/60 px-1.5 py-0.5 rounded">
                            {lineInfo}
                        </span>
                    )}
                    {!isExpanded && (
                        <span className="text-xs font-mono text-zinc-600 truncate">
                            {previewText}
                        </span>
                    )}
                </div>

                {/* Per-hunk actions (optional) */}
                {(onAccept || onReject) && (
                    <div className="flex items-center gap-1 ml-2" onClick={e => e.stopPropagation()}>
                        {onAccept && (
                            <button
                                onClick={onAccept}
                                className="p-1 rounded text-emerald-400 hover:bg-emerald-900/30 transition-colors"
                                title="Accept this hunk"
                            >
                                <Check className="w-3.5 h-3.5" />
                            </button>
                        )}
                        {onReject && (
                            <button
                                onClick={onReject}
                                className="p-1 rounded text-red-400 hover:bg-red-900/30 transition-colors"
                                title="Reject this hunk"
                            >
                                <X className="w-3.5 h-3.5" />
                            </button>
                        )}
                    </div>
                )}
            </div>

            {/* Hunk Content (expanded) */}
            {isExpanded && (
                <div className="flex flex-col">
                    {/* Original (removed) */}
                    <div className="bg-red-900/10 border-l-2 border-red-500/50">
                        <div className="px-3 py-1 bg-red-900/20">
                            <span className="text-[10px] font-semibold text-red-400 uppercase tracking-wide">
                                - Remove
                            </span>
                        </div>
                        <pre className="px-3 py-2 font-mono text-xs text-red-200/80 whitespace-pre-wrap leading-relaxed overflow-x-auto">
                            {hunk.old_text}
                        </pre>
                    </div>

                    {/* Modified (added) */}
                    <div className="bg-emerald-900/10 border-l-2 border-emerald-500/50">
                        <div className="px-3 py-1 bg-emerald-900/20">
                            <span className="text-[10px] font-semibold text-emerald-400 uppercase tracking-wide">
                                + Add
                            </span>
                        </div>
                        <pre className="px-3 py-2 font-mono text-xs text-emerald-200 whitespace-pre-wrap leading-relaxed overflow-x-auto">
                            {hunk.new_text}
                        </pre>
                    </div>
                </div>
            )}
        </div>
    );
};

export const MultiPatchDiff: React.FC<MultiPatchDiffProps> = ({
    change,
    onAcceptAll,
    onRejectAll,
    onAcceptHunk,
    onRejectHunk,
}) => {
    const [expandedHunks, setExpandedHunks] = useState<Set<number>>(() => {
        // Auto-expand first hunk, collapse rest if many
        if (change.patches.length <= 3) {
            return new Set(change.patches.map((_, i) => i));
        }
        return new Set([0]);
    });

    const filename = change.path.split('/').pop() || change.path;
    const patchCount = change.patches.length;

    const toggleHunk = (index: number) => {
        setExpandedHunks(prev => {
            const next = new Set(prev);
            if (next.has(index)) {
                next.delete(index);
            } else {
                next.add(index);
            }
            return next;
        });
    };

    const expandAll = () => {
        setExpandedHunks(new Set(change.patches.map((_, i) => i)));
    };

    const collapseAll = () => {
        setExpandedHunks(new Set());
    };

    return (
        <div className="absolute top-4 left-4 right-4 max-h-[70vh] bg-[#1e1e1e] border border-blue-500/50 rounded-lg shadow-2xl z-50 flex flex-col overflow-hidden">
            {/* Header */}
            <div className="flex items-center justify-between bg-blue-900/20 px-3 py-2 border-b border-blue-500/30">
                <div className="flex items-center gap-2">
                    <Layers className="w-4 h-4 text-blue-400" />
                    <span className="text-[10px] font-semibold text-blue-400 uppercase tracking-wide">
                        {change.applied ? 'Applied Changes (Undoable)' : 'Multi-Patch Edit'}
                    </span>
                    <span className="text-xs text-zinc-400 font-mono">{filename}</span>
                    <span className="text-[10px] font-mono text-blue-300 bg-blue-900/40 px-1.5 py-0.5 rounded">
                        {patchCount} change{patchCount !== 1 ? 's' : ''}
                    </span>
                </div>
                <div className="flex items-center gap-1.5">
                    {/* Expand/Collapse toggle */}
                    <button
                        onClick={expandedHunks.size === patchCount ? collapseAll : expandAll}
                        className="px-2 py-1 text-[10px] text-zinc-400 hover:text-zinc-200 transition-colors"
                    >
                        {expandedHunks.size === patchCount ? 'Collapse All' : 'Expand All'}
                    </button>

                    {/* Main actions */}
                    {change.applied ? (
                        <>
                            <button
                                onClick={onAcceptAll}
                                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-zinc-700 hover:bg-zinc-600 text-white transition-colors"
                            >
                                <Check className="w-3.5 h-3.5" />
                                Done
                            </button>
                            <button
                                onClick={onRejectAll}
                                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-blue-600/90 hover:bg-blue-500 text-white transition-colors"
                            >
                                <X className="w-3.5 h-3.5" />
                                Undo Changes
                            </button>
                        </>
                    ) : (
                        <>
                            <button
                                onClick={onAcceptAll}
                                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-emerald-600/90 hover:bg-emerald-500 text-white transition-colors"
                            >
                                <Check className="w-3.5 h-3.5" />
                                Accept All
                            </button>
                            <button
                                onClick={onRejectAll}
                                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-red-600/90 hover:bg-red-500 text-white transition-colors"
                            >
                                <X className="w-3.5 h-3.5" />
                                Reject All
                            </button>
                        </>
                    )}
                </div>
            </div>

            {/* File path */}
            <div className="flex items-center gap-2 px-3 py-1.5 bg-zinc-900/60 border-b border-zinc-800/50">
                <FileCode className="w-3.5 h-3.5 text-zinc-500" />
                <span className="text-xs font-mono text-zinc-400">{change.path}</span>
            </div>

            {/* Hunks list */}
            <div className="flex-1 overflow-auto">
                {change.patches.map((hunk, index) => (
                    <HunkDisplay
                        key={index}
                        hunk={hunk}
                        index={index}
                        isExpanded={expandedHunks.has(index)}
                        onToggle={() => toggleHunk(index)}
                        onAccept={onAcceptHunk ? () => onAcceptHunk(index) : undefined}
                        onReject={onRejectHunk ? () => onRejectHunk(index) : undefined}
                    />
                ))}
            </div>

            {/* Footer summary */}
            <div className="flex items-center justify-between px-3 py-2 bg-zinc-900/40 border-t border-zinc-800/50 text-[10px] text-zinc-500">
                <span>
                    {expandedHunks.size} of {patchCount} hunks expanded
                </span>
                <span className="font-mono">
                    All changes will be applied atomically
                </span>
            </div>
        </div>
    );
};

export default MultiPatchDiff;
