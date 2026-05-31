import React from 'react';
import { ArrowDown } from 'lucide-react';

export const FloatingJumpToBottomButton: React.FC<{ visible: boolean; onClick: () => void }> = ({ visible, onClick }) => {
    if (!visible) {
        return null;
    }

    return (
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex justify-center px-4 pb-4">
            <button type="button" onClick={onClick} className="pointer-events-auto inline-flex items-center gap-2 rounded-full border border-(--border-subtle) bg-(--bg-surface)/95 px-3 py-2 text-[11px] text-(--fg-secondary)">
                <ArrowDown className="h-3.5 w-3.5" />
                Latest
            </button>
        </div>
    );
};
