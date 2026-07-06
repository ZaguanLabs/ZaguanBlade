import React from 'react';
import { Terminal } from 'lucide-react';

interface FloatingCommandApprovalPillProps {
    visible: boolean;
    onClick: () => void;
}

export const FloatingCommandApprovalPill: React.FC<FloatingCommandApprovalPillProps> = ({ visible, onClick }) => {
    if (!visible) {
        return null;
    }

    return (
        <div className="pointer-events-none absolute inset-x-0 bottom-12 z-10 flex justify-center px-4 pb-1">
            <button
                type="button"
                onClick={onClick}
                className="pointer-events-auto inline-flex items-center gap-2 rounded-full border border-(--accent-warning)/30 bg-[color-mix(in_srgb,var(--accent-warning)_12%,transparent)] px-3 py-2 text-[11px] font-medium text-(--accent-warning) shadow-lg backdrop-blur-sm transition-colors hover:bg-[color-mix(in_srgb,var(--accent-warning)_22%,transparent)]"
            >
                <Terminal className="h-3.5 w-3.5" aria-hidden="true" />
                Command awaiting your approval
            </button>
        </div>
    );
};
