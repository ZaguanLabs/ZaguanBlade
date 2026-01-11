'use client';
import React, { useState } from 'react';
import { Check, X, ChevronDown, ChevronUp, Maximize2, Minimize2 } from 'lucide-react';
import type { Change, PatchHunk } from '../../types/change';

interface InlineDiffPreviewProps {
    change: Change;
    onAccept: () => void;
    onReject: () => void;
    onExpand?: () => void;
}

const DiffLine: React.FC<{ type: 'add' | 'remove' | 'context'; content: string }> = ({ type, content }) => {
    const baseClass = "font-mono text-[11px] leading-relaxed px-2";
    const typeClasses = {
        add: "bg-emerald-900/20 text-emerald-300 border-l-2 border-emerald-500",
        remove: "bg-red-900/20 text-red-300 border-l-2 border-red-500",
        context: "text-zinc-500",
    };
    const prefix = type === 'add' ? '+' : type === 'remove' ? '-' : ' ';

    return (
        <div className={`${baseClass} ${typeClasses[type]}`}>
            <span className="select-none opacity-50 mr-2">{prefix}</span>
            {content || ' '}
        </div>
    );
};

const SimpleUnifiedDiff: React.FC<{ oldText: string; newText: string; maxLines?: number }> = ({
    oldText,
    newText,
    maxLines = 8
}) => {
    const oldLines = oldText.split('\n');
    const newLines = newText.split('\n');

    // Simple diff: show removed then added (not a real unified diff algorithm)
    const lines: { type: 'add' | 'remove'; content: string }[] = [];

    oldLines.forEach(line => {
        if (!newLines.includes(line)) {
            lines.push({ type: 'remove', content: line });
        }
    });

    newLines.forEach(line => {
        if (!oldLines.includes(line)) {
            lines.push({ type: 'add', content: line });
        }
    });

    // Truncate if too many lines
    const displayLines = lines.slice(0, maxLines);
    const hasMore = lines.length > maxLines;

    return (
        <div className="overflow-hidden">
            {displayLines.map((line, idx) => (
                <DiffLine key={idx} type={line.type} content={line.content} />
            ))}
            {hasMore && (
                <div className="text-[10px] text-zinc-500 px-2 py-1 bg-zinc-900/50">
                    +{lines.length - maxLines} more lines
                </div>
            )}
        </div>
    );
};

export const InlineDiffPreview: React.FC<InlineDiffPreviewProps> = ({
    change,
    onAccept,
    onReject,
    onExpand,
}) => {
    const [isCollapsed, setIsCollapsed] = useState(true);
    const filename = change.path.split('/').pop() || change.path;

    // Get preview text based on change type
    const getPreviewContent = () => {
        if (change.change_type === 'patch') {
            return (
                <SimpleUnifiedDiff
                    oldText={change.old_content}
                    newText={change.new_content}
                    maxLines={isCollapsed ? 4 : 12}
                />
            );
        }

        if (change.change_type === 'multi_patch') {
            const totalPatches = change.patches.length;
            const displayPatches = isCollapsed ? change.patches.slice(0, 1) : change.patches.slice(0, 3);

            return (
                <div className="space-y-1">
                    {displayPatches.map((hunk, idx) => (
                        <div key={idx} className="border-l-2 border-blue-500/50 bg-blue-900/10">
                            <div className="text-[10px] text-blue-400 px-2 py-0.5 bg-blue-900/20">
                                Hunk {idx + 1}/{totalPatches}
                                {hunk.start_line && ` (line ${hunk.start_line})`}
                            </div>
                            <SimpleUnifiedDiff
                                oldText={hunk.old_text}
                                newText={hunk.new_text}
                                maxLines={isCollapsed ? 2 : 4}
                            />
                        </div>
                    ))}
                    {!isCollapsed && change.patches.length > 3 && (
                        <div className="text-[10px] text-zinc-500 px-2 py-1">
                            +{change.patches.length - 3} more hunks
                        </div>
                    )}
                </div>
            );
        }

        if (change.change_type === 'new_file') {
            const preview = change.content.split('\n').slice(0, isCollapsed ? 3 : 8);
            return (
                <div className="bg-emerald-900/10 border-l-2 border-emerald-500">
                    {preview.map((line, idx) => (
                        <DiffLine key={idx} type="add" content={line} />
                    ))}
                    {change.content.split('\n').length > preview.length && (
                        <div className="text-[10px] text-zinc-500 px-2 py-1">
                            +{change.content.split('\n').length - preview.length} more lines
                        </div>
                    )}
                </div>
            );
        }

        if (change.change_type === 'delete_file') {
            return (
                <div className="bg-red-900/20 border-l-2 border-red-500 px-2 py-2 text-red-300 text-xs">
                    This file will be deleted
                </div>
            );
        }

        return null;
    };

    return (
        <div className="bg-zinc-900/95 border border-zinc-700/50 rounded-lg shadow-lg overflow-hidden max-w-md">
            {/* Compact Header */}
            <div className="flex items-center justify-between px-2 py-1.5 bg-zinc-800/80 border-b border-zinc-700/50">
                <div className="flex items-center gap-2 min-w-0 flex-1">
                    <button
                        onClick={() => setIsCollapsed(!isCollapsed)}
                        className="p-0.5 hover:bg-zinc-700/50 rounded transition-colors"
                    >
                        {isCollapsed ? (
                            <ChevronDown className="w-3.5 h-3.5 text-zinc-400" />
                        ) : (
                            <ChevronUp className="w-3.5 h-3.5 text-zinc-400" />
                        )}
                    </button>
                    <span className="text-xs font-mono text-zinc-300 truncate">{filename}</span>
                    {change.change_type === 'multi_patch' && (
                        <span className="text-[9px] bg-blue-900/50 text-blue-300 px-1 rounded shrink-0">
                            {change.patches.length} hunks
                        </span>
                    )}
                </div>

                {/* Actions */}
                <div className="flex items-center gap-1 shrink-0 ml-2">
                    {onExpand && (
                        <button
                            onClick={onExpand}
                            className="p-1 rounded hover:bg-zinc-700/50 text-zinc-400 transition-colors"
                            title="Expand"
                        >
                            <Maximize2 className="w-3 h-3" />
                        </button>
                    )}
                    <button
                        onClick={onAccept}
                        className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-emerald-600 hover:bg-emerald-500 text-white transition-colors"
                    >
                        <Check className="w-3 h-3" />
                        Accept
                    </button>
                    <button
                        onClick={onReject}
                        className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-zinc-700 hover:bg-red-600 text-zinc-300 hover:text-white transition-colors"
                    >
                        <X className="w-3 h-3" />
                    </button>
                </div>
            </div>

            {/* Diff Preview */}
            <div className="max-h-48 overflow-y-auto bg-[#1a1a1a]">
                {getPreviewContent()}
            </div>
        </div>
    );
};

export default InlineDiffPreview;
