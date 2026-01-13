'use client';
import React, { useState } from 'react';
import { Check, X, ChevronDown, ChevronRight, FileCode, Layers, GitBranch } from 'lucide-react';
import type { Change, PatchHunk } from '../../types/change';

interface PendingChangesBarProps {
    changes: Change[];
    onAccept: (changeId: string) => void;
    onReject: (changeId: string) => void;
    onAcceptAll: () => void;
    onRejectAll: () => void;
    onSelectChange?: (change: Change) => void;
}

interface ChangeItemProps {
    change: Change;
    isExpanded: boolean;
    isSelected: boolean;
    onToggle: () => void;
    onSelect: () => void;
    onAccept: () => void;
    onReject: () => void;
}

const getPatchCount = (change: Change): number => {
    if (change.change_type === 'multi_patch') return change.patches.length;
    if (change.change_type === 'patch') return 1;
    return 0;
};

const getChangeIcon = (change: Change) => {
    switch (change.change_type) {
        case 'multi_patch':
            return <Layers className="w-3.5 h-3.5 text-blue-400" />;
        case 'patch':
            return <GitBranch className="w-3.5 h-3.5 text-purple-400" />;
        case 'new_file':
            return <FileCode className="w-3.5 h-3.5 text-emerald-400" />;
        case 'delete_file':
            return <X className="w-3.5 h-3.5 text-red-400" />;
    }
};

const getChangeLabel = (change: Change): string => {
    switch (change.change_type) {
        case 'multi_patch':
            return `${change.patches.length} patches`;
        case 'patch':
            return 'Edit';
        case 'new_file':
            return 'Create';
        case 'delete_file':
            return 'Delete';
    }
};

const HunkPreview: React.FC<{ hunk: PatchHunk; index: number }> = ({ hunk, index }) => {
    const previewOld = hunk.old_text.split('\n').slice(0, 2).join('\n');
    const previewNew = hunk.new_text.split('\n').slice(0, 2).join('\n');

    return (
        <div className="border-l-2 border-zinc-700 ml-2 pl-2 py-1 text-[10px] font-mono">
            <div className="text-zinc-500 mb-0.5">Hunk {index + 1}</div>
            <div className="text-red-400/70 truncate">- {previewOld.split('\n')[0] || '(empty)'}</div>
            <div className="text-emerald-400/70 truncate">+ {previewNew.split('\n')[0] || '(empty)'}</div>
        </div>
    );
};

