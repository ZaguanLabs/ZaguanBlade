'use client';
import React from 'react';
import { Check, X } from 'lucide-react';

interface FloatingDiffActionsProps {
    onAccept: () => void;
    onReject: () => void;
    style?: React.CSSProperties;
}

/**
 * Small floating Accept/Reject buttons that appear near the changed code section.
 * Positioned near the top-right of the diff block, similar to Cursor/Windsurf.
 */
export const FloatingDiffActions: React.FC<FloatingDiffActionsProps> = ({
    onAccept,
    onReject,
    style,
}) => {
    return (
        <div
            className="absolute z-40 flex items-center gap-0.5"
            style={style}
        >
            <button
                onClick={onAccept}
                className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-emerald-600/90 hover:bg-emerald-500 text-white shadow-lg transition-colors"
                title="Accept (Alt+Enter)"
            >
                Accept
                <kbd className="text-[8px] opacity-70">Alt+↵</kbd>
            </button>
            <button
                onClick={onReject}
                className="flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium bg-zinc-700/90 hover:bg-red-600 text-zinc-300 hover:text-white shadow-lg transition-colors"
                title="Reject (Shift+Alt+Backspace)"
            >
                Reject
                <kbd className="text-[8px] opacity-70">Shift+Alt+⌫</kbd>
            </button>
        </div>
    );
};

export default FloatingDiffActions;
