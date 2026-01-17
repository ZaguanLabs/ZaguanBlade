'use client';
import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { X, Database, Cloud, Shield, Zap, HardDrive, Server, ChevronRight, Info, Loader2 } from 'lucide-react';

type StorageMode = 'local' | 'server';

interface SettingsState {
    storage: {
        mode: StorageMode;
        syncMetadata: boolean;
        cache: {
            enabled: boolean;
            maxSizeMb: number;
        };
    };
    context: {
        maxTokens: number;
        compression: {
            enabled: boolean;
            model: 'local' | 'remote';
        };
    };
    privacy: {
        telemetry: boolean;
    };
}

const defaultSettings: SettingsState = {
    storage: {
        mode: 'local',
        syncMetadata: true,
        cache: {
            enabled: true,
            maxSizeMb: 100,
        },
    },
    context: {
        maxTokens: 8000,
        compression: {
            enabled: true,
            model: 'remote',
        },
    },
    privacy: {
        telemetry: false,
    },
};

interface BackendSettings {
    storage: {
        mode: 'local' | 'server';
        sync_metadata: boolean;
        cache: {
            enabled: boolean;
            max_size_mb: number;
        };
    };
    context: {
        max_tokens: number;
        compression: {
            enabled: boolean;
            model: 'local' | 'remote';
        };
    };
    privacy: {
        telemetry: boolean;
    };
}

function backendToFrontend(backend: BackendSettings): SettingsState {
    return {
        storage: {
            mode: backend.storage.mode,
            syncMetadata: backend.storage.sync_metadata,
            cache: {
                enabled: backend.storage.cache.enabled,
                maxSizeMb: backend.storage.cache.max_size_mb,
            },
        },
        context: {
            maxTokens: backend.context.max_tokens,
            compression: {
                enabled: backend.context.compression.enabled,
                model: backend.context.compression.model,
            },
        },
        privacy: {
            telemetry: backend.privacy.telemetry,
        },
    };
}

function frontendToBackend(frontend: SettingsState): BackendSettings {
    return {
        storage: {
            mode: frontend.storage.mode,
            sync_metadata: frontend.storage.syncMetadata,
            cache: {
                enabled: frontend.storage.cache.enabled,
                max_size_mb: frontend.storage.cache.maxSizeMb,
            },
        },
        context: {
            max_tokens: frontend.context.maxTokens,
            compression: {
                enabled: frontend.context.compression.enabled,
                model: frontend.context.compression.model,
            },
        },
        privacy: {
            telemetry: frontend.privacy.telemetry,
        },
    };
}

interface SettingsModalProps {
    isOpen: boolean;
    onClose: () => void;
    workspacePath?: string | null;
}

