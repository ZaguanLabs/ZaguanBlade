'use client';
import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { HardDrive, Cloud, Shield, Zap, ArrowRight, Loader2 } from 'lucide-react';

interface StorageSetupModalProps {
    isOpen: boolean;
    workspacePath: string;
    onComplete: () => void;
}

type StorageMode = 'local' | 'server';

export const StorageSetupModal: React.FC<StorageSetupModalProps> = ({
    isOpen,
    workspacePath,
    onComplete,
}) => {
    const [selectedMode, setSelectedMode] = useState<StorageMode>('local');
    const [isSettingUp, setIsSettingUp] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const handleSetup = async () => {
        setIsSettingUp(true);
        setError(null);

        try {
            // Initialize .zblade directory
            await invoke('init_zblade_directory', { projectPath: workspacePath });

            // Save initial settings with chosen storage mode
            const settings = {
                storage: {
                    mode: selectedMode,
                    sync_metadata: true,
                    cache: {
                        enabled: true,
                        max_size_mb: 100,
                    },
                },
                context: {
                    max_tokens: 8000,
                    compression: {
                        enabled: true,
                        model: 'remote',
                    },
                },
                privacy: {
                    telemetry: false,
                },
            };

            await invoke('save_project_settings', {
                projectPath: workspacePath,
                settings,
            });

            console.log('[StorageSetup] Initialized with mode:', selectedMode);
            onComplete();
        } catch (e) {
            console.error('[StorageSetup] Failed:', e);
            setError(String(e));
        } finally {
            setIsSettingUp(false);
        }
    };

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-[9999] flex items-center justify-center">
            {/* Backdrop */}
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" />

            {/* Modal */}
            <div className="relative bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-xl shadow-2xl w-full max-w-2xl mx-4 animate-in fade-in zoom-in-95 duration-200">
                {/* Header */}
                <div className="px-8 pt-8 pb-4">
                    <h2 className="text-2xl font-bold text-[var(--fg-primary)]">
                        Welcome to ZaguanBlade
                    </h2>
                    <p className="mt-2 text-[var(--fg-secondary)]">
                        Choose how you want to store your conversation history for this project.
                    </p>
                </div>

                {/* Options */}
                <div className="px-8 py-4 space-y-4">
                    {/* Local Storage Option */}
                    <button
                        onClick={() => setSelectedMode('local')}
                        className={`w-full p-5 rounded-lg border-2 text-left transition-all ${
                            selectedMode === 'local'
                                ? 'border-emerald-500 bg-emerald-500/10'
                                : 'border-[var(--border-subtle)] hover:border-[var(--border-focus)] hover:bg-[var(--bg-surface-hover)]'
                        }`}
                    >
                        <div className="flex items-start gap-4">
                            <div className={`p-3 rounded-lg ${
                                selectedMode === 'local' ? 'bg-emerald-500/20' : 'bg-[var(--bg-surface-hover)]'
                            }`}>
                                <HardDrive className={`w-6 h-6 ${
                                    selectedMode === 'local' ? 'text-emerald-400' : 'text-[var(--fg-tertiary)]'
                                }`} />
                            </div>
                            <div className="flex-1">
                                <div className="flex items-center gap-2">
                                    <h3 className="font-semibold text-[var(--fg-primary)]">Local Storage</h3>
                                    <span className="px-2 py-0.5 text-xs font-medium bg-emerald-500/20 text-emerald-400 rounded">
                                        Recommended
                                    </span>
                                </div>
                                <p className="mt-1 text-sm text-[var(--fg-secondary)]">
                                    Your code and conversations stay on your machine. Maximum privacy.
                                </p>
                                <div className="mt-3 flex items-center gap-4 text-xs text-[var(--fg-tertiary)]">
                                    <span className="flex items-center gap-1">
                                        <Shield className="w-3.5 h-3.5" /> Code never leaves your machine
                                    </span>
                                    <span className="flex items-center gap-1">
                                        <HardDrive className="w-3.5 h-3.5" /> Stored in .zblade/
                                    </span>
                                </div>
                            </div>
                        </div>
                    </button>

                    {/* Server Storage Option */}
                    <button
                        onClick={() => setSelectedMode('server')}
                        className={`w-full p-5 rounded-lg border-2 text-left transition-all ${
                            selectedMode === 'server'
                                ? 'border-blue-500 bg-blue-500/10'
                                : 'border-[var(--border-subtle)] hover:border-[var(--border-focus)] hover:bg-[var(--bg-surface-hover)]'
                        }`}
                    >
                        <div className="flex items-start gap-4">
                            <div className={`p-3 rounded-lg ${
                                selectedMode === 'server' ? 'bg-blue-500/20' : 'bg-[var(--bg-surface-hover)]'
                            }`}>
                                <Cloud className={`w-6 h-6 ${
                                    selectedMode === 'server' ? 'text-blue-400' : 'text-[var(--fg-tertiary)]'
                                }`} />
                            </div>
                            <div className="flex-1">
                                <h3 className="font-semibold text-[var(--fg-primary)]">Server Storage</h3>
                                <p className="mt-1 text-sm text-[var(--fg-secondary)]">
                                    Conversations stored on server for faster context assembly.
                                </p>
                                <div className="mt-3 flex items-center gap-4 text-xs text-[var(--fg-tertiary)]">
                                    <span className="flex items-center gap-1">
                                        <Zap className="w-3.5 h-3.5" /> Faster responses
                                    </span>
                                    <span className="flex items-center gap-1">
                                        <Cloud className="w-3.5 h-3.5" /> Encrypted on server
                                    </span>
                                </div>
                            </div>
                        </div>
                    </button>
                </div>

                {/* Error */}
                {error && (
                    <div className="mx-8 p-3 bg-red-500/10 border border-red-500/30 rounded-lg text-sm text-red-400">
                        {error}
                    </div>
                )}

                {/* Footer */}
                <div className="px-8 py-6 flex items-center justify-between">
                    <p className="text-xs text-[var(--fg-tertiary)]">
                        You can change this later in Settings
                    </p>
                    <button
                        onClick={handleSetup}
                        disabled={isSettingUp}
                        className="flex items-center gap-2 px-6 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isSettingUp ? (
                            <>
                                <Loader2 className="w-4 h-4 animate-spin" />
                                Setting up...
                            </>
                        ) : (
                            <>
                                Get Started
                                <ArrowRight className="w-4 h-4" />
                            </>
                        )}
                    </button>
                </div>
            </div>
        </div>
    );
};
