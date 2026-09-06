import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Terminal, Clock, CheckCircle2, XCircle, Loader2, ChevronRight } from 'lucide-react';
import type { ToolCall } from '../types/chat';
import { getCommandSessionAction, parseCommandSessionResult } from '../utils/commandSession';
import { stripAllAnsi } from './CommandOutputDisplay';
import { useSmoothWheelScroll } from '../hooks/useSmoothWheelScroll';

interface CommandSessionDisplayProps {
    id: string;
    args: Record<string, unknown>;
    status: NonNullable<ToolCall['status']>;
    result?: string;
}

export const CommandSessionDisplay: React.FC<CommandSessionDisplayProps> = ({ id, args, status, result }) => {
    const { t } = useTranslation();
    const parsed = useMemo(() => parseCommandSessionResult(result), [result]);
    const output = useMemo(() => stripAllAnsi(parsed.output), [parsed.output]);
    const onOutputWheel = useSmoothWheelScroll<HTMLPreElement>();
    const action = getCommandSessionAction(args);
    const sessionId = typeof args.session_id === 'string' ? args.session_id : parsed.sessionId;
    const failed = status === 'error' || (status === 'complete' && parsed.exitCode !== undefined && parsed.exitCode !== 0);
    const busy = status === 'executing';
    const running = status === 'complete' && parsed.state === 'running';
    const tone = failed ? 'var(--state-danger)'
        : busy || running ? 'var(--accent-ai)'
            : status === 'complete' ? 'var(--accent-mention)' : 'var(--accent-planning)';
    const statusText = failed ? t('toolCall.status.failed')
        : running ? t('toolCall.commandSession.running')
            : status === 'complete' && parsed.state === 'exited' ? t('toolCall.commandSession.exited')
                : t(`toolCall.status.${status}`);
    const Icon = failed ? XCircle : busy ? Loader2 : running ? Clock : status === 'complete' ? CheckCircle2 : Terminal;

    return (
        <div data-tool-call-id={id} className="border-l-2 pl-2.5 text-[11px]" style={{ borderLeftColor: tone }}>
            <div className="flex flex-wrap items-center gap-2 py-1">
                <Icon aria-hidden="true" className={`h-3.5 w-3.5 shrink-0 ${busy ? 'animate-spin' : ''}`} style={{ color: tone }} />
                <span className="font-medium text-(--fg-primary)">{t(`toolCall.commandSession.${action}`)}</span>
                <span className="ml-auto text-[9px] font-semibold uppercase tracking-[0.14em]" style={{ color: tone }}>{statusText}</span>
            </div>
            <div className="ml-5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-(--fg-tertiary)">
                {sessionId && <span className="min-w-0 max-w-full truncate font-mono" title={sessionId}>{t('toolCall.commandSession.session', { id: sessionId })}</span>}
                {parsed.elapsedSeconds !== undefined && <span>{t('toolCall.commandSession.elapsed', { seconds: parsed.elapsedSeconds })}</span>}
                {parsed.exitCode !== undefined && <span style={{ color: tone }}>{t('chat.commandOutput.exit', { code: parsed.exitCode })}</span>}
            </div>
            {output ? (
                <details className="group/session ml-5 mt-1" open={failed}>
                    <summary className="flex cursor-pointer list-none items-center gap-1 py-1 text-[10px] text-(--fg-secondary)">
                        <ChevronRight aria-hidden="true" className="h-3 w-3 transition-transform group-open/session:rotate-90" />
                        {t(failed ? 'toolCall.details.error' : 'toolCall.commandSession.output')}
                    </summary>
                    <pre onWheel={onOutputWheel} className="max-h-64 overflow-auto whitespace-pre-wrap wrap-break-word rounded bg-(--bg-surface)/60 p-2 font-mono text-[11px] leading-4 text-(--fg-primary) select-text">{output}</pre>
                </details>
            ) : status === 'complete' && parsed.state !== 'unknown' ? (
                <div className="ml-5 py-1 text-[10px] italic text-(--fg-tertiary)">{t('toolCall.commandSession.noOutput')}</div>
            ) : null}
        </div>
    );
};
