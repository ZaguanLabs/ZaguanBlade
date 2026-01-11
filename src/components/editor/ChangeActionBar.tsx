'use client';
import React from 'react';
import { Check, X, ChevronUp, ChevronDown } from 'lucide-react';

interface ChangeActionBarProps {
    currentFileIndex: number;
    totalFiles: number;
    currentHunkIndex?: number;
    totalHunks?: number;
    onAccept: () => void;
    onReject: () => void;
    onNextFile?: () => void;
    onPrevFile?: () => void;
    onNextHunk?: () => void;
    onPrevHunk?: () => void;
    filename?: string;
}

/**
 * Non-invasive bottom action bar for accepting/rejecting code changes.
 * Inspired by Cursor/Windsurf/Antigravity inline diff UI.
 * 
 * Positioned at the bottom of the editor, doesn't block code view.
 */
export const ChangeActionBar: React.FC<ChangeActionBarProps> = ({
    currentFileIndex,
    totalFiles,
    currentHunkIndex,
    totalHunks,
    onAccept,
    onReject,
    onNextFile,
    onPrevFile,
    onNextHunk,
    onPrevHunk,
    filename,
}) => {
    // Keyboard shortcuts
    React.useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ctrl+Enter = Accept
            if (e.ctrlKey && e.key === 'Enter') {
                e.preventDefault();
                onAccept();
            }
            // Ctrl+Backspace = Reject
            if (e.ctrlKey && e.key === 'Backspace') {
                e.preventDefault();
                onReject();
            }
            // Alt+K = Previous hunk
            if (e.altKey && e.key === 'k') {
                e.preventDefault();
                onPrevHunk?.();
            }
            // Alt+J = Next hunk
            if (e.altKey && e.key === 'j') {
                e.preventDefault();
                onNextHunk?.();
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [onAccept, onReject, onNextHunk, onPrevHunk]);

    return (
        <div className="fixed bottom-8 left-1/2 transform -translate-x-1/2 z-50">
            <div className="flex items-center gap-1 bg-zinc-900/95 border border-zinc-700/60 rounded-lg shadow-2xl px-2 py-1.5 backdrop-blur-sm">
                {/* Accept Button */}
                <button
                    onClick={onAccept}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-emerald-600 hover:bg-emerald-500 text-white transition-colors"
                    title="Accept Changes (Ctrl+Enter)"
                >
                    <Check className="w-3.5 h-3.5" />
                    <span>Accept Changes</span>
                    <kbd className="ml-1 px-1 py-0.5 text-[9px] bg-emerald-700/50 rounded">Ctrl+↵</kbd>
                </button>

                {/* Reject Button */}
                <button
                    onClick={onReject}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-zinc-700 hover:bg-red-600 text-zinc-300 hover:text-white transition-colors"
                    title="Reject (Ctrl+Backspace)"
                >
                    <span>Reject</span>
                    <kbd className="ml-1 px-1 py-0.5 text-[9px] bg-zinc-600/50 rounded">Ctrl+⌫</kbd>
                </button>

                {/* Separator */}
                <div className="w-px h-5 bg-zinc-700 mx-1" />

                {/* Hunk Navigation (if multi-hunk) */}
                {totalHunks && totalHunks > 1 && (
                    <>
                        <button
                            onClick={onPrevHunk}
                            className="p-1.5 rounded hover:bg-zinc-700/50 text-zinc-400 hover:text-zinc-200 transition-colors"
                            title="Previous hunk (Alt+K)"
                        >
                            <ChevronUp className="w-3.5 h-3.5" />
                        </button>
                        <span className="text-[10px] text-zinc-500 font-mono px-1">
                            <kbd className="text-zinc-600">Alt+K</kbd>
                        </span>
                        <button
                            onClick={onNextHunk}
                            className="p-1.5 rounded hover:bg-zinc-700/50 text-zinc-400 hover:text-zinc-200 transition-colors"
                            title="Next hunk (Alt+J)"
                        >
                            <ChevronDown className="w-3.5 h-3.5" />
                        </button>
                        <span className="text-[10px] text-zinc-500 font-mono px-1">
                            <kbd className="text-zinc-600">Alt+J</kbd>
                        </span>

                        <div className="w-px h-5 bg-zinc-700 mx-1" />
                    </>
                )}

                {/* File Navigation */}
                <button
                    onClick={onPrevFile}
                    disabled={currentFileIndex <= 1}
                    className="p-1.5 rounded hover:bg-zinc-700/50 text-zinc-400 hover:text-zinc-200 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                    title="Previous file"
                >
                    <ChevronUp className="w-3.5 h-3.5" />
                </button>
                <span className="text-xs text-zinc-400 font-mono min-w-[80px] text-center">
                    Edited files <span className="text-zinc-200">{currentFileIndex}/{totalFiles}</span>
                </span>
                <button
                    onClick={onNextFile}
                    disabled={currentFileIndex >= totalFiles}
                    className="p-1.5 rounded hover:bg-zinc-700/50 text-zinc-400 hover:text-zinc-200 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                    title="Next file"
                >
                    <ChevronDown className="w-3.5 h-3.5" />
                </button>
            </div>
        </div>
    );
};

export default ChangeActionBar;
