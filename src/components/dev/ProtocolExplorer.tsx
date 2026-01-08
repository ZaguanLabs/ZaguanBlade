'use client';
import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
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

        let unlisten: (() => void) | undefined;

        const setup = async () => {
            // Listen to the hardened 'blade-event' channel
            unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
                if (isPaused) return;

                const envelope = event.payload;
                const entry: LogEntry = {
                    id: envelope.id,
                    timestamp: envelope.timestamp,
                    causality_id: envelope.causality_id,
                    event: envelope.event,
                    receivedAt: Date.now()
                };

                setLogs(prev => [...prev.slice(-99), entry]); // Keep last 100
            });
        };

        setup();
        return () => { if (unlisten) unlisten(); };
    }, [isPaused]);

    if (!IS_DEV) return null;

    if (!isOpen) {
        return (
            <button
                onClick={() => setIsOpen(true)}
                className="fixed bottom-4 right-4 z-50 p-2 bg-zinc-900 border border-zinc-700 rounded-full shadow-lg hover:bg-zinc-800 transition-all text-xs text-zinc-400 flex items-center gap-2 group"
                title="Open Protocol Explorer"
            >
                <Activity className="w-4 h-4 text-emerald-500 group-hover:text-emerald-400" />
            </button>
        );
    }

    return (
        <div className="fixed bottom-4 right-4 z-50 w-[400px] h-[500px] bg-[#09090b] border border-zinc-800 rounded-lg shadow-2xl flex flex-col font-mono text-[10px] opacity-95">
            {/* Header */}
            <div className="flex items-center justify-between px-3 py-2 border-b border-zinc-800 bg-zinc-900/50">
                <div className="flex items-center gap-2 text-zinc-300 font-semibold uppercase tracking-wider">
                    <Activity className="w-3 h-3 text-emerald-500" />
                    Protocol Explorer
                </div>
                <div className="flex items-center gap-1">
                    <button
                        onClick={() => setIsPaused(!isPaused)}
                        className={`p-1 rounded hover:bg-zinc-800 ${isPaused ? 'text-yellow-500' : 'text-zinc-400'}`}
                        title={isPaused ? "Resume" : "Pause"}
                    >
                        {isPaused ? <Play className="w-3 h-3" /> : <Pause className="w-3 h-3" />}
                    </button>
                    <button
                        onClick={() => setLogs([])}
                        className="p-1 rounded hover:bg-zinc-800 text-zinc-400 hover:text-red-400"
                        title="Clear Logs"
                    >
                        <Trash2 className="w-3 h-3" />
                    </button>
                    <div className="w-px h-3 bg-zinc-800 mx-1" />
                    <button
                        onClick={() => setIsOpen(false)}
                        className="p-1 rounded hover:bg-zinc-800 text-zinc-400 hover:text-white"
                    >
                        <X className="w-3 h-3" />
                    </button>
                </div>
            </div>

            {/* Log Stream */}
            <div className="flex-1 overflow-y-auto p-2 space-y-2 scrollbar-thin scrollbar-thumb-zinc-800">
                {logs.length === 0 && (
                    <div className="text-center text-zinc-600 mt-20 italic">
                        Waiting for events...
                    </div>
                )}
                {logs.map((log) => (
                    <div key={log.id} className="relative group border border-zinc-800/50 rounded bg-zinc-900/20 p-2 hover:bg-zinc-900/40 transition-colors">
                        {/* Meta Line */}
                        <div className="flex items-center gap-2 text-zinc-500 mb-1">
                            <Clock className="w-3 h-3" />
                            <span>{new Date(log.receivedAt).toLocaleTimeString().split(' ')[0]}.{String(new Date(log.receivedAt).getMilliseconds()).padStart(3, '0')}</span>
                            <span className="text-zinc-700">|</span>
                            <span className="font-mono text-zinc-600" title={`Event ID: ${log.id}`}>{log.id.slice(0, 8)}...</span>
                            {log.causality_id && (
                                <>
                                    <ArrowRight className="w-3 h-3 text-zinc-700" />
                                    <span className="bg-zinc-800/50 px-1 rounded text-zinc-400" title={`Caused by Intent: ${log.causality_id}`}>{log.causality_id.slice(0, 8)}...</span>
                                </>
                            )}
                        </div>

                        {/* Content Line */}
                        <div className="flex items-start gap-2">

                            <div className="flex-1">
                                <span className={`font-semibold ${getEventColor(log.event.type)}`}>
                                    {log.event.type}
                                </span>
                                <span className="text-zinc-500 mx-1">::</span>
                                <span className="text-zinc-300">
                                    {/* Try to extract inner variant name */}
                                    {Object.keys(log.event.payload || {})[0] || 'Unknown'}
                                </span>
                            </div>
                        </div>

                        {/* Details (JSON) */}
                        <pre className="mt-2 text-zinc-500 overflow-x-auto p-2 bg-black/20 rounded hidden group-hover:block select-text">
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
        case 'Chat': return 'text-purple-400';
        case 'Editor': return 'text-blue-400';
        case 'File': return 'text-yellow-400';
        case 'Workflow': return 'text-orange-400';
        case 'Terminal': return 'text-pink-400';
        case 'System': return 'text-red-400';
        default: return 'text-zinc-400';
    }
}
