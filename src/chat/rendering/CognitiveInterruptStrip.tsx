import React from 'react';
import { BrainCircuit, CheckCircle2, PauseCircle } from 'lucide-react';
import type { CognitiveInterruptState } from '../../types/chat';
import { StatusStripFrame } from './StatusStripFrame';

interface CognitiveInterruptStripProps {
    interrupt: CognitiveInterruptState | null;
}

function titleForInterrupt(interrupt: CognitiveInterruptState): string {
    if (interrupt.state === 'cleared') {
        return 'Diagnostic evidence gathered';
    }
    if (interrupt.state === 'blocked') {
        return 'Edit paused until diagnostic evidence is gathered';
    }
    return 'Cognitive Interrupt';
}

function detailForInterrupt(interrupt: CognitiveInterruptState): string {
    if (interrupt.state === 'active') {
        return interrupt.summary || 'Reassessing after repeated failure';
    }
    if (interrupt.state === 'blocked' && interrupt.tool_name) {
        return `${interrupt.tool_name} paused until diagnostic evidence is gathered`;
    }
    return interrupt.summary;
}

export const CognitiveInterruptStrip: React.FC<CognitiveInterruptStripProps> = React.memo(({ interrupt }) => {
    if (!interrupt) {
        return null;
    }

    const Icon = interrupt.state === 'cleared'
        ? CheckCircle2
        : interrupt.state === 'blocked'
            ? PauseCircle
            : BrainCircuit;

    return (
        <StatusStripFrame label="Status" tone="ai">
            <div className="flex min-w-0 items-center gap-2">
                <Icon aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-(--accent-ai)" />
                <div className="min-w-0">
                    <div className="truncate text-[11px] font-medium text-(--fg-secondary)">
                        {titleForInterrupt(interrupt)}
                    </div>
                    {detailForInterrupt(interrupt) && (
                        <div className="truncate text-[10px] text-(--fg-tertiary)">
                            {detailForInterrupt(interrupt)}
                        </div>
                    )}
                </div>
            </div>
        </StatusStripFrame>
    );
});

CognitiveInterruptStrip.displayName = 'CognitiveInterruptStrip';
