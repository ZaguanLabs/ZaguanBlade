import React from 'react';
import { useTranslations } from 'next-intl';
import { Check, X, FilePlus, Trash2 } from 'lucide-react';
import type { Change } from '../../types/change';

interface EditorDiffOverlayProps {
    change: Change;
    onAccept: () => void;
    onReject: () => void;
}

export const EditorDiffOverlay: React.FC<EditorDiffOverlayProps> = ({ 
    change,
    onAccept, 
    onReject
}) => {
    const t = useTranslations();
    const filename = change.path.split('/').pop() || change.path;
    
    return (
        <div className="absolute top-4 left-4 right-4 max-h-[60vh] bg-[#1e1e1e] border border-purple-500/50 rounded-lg shadow-2xl z-50 flex flex-col overflow-hidden">
            {/* Compact header */}
            <div className="flex items-center justify-between bg-purple-900/20 px-3 py-2 border-b border-purple-500/30">
                <div className="flex items-center gap-2">
                    {change.change_type === 'new_file' ? (
                        <>
                            <FilePlus className="w-3.5 h-3.5 text-emerald-400" />
                            <span className="text-[10px] font-semibold text-emerald-400 uppercase tracking-wide">Create New File</span>
                        </>
                    ) : change.change_type === 'delete_file' ? (
                        <>
                            <Trash2 className="w-3.5 h-3.5 text-red-400" />
                            <span className="text-[10px] font-semibold text-red-400 uppercase tracking-wide">Delete File</span>
                        </>
                    ) : (
                        <span className="text-[10px] font-semibold text-purple-400 uppercase tracking-wide">{t('diff.aiSuggestion')}</span>
                    )}
                    <span className="text-xs text-zinc-400 font-mono">{filename}</span>
                </div>
                <div className="flex items-center gap-1.5">
                    <button
                        onClick={onAccept}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-emerald-600/90 hover:bg-emerald-500 text-white transition-colors"
                    >
                        <Check className="w-3.5 h-3.5" />
                        {change.change_type === 'new_file' ? 'Create' : change.change_type === 'delete_file' ? 'Delete' : t('diff.accept')}
                    </button>
                    <button
                        onClick={onReject}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium bg-red-600/90 hover:bg-red-500 text-white transition-colors"
                    >
                        <X className="w-3.5 h-3.5" />
                        {t('diff.reject')}
                    </button>
                </div>
            </div>

            {/* Content preview */}
            <div className="flex-1 overflow-auto">
                {change.change_type === 'new_file' ? (
                    /* New file: show content preview only */
                    <div className="bg-emerald-900/10 border-l-2 border-emerald-500/50">
                        <div className="px-3 py-1 bg-emerald-900/20">
                            <span className="text-[10px] font-semibold text-emerald-400 uppercase tracking-wide">File Content</span>
                        </div>
                        <pre className="px-3 py-2 font-mono text-xs text-emerald-200 whitespace-pre-wrap leading-relaxed">{change.content}</pre>
                    </div>
                ) : change.change_type === 'delete_file' ? (
                    /* Delete file: show confirmation message */
                    <div className="bg-red-900/10 border-l-2 border-red-500/50">
                        <div className="px-3 py-1 bg-red-900/20">
                            <span className="text-[10px] font-semibold text-red-400 uppercase tracking-wide">Confirm Deletion</span>
                        </div>
                        <div className="px-3 py-2 text-sm text-red-200">
                            <p>This will permanently delete the file:</p>
                            <p className="font-mono mt-2 text-red-300">{change.path}</p>
                        </div>
                    </div>
                ) : (
                    /* Existing file: show diff */
                    <div className="flex flex-col">
                        {/* Original (removed) */}
                        <div className="bg-red-900/10 border-l-2 border-red-500/50">
                            <div className="px-3 py-1 bg-red-900/20">
                                <span className="text-[10px] font-semibold text-red-400 uppercase tracking-wide">- {t('approval.remove')}</span>
                            </div>
                            <pre className="px-3 py-2 font-mono text-xs text-red-200/80 whitespace-pre-wrap leading-relaxed">{change.old_content}</pre>
                        </div>

                        {/* Modified (added) */}
                        <div className="bg-emerald-900/10 border-l-2 border-emerald-500/50">
                            <div className="px-3 py-1 bg-emerald-900/20">
                                <span className="text-[10px] font-semibold text-emerald-400 uppercase tracking-wide">+ {t('approval.add')}</span>
                            </div>
                            <pre className="px-3 py-2 font-mono text-xs text-emerald-200 whitespace-pre-wrap leading-relaxed">{change.new_content}</pre>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
};
