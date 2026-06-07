'use client';
import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { subscribeBladeEvents } from '../../services/bladeEvents';
import { X, Play, Pause, Trash2, Activity, ArrowRight, Clock, Box } from 'lucide-react';
import { BladeEvent, BladeEventEnvelope } from '../../types/blade';

// Should be stripped in production or hidden
const IS_DEV = process.env.NODE_ENV === 'development';

interface LogEntry {
    id: string; // Event ID
    timestamp: number;
    causality_id?: string;
    event: BladeEvent;
    receivedAt: number;
}

export const ProtocolExplorer: React.FC = () => {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<LogEntry[]>([]);
    const [isOpen, setIsOpen] = useState(false);
    const [isPaused, setIsPaused] = useState(false);
    const logsEndRef = useRef<HTMLDivElement>(null);

    // Auto-scroll logic
    useEffect(() => {
        if (!isPaused && logsEndRef.current) {
            logsEndRef.current.scrollIntoView({ behavior: 'smooth' });
        }
    }, [logs, isPaused]);

    useEffect(() => {
        if (!IS_DEV) return;

        const unsubscribe = subscribeBladeEvents((envelope) => {
            if (isPaused) return;

            const entry: LogEntry = {
                id: envelope.id,
                timestamp: envelope.timestamp,
                causality_id: envelope.causality_id || undefined,
                event: envelope.event,
                receivedAt: Date.now()
            };

            setLogs(prev => [...prev.slice(-99), entry]);
        });

        return () => { unsubscribe(); };
    }, [isPaused]);

    if (!IS_DEV) return null;

    if (!isOpen) {
        return (
            <button
                type="button"
                onClick={() => setIsOpen(true)}
                className="fixed bottom-4 right-4 z-50 p-2 bg-(--bg-surface) border border-(--border-subtle) rounded-full shadow-(--shadow-lg) hover:bg-(--bg-surface-hover) transition-all text-xs text-(--fg-tertiary) flex items-center gap-2 group"
                title={t('protocolExplorer.open')}
                aria-label={t('protocolExplorer.open')}
            >
                <Activity className="w-4 h-4 text-(--accent-mention) group-hover:text-(--accent-ai)" />
            </button>
        );
    }

    return (
        <div className="fixed bottom-4 right-4 z-50 w-[400px] h-[500px] bg-(--bg-panel) border border-(--border-subtle) rounded-(--panel-radius) shadow-(--shadow-xl) flex flex-col font-mono text-[10px] opacity-95">
            {/* Header */}
            <div className="flex items-center justify-between px-3 py-2 border-b border-(--border-subtle) bg-(--bg-surface)/50">
                <div className="flex items-center gap-2 text-(--fg-secondary) font-semibold uppercase tracking-wider">
                    <Activity className="w-3 h-3 text-(--accent-mention)" />
                    {t('protocolExplorer.title')}
                </div>
                <div className="flex items-center gap-1">
                    <button
                        type="button"
                        onClick={() => setIsPaused(!isPaused)}
                        className={`p-1 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) ${isPaused ? 'text-(--accent-warning)' : 'text-(--fg-tertiary)'}`}
                        title={isPaused ? t('protocolExplorer.resume') : t('protocolExplorer.pause')}
                        aria-label={isPaused ? t('protocolExplorer.resume') : t('protocolExplorer.pause')}
                    >
                        {isPaused ? <Play className="w-3 h-3" /> : <Pause className="w-3 h-3" />}
                    </button>
                    <button
                        type="button"
                        onClick={() => setLogs([])}
                        className="p-1 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) text-(--fg-tertiary) hover:text-(--state-danger)"
                        title={t('protocolExplorer.clearLogs')}
                        aria-label={t('protocolExplorer.clearLogs')}
                    >
                        <Trash2 className="w-3 h-3" />
                    </button>
                    <div className="w-px h-3 bg-(--border-subtle) mx-1" />
                    <button
                        type="button"
                        onClick={() => setIsOpen(false)}
                        aria-label={t('common.close')}
                        className="p-1 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) text-(--fg-tertiary) hover:text-(--fg-primary)"
                    >
                        <X className="w-3 h-3" />
                    </button>
                </div>
            </div>

            {/* Log Stream */}
            <div className="flex-1 overflow-y-auto p-2 space-y-2 scrollbar-thin scrollbar-thumb-(--bg-surface-hover)">
                {logs.length === 0 && (
                    <div className="text-center text-(--fg-tertiary) mt-20 italic">
                        {t('protocolExplorer.waitingForEvents')}
                    </div>
                )}
                {logs.map((log) => (
                    <div key={log.id} className="relative group border border-(--border-subtle) rounded-[calc(var(--panel-radius)*0.35)] bg-(--bg-surface)/20 p-2 hover:bg-(--bg-surface)/40 transition-colors">
                        {/* Meta Line */}
                        <div className="flex items-center gap-2 text-(--fg-tertiary) mb-1">
                            <Clock className="w-3 h-3" />
                            <span>{new Date(log.receivedAt).toLocaleTimeString().split(' ')[0]}.{String(new Date(log.receivedAt).getMilliseconds()).padStart(3, '0')}</span>
                            <span className="text-(--fg-tertiary)">|</span>
                            <span className="font-mono text-(--fg-tertiary)" title={t('protocolExplorer.eventId', { id: log.id })}>{log.id.slice(0, 8)}...</span>
                            {log.causality_id && (
                                <>
                                    <ArrowRight className="w-3 h-3 text-(--fg-tertiary)" />
                                    <span className="bg-(--bg-surface-hover)/50 px-1 rounded-[calc(var(--panel-radius)*0.25)] text-(--fg-tertiary)" title={t('protocolExplorer.causedByIntent', { id: log.causality_id })}>{log.causality_id.slice(0, 8)}...</span>
                                </>
                            )}
                        </div>

                        {/* Content Line */}
                        <div className="flex items-start gap-2">

                            <div className="flex-1">
                                <span className={`font-semibold ${getEventColor(log.event.type)}`}>
                                    {log.event.type}
                                </span>
                                <span className="text-(--fg-tertiary) mx-1">::</span>
                                <span className="text-(--fg-secondary)">
                                    {/* Try to extract inner variant name */}
                                    {Object.keys(log.event.payload || {})[0] || t('common.unknown')}
                                </span>
                            </div>
                        </div>

                        {/* Details (JSON) */}
                        <pre className="mt-2 text-(--fg-tertiary) overflow-x-auto p-2 bg-(--bg-app)/40 rounded-[calc(var(--panel-radius)*0.35)] hidden group-hover:block select-text">
                            {JSON.stringify(log.event.payload, null, 2)}
                        </pre>
                    </div>
                ))}
                <div ref={logsEndRef} />
            </div>
        </div>
    );
};

function getEventColor(type: string): string {
    switch (type) {
        case 'Chat': return 'text-(--accent-planning)';
        case 'Editor': return 'text-(--accent-ai)';
        case 'File': return 'text-(--accent-warning)';
        case 'Workflow': return 'text-(--accent-mention)';
        case 'Terminal': return 'text-(--accent-planning)';
        case 'System': return 'text-(--state-danger)';
        default: return 'text-(--fg-tertiary)';
    }
}