type SettingsSection = 'storage' | 'context' | 'privacy';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, workspacePath }) => {
    const [settings, setSettings] = useState<SettingsState>(defaultSettings);
    const [activeSection, setActiveSection] = useState<SettingsSection>('storage');
    const [hasChanges, setHasChanges] = useState(false);
    const [isLoading, setIsLoading] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!isOpen) return;

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onClose();
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    useEffect(() => {
        if (!isOpen || !workspacePath) return;

        const loadSettings = async () => {
            setIsLoading(true);
            setError(null);
            try {
                const backendSettings = await invoke<BackendSettings>('load_project_settings', {
                    projectPath: workspacePath,
                });
                setSettings(backendToFrontend(backendSettings));
                setHasChanges(false);
                console.log('[Settings] Loaded from backend:', backendSettings);
            } catch (e) {
                console.error('[Settings] Failed to load:', e);
                setError(String(e));
                setSettings(defaultSettings);
            } finally {
                setIsLoading(false);
            }
        };

        loadSettings();
    }, [isOpen, workspacePath]);

    const updateSettings = <K extends keyof SettingsState>(
        section: K,
        updates: Partial<SettingsState[K]>
    ) => {
        setSettings(prev => ({
            ...prev,
            [section]: { ...prev[section], ...updates },
        }));
        setHasChanges(true);
    };

    const handleSave = async () => {
        if (!workspacePath) {
            setError('No workspace path available');
            return;
        }

        setIsSaving(true);
        setError(null);
        try {
            const backendSettings = frontendToBackend(settings);
            await invoke('save_project_settings', {
                projectPath: workspacePath,
                settings: backendSettings,
            });
            console.log('[Settings] Saved to backend:', backendSettings);
            setHasChanges(false);
            onClose();
        } catch (e) {
            console.error('[Settings] Failed to save:', e);
            setError(String(e));
        } finally {
            setIsSaving(false);
        }
    };

    if (!isOpen) return null;

    const sections: { id: SettingsSection; label: string; icon: React.ReactNode }[] = [
        { id: 'storage', label: 'Storage', icon: <Database className="w-4 h-4" /> },
        { id: 'context', label: 'Context', icon: <Zap className="w-4 h-4" /> },
        { id: 'privacy', label: 'Privacy', icon: <Shield className="w-4 h-4" /> },
    ];

    return (
        <div className="fixed inset-0 z-[9999] flex items-center justify-center">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                onClick={onClose}
            />

            {/* Modal */}
            <div className="relative bg-[var(--bg-surface)] border border-[var(--border-focus)] rounded-lg shadow-2xl w-[800px] h-[620px] flex flex-col animate-in fade-in zoom-in-95 duration-150">
                {/* Header */}
                <div className="flex items-center justify-between px-6 py-4 border-b border-[var(--border-subtle)]">
                    <h2 className="text-lg font-semibold text-[var(--fg-primary)]">Settings</h2>
                    <button
                        onClick={onClose}
                        className="p-1.5 text-[var(--fg-tertiary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="flex flex-1 overflow-hidden">
                    {/* Sidebar */}
                    <div className="w-48 border-r border-[var(--border-subtle)] py-2">
                        {sections.map(section => (
                            <button
                                key={section.id}
                                onClick={() => setActiveSection(section.id)}
                                className={`w-full flex items-center gap-3 px-4 py-2.5 text-sm transition-colors ${
                                    activeSection === section.id
                                        ? 'bg-[var(--bg-surface-hover)] text-[var(--fg-primary)] border-l-2 border-[var(--accent-primary)]'
                                        : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)] hover:text-[var(--fg-primary)] border-l-2 border-transparent'
                                }`}
                            >
                                {section.icon}
                                {section.label}
                            </button>
                        ))}
                    </div>

                    {/* Main Content */}
                    <div className="flex-1 overflow-y-auto p-6">
                        {isLoading ? (
                            <div className="flex items-center justify-center h-full">
                                <Loader2 className="w-6 h-6 text-[var(--fg-tertiary)] animate-spin" />
                            </div>
                        ) : (
                            <>
                                {error && (
                                    <div className="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg text-sm text-red-400">
                                        {error}
                                    </div>
                                )}
                                {activeSection === 'storage' && (
                                    <StorageSettings
                                        settings={settings.storage}
                                        onChange={(updates) => updateSettings('storage', updates)}
                                    />
                                )}
                                {activeSection === 'context' && (
                                    <ContextSettings
                                        settings={settings.context}
                                        onChange={(updates) => updateSettings('context', updates)}
                                    />
                                )}
                                {activeSection === 'privacy' && (
                                    <PrivacySettings
                                        settings={settings.privacy}
                                        onChange={(updates) => updateSettings('privacy', updates)}
                                    />
                                )}
                            </>
                        )}
                    </div>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-[var(--border-subtle)]">
                    <button
                        onClick={onClose}
                        className="px-4 py-2 text-sm font-medium text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] rounded-md transition-colors"
                    >
                        Cancel
                    </button>
                    <button
                        onClick={handleSave}
                        disabled={!hasChanges || isSaving}
                        className="px-4 py-2 text-sm font-medium bg-emerald-600 hover:bg-emerald-500 text-white rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                    >
                        {isSaving && <Loader2 className="w-4 h-4 animate-spin" />}
                        {isSaving ? 'Saving...' : 'Save Changes'}
                    </button>
                </div>
            </div>
        </div>
    );
};

interface StorageSettingsProps {
    settings: SettingsState['storage'];
    onChange: (updates: Partial<SettingsState['storage']>) => void;
}

