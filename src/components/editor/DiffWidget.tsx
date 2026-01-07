import React from 'react';
import { Check, X, ArrowRight } from 'lucide-react';

interface DiffWidgetProps {
    original: string;
    modified: string;
    onAccept: () => void;
    onReject: () => void;
}

export const DiffWidget: React.FC<DiffWidgetProps> = ({ original, modified, onAccept, onReject }) => {
    return (
        <div className="my-1 rounded-md overflow-hidden border border-zinc-700 font-mono text-sm bg-[#1e1e1e] shadow-lg select-none">
            {/* Header / Controls */}
            <div className="flex items-center justify-between bg-zinc-800 px-2 py-1 border-b border-zinc-700">
                <div className="flex items-center gap-2 text-xs text-zinc-400">
                    <span className="font-semibold text-purple-400">AI Suggestion</span>
                </div>
                <div className="flex items-center gap-1">
                    <button
                        onClick={onAccept}
                        className="flex items-center gap-1 px-3 py-1 rounded bg-emerald-600 text-white hover:bg-emerald-500 transition-colors text-xs font-semibold shadow-lg"
                    >
                        <Check className="w-3 h-3" />
                        Accept
                    </button>
                    <button
                        onClick={onReject}
                        className="flex items-center gap-1 px-3 py-1 rounded bg-red-600 text-white hover:bg-red-500 transition-colors text-xs font-semibold shadow-lg"
                    >
                        <X className="w-3 h-3" />
                        Reject
                    </button>
                </div>
            </div>

            {/* Content (Simple Side-by-Side or Vertical) */}
            {/* For vertical diffs, we usually show Original then Arrow then Modified, 
                or just visual difference. Let's do a vertical stack for 2-way diff ease. */}

            <div className="flex flex-col">
                {/* Original (Red) */}
                <div className="bg-red-900/20 border-l-2 border-red-500/50 p-2 opacity-70 relative group">
                    <div className="absolute right-2 top-2 opacity-0 group-hover:opacity-50 text-[10px] text-red-400">ORIGINAL</div>
                    <pre className="m-0 whitespace-pre-wrap text-red-100/70 strike-through">{original}</pre>
                </div>

                {/* Modified (Green) */}
                <div className="bg-emerald-900/20 border-l-2 border-emerald-500/50 p-2 relative group">
                    <div className="absolute right-2 top-2 opacity-0 group-hover:opacity-50 text-[10px] text-emerald-400">MODIFIED</div>
                    <pre className="m-0 whitespace-pre-wrap text-emerald-100">{modified}</pre>
                </div>
            </div>
        </div>
    );
};