const ChangeItem: React.FC<ChangeItemProps> = ({
    change,
    isExpanded,
    isSelected,
    onToggle,
    onSelect,
    onAccept,
    onReject,
}) => {
    const filename = change.path.split('/').pop() || change.path;
    const patchCount = getPatchCount(change);

    return (
        <div className={`border-b border-zinc-800/50 ${isSelected ? 'bg-zinc-800/40' : ''}`}>
            {/* Change Header */}
            <div
                className="flex items-center gap-2 px-2 py-1.5 hover:bg-zinc-800/30 cursor-pointer transition-colors"
                onClick={onSelect}
            >
                {/* Expand/collapse for multi-patch */}
                {change.change_type === 'multi_patch' && (
                    <button onClick={(e) => { e.stopPropagation(); onToggle(); }} className="p-0.5">
                        {isExpanded ? (
                            <ChevronDown className="w-3 h-3 text-zinc-500" />
                        ) : (
                            <ChevronRight className="w-3 h-3 text-zinc-500" />
                        )}
                    </button>
                )}
                {change.change_type !== 'multi_patch' && <div className="w-4" />}

                {/* Icon */}
                {getChangeIcon(change)}

                {/* Filename */}
                <span className="text-xs text-zinc-300 truncate flex-1" title={change.path}>
                    {filename}
                </span>

                {/* Badge */}
                {patchCount > 1 && (
                    <span className="text-[9px] font-mono bg-blue-900/50 text-blue-300 px-1 rounded">
                        {patchCount}
                    </span>
                )}

                {/* Quick actions */}
                <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button
                        onClick={(e) => { e.stopPropagation(); onAccept(); }}
                        className="p-1 rounded hover:bg-emerald-900/40 text-emerald-400 transition-colors"
                        title="Accept"
                    >
                        <Check className="w-3 h-3" />
                    </button>
                    <button
                        onClick={(e) => { e.stopPropagation(); onReject(); }}
                        className="p-1 rounded hover:bg-red-900/40 text-red-400 transition-colors"
                        title="Reject"
                    >
                        <X className="w-3 h-3" />
                    </button>
                </div>
            </div>

            {/* Expanded hunks preview */}
            {isExpanded && change.change_type === 'multi_patch' && (
                <div className="pb-1 px-2">
                    {change.patches.slice(0, 3).map((hunk, idx) => (
                        <HunkPreview key={idx} hunk={hunk} index={idx} />
                    ))}
                    {change.patches.length > 3 && (
                        <div className="text-[10px] text-zinc-500 ml-4 py-0.5">
                            +{change.patches.length - 3} more hunks
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};

export const PendingChangesBar: React.FC<PendingChangesBarProps> = ({
    changes,
    onAccept,
    onReject,
    onAcceptAll,
    onRejectAll,
    onSelectChange,
}) => {
    const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [isMinimized, setIsMinimized] = useState(false);

    const toggleExpanded = (id: string) => {
        setExpandedIds(prev => {
            const next = new Set(prev);
            if (next.has(id)) {
                next.delete(id);
            } else {
                next.add(id);
            }
            return next;
        });
    };

    const handleSelect = (change: Change) => {
        setSelectedId(change.id);
        onSelectChange?.(change);
    };

    if (changes.length === 0) {
        return null;
    }

    // Group changes by file
    const fileGroups = new Map<string, Change[]>();
    changes.forEach(c => {
        const group = fileGroups.get(c.path) || [];
        group.push(c);
        fileGroups.set(c.path, group);
    });

    const totalPatches = changes.reduce((sum, c) => sum + getPatchCount(c), 0);

    return (
        <div className="flex flex-col bg-zinc-900/95 border-l border-zinc-800 w-64 max-h-full overflow-hidden shadow-xl">
            {/* Header */}
            <div
                className="flex items-center justify-between px-3 py-2 bg-zinc-900 border-b border-zinc-800 cursor-pointer"
                onClick={() => setIsMinimized(!isMinimized)}
            >
                <div className="flex items-center gap-2">
                    {isMinimized ? (
                        <ChevronRight className="w-3.5 h-3.5 text-zinc-500" />
                    ) : (
                        <ChevronDown className="w-3.5 h-3.5 text-zinc-500" />
                    )}
                    <span className="text-xs font-semibold text-zinc-300">Pending Changes</span>
                    <span className="text-[10px] font-mono bg-purple-900/50 text-purple-300 px-1.5 py-0.5 rounded">
                        {changes.length} file{changes.length !== 1 ? 's' : ''} · {totalPatches} edit{totalPatches !== 1 ? 's' : ''}
                    </span>
                </div>
            </div>

            {!isMinimized && (
                <>
                    {/* Batch Actions */}
                    <div className="flex items-center justify-end gap-1 px-2 py-1.5 border-b border-zinc-800/50 bg-zinc-900/60">
                        <button
                            onClick={onAcceptAll}
                            className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-emerald-600/80 hover:bg-emerald-500 text-white transition-colors"
                        >
                            <Check className="w-3 h-3" />
                            Accept All
                        </button>
                        <button
                            onClick={onRejectAll}
                            className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-red-600/80 hover:bg-red-500 text-white transition-colors"
                        >
                            <X className="w-3 h-3" />
                            Reject All
                        </button>
                    </div>

                    {/* Change List */}
                    <div className="flex-1 overflow-y-auto">
                        {changes.map(change => (
                            <ChangeItem
                                key={change.id}
                                change={change}
                                isExpanded={expandedIds.has(change.id)}
                                isSelected={selectedId === change.id}
                                onToggle={() => toggleExpanded(change.id)}
                                onSelect={() => handleSelect(change)}
                                onAccept={() => onAccept(change.id)}
                                onReject={() => onReject(change.id)}
                            />
                        ))}
                    </div>

                    {/* Footer */}
                    <div className="px-2 py-1.5 bg-zinc-900/80 border-t border-zinc-800/50 text-[9px] text-zinc-500 text-center">
                        Click a change to preview · All edits are atomic
                    </div>
                </>
            )}
        </div>
    );
};

export default PendingChangesBar;