const StorageSettings: React.FC<StorageSettingsProps> = ({ settings, onChange }) => {
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Storage Mode</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Choose where your conversation history is stored.
                </p>

                <div className="grid grid-cols-2 gap-4">
                    {/* Local Storage Option */}
                    <button
                        onClick={() => onChange({ mode: 'local' })}
                        className={`relative p-4 rounded-lg border-2 text-left transition-all ${
                            settings.mode === 'local'
                                ? 'border-emerald-500 bg-emerald-500/10'
                                : 'border-[var(--border-subtle)] hover:border-[var(--border-focus)]'
                        }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`p-2 rounded-lg ${settings.mode === 'local' ? 'bg-emerald-500/20' : 'bg-[var(--bg-app)]'}`}>
                                <HardDrive className={`w-5 h-5 ${settings.mode === 'local' ? 'text-emerald-400' : 'text-[var(--fg-secondary)]'}`} />
                            </div>
                            <div>
                                <div className="font-medium text-[var(--fg-primary)]">Local Storage</div>
                                <div className="text-xs text-[var(--fg-tertiary)]">Maximum privacy</div>
                            </div>
                        </div>
                        <ul className="text-xs text-[var(--fg-secondary)] space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-emerald-400" />
                                Code stays on your machine
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-emerald-400" />
                                Privacy secured
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-emerald-400" />
                                Higher network latency
                            </li>
                        </ul>
                        {settings.mode === 'local' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-emerald-500" />
                        )}
                    </button>

                    {/* Server Storage Option */}
                    <button
                        onClick={() => onChange({ mode: 'server' })}
                        className={`relative p-4 rounded-lg border-2 text-left transition-all ${
                            settings.mode === 'server'
                                ? 'border-blue-500 bg-blue-500/10'
                                : 'border-[var(--border-subtle)] hover:border-[var(--border-focus)]'
                        }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`p-2 rounded-lg ${settings.mode === 'server' ? 'bg-blue-500/20' : 'bg-[var(--bg-app)]'}`}>
                                <Server className={`w-5 h-5 ${settings.mode === 'server' ? 'text-blue-400' : 'text-[var(--fg-secondary)]'}`} />
                            </div>
                            <div>
                                <div className="font-medium text-[var(--fg-primary)]">Server Storage</div>
                                <div className="text-xs text-[var(--fg-tertiary)]">Maximum performance</div>
                            </div>
                        </div>
                        <ul className="text-xs text-[var(--fg-secondary)] space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-blue-400" />
                                Faster context retrieval
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-blue-400" />
                                Cross-device sync
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-blue-400" />
                                Lower network latency
                            </li>
                        </ul>
                        {settings.mode === 'server' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-blue-500" />
                        )}
                    </button>
                </div>

                {/* Info box */}
                <div className="mt-4 p-3 bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-lg flex gap-3">
                    <Info className="w-4 h-4 text-[var(--fg-tertiary)] shrink-0 mt-0.5" />
                    <p className="text-xs text-[var(--fg-tertiary)]">
                        {settings.mode === 'local'
                            ? 'Your conversations are stored in the .zblade/ folder in your project. Code never leaves your machine.'
                            : 'Your conversations are stored on Zaguán servers. This enables faster context retrieval and cross-device sync.'}
                    </p>
                </div>
            </div>

            {/* Sync Metadata Toggle (only for local mode) */}
            {settings.mode === 'local' && (
                <div className="flex items-center justify-between py-3 border-t border-[var(--border-subtle)]">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">Sync Metadata</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Sync conversation titles and tags to server (no code)
                        </div>
                    </div>
                    <Toggle
                        checked={settings.syncMetadata}
                        onChange={(checked) => onChange({ syncMetadata: checked })}
                    />
                </div>
            )}

            {/* Cache Settings */}
            <div className="border-t border-[var(--border-subtle)] pt-4">
                <div className="flex items-center justify-between mb-3">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">Enable Cache</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Cache recent context for faster access
                        </div>
                    </div>
                    <Toggle
                        checked={settings.cache.enabled}
                        onChange={(checked) => onChange({ cache: { ...settings.cache, enabled: checked } })}
                    />
                </div>

                {settings.cache.enabled && (
                    <div className="mt-3">
                        <label className="text-xs text-[var(--fg-secondary)] mb-1 block">
                            Max Cache Size: {settings.cache.maxSizeMb} MB
                        </label>
                        <input
                            type="range"
                            min="10"
                            max="500"
                            step="10"
                            value={settings.cache.maxSizeMb}
                            onChange={(e) => onChange({ cache: { ...settings.cache, maxSizeMb: parseInt(e.target.value) } })}
                            className="w-full h-1.5 bg-[var(--bg-app)] rounded-lg appearance-none cursor-pointer accent-emerald-500"
                        />
                        <div className="flex justify-between text-[10px] text-[var(--fg-tertiary)] mt-1">
                            <span>10 MB</span>
                            <span>500 MB</span>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
};

interface ContextSettingsProps {
    settings: SettingsState['context'];
    onChange: (updates: Partial<SettingsState['context']>) => void;
}

const ContextSettings: React.FC<ContextSettingsProps> = ({ settings, onChange }) => {
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Context Settings</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Configure how context is assembled and compressed.
                </p>
            </div>

            {/* Max Tokens */}
            <div>
                <label className="text-sm font-medium text-[var(--fg-primary)] mb-2 block">
                    Max Context Tokens: {settings.maxTokens.toLocaleString()}
                </label>
                <input
                    type="range"
                    min="2000"
                    max="32000"
                    step="1000"
                    value={settings.maxTokens}
                    onChange={(e) => onChange({ maxTokens: parseInt(e.target.value) })}
                    className="w-full h-1.5 bg-[var(--bg-app)] rounded-lg appearance-none cursor-pointer accent-emerald-500"
                />
                <div className="flex justify-between text-[10px] text-[var(--fg-tertiary)] mt-1">
                    <span>2K</span>
                    <span>32K</span>
                </div>
                <p className="text-xs text-[var(--fg-tertiary)] mt-2">
                    Higher values provide more context but increase latency and cost.
                </p>
            </div>

            {/* Compression */}
            <div className="border-t border-[var(--border-subtle)] pt-4">
                <div className="flex items-center justify-between mb-3">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">Enable Compression</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Use AI to intelligently compress context
                        </div>
                    </div>
                    <Toggle
                        checked={settings.compression.enabled}
                        onChange={(checked) => onChange({ compression: { ...settings.compression, enabled: checked } })}
                    />
                </div>

                {settings.compression.enabled && (
                    <div className="mt-4 space-y-2">
                        <label className="text-xs text-[var(--fg-secondary)] block">Compression Model</label>
                        <div className="flex gap-3">
                            <button
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'remote' } })}
                                className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${
                                    settings.compression.model === 'remote'
                                        ? 'bg-emerald-600 text-white'
                                        : 'bg-[var(--bg-app)] text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                }`}
                            >
                                <Cloud className="w-4 h-4 inline-block mr-2" />
                                Remote (Faster)
                            </button>
                            <button
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'local' } })}
                                className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${
                                    settings.compression.model === 'local'
                                        ? 'bg-emerald-600 text-white'
                                        : 'bg-[var(--bg-app)] text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                }`}
                            >
                                <HardDrive className="w-4 h-4 inline-block mr-2" />
                                Local (Private)
                            </button>
                        </div>
                        <p className="text-xs text-[var(--fg-tertiary)]">
                            {settings.compression.model === 'remote'
                                ? 'Uses a fast cloud model for compression. Summaries are sent to server.'
                                : 'Uses a local model for compression. Everything stays on your machine.'}
                        </p>
                    </div>
                )}
            </div>
        </div>
    );
};

interface PrivacySettingsProps {
    settings: SettingsState['privacy'];
    onChange: (updates: Partial<SettingsState['privacy']>) => void;
}

const PrivacySettings: React.FC<PrivacySettingsProps> = ({ settings, onChange }) => {
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Privacy</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Control what data is shared.
                </p>
            </div>

            {/* Telemetry */}
            <div className="flex items-center justify-between py-3">
                <div>
                    <div className="text-sm font-medium text-[var(--fg-primary)]">Usage Telemetry</div>
                    <div className="text-xs text-[var(--fg-tertiary)]">
                        Help improve Zaguán Blade by sending anonymous usage data
                    </div>
                </div>
                <Toggle
                    checked={settings.telemetry}
                    onChange={(checked) => onChange({ telemetry: checked })}
                />
            </div>

            <div className="p-3 bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-lg">
                <div className="flex gap-3">
                    <Shield className="w-4 h-4 text-emerald-400 shrink-0 mt-0.5" />
                    <div className="text-xs text-[var(--fg-tertiary)]">
                        <p className="font-medium text-[var(--fg-secondary)] mb-1">Your code is never shared</p>
                        <p>
                            Telemetry only includes feature usage, performance metrics, and error reports.
                            No code, file contents, or conversation data is ever collected.
                        </p>
                    </div>
                </div>
            </div>
        </div>
    );
};

interface ToggleProps {
    checked: boolean;
    onChange: (checked: boolean) => void;
}

const Toggle: React.FC<ToggleProps> = ({ checked, onChange }) => {
    return (
        <button
            role="switch"
            aria-checked={checked}
            onClick={() => onChange(!checked)}
            className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 ${
                checked ? 'bg-emerald-600' : 'bg-[var(--bg-app)]'
            }`}
        >
            <span
                className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow-lg ring-0 transition duration-200 ease-in-out ${
                    checked ? 'translate-x-4' : 'translate-x-0'
                }`}
            />
        </button>
    );
};
