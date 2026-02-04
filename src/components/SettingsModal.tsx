'use client';
import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { X, Database, Cloud, Shield, Zap, HardDrive, Server, ChevronRight, Info, Loader2, Code, Key, CheckCircle2 } from 'lucide-react';
import type { ApiConfig, BackendSettings } from '../types/settings';

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
    editor: {};
    account: {
        bladeUrl: string;
        apiKey: string;
        userId: string;
        theme: string;
        markdownView: string;
    };
    localAi: {
        ollamaEnabled: boolean;
        ollamaUrl: string;
        openaiCompatEnabled: boolean;
        openaiCompatUrl: string;
    };
    allowGitIgnoredFiles?: boolean;  // Per-project setting
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
    editor: {},
    account: {
        bladeUrl: '',
        apiKey: '',
        userId: '',
        theme: 'system',
        markdownView: 'split',
    },
    localAi: {
        ollamaEnabled: false,
        ollamaUrl: 'http://localhost:11434',
        openaiCompatEnabled: false,
        openaiCompatUrl: 'http://localhost:8080/v1',
    },
    allowGitIgnoredFiles: false,  // Default: respect .gitignore
};







function backendGlobalToFrontend(backend: ApiConfig): Pick<SettingsState, 'account' | 'localAi'> {
    return {
        account: {
            bladeUrl: '', // Always empty, internal only
            apiKey: backend.api_key,
            userId: backend.user_id,
            theme: backend.theme,
            markdownView: backend.markdown_view,
        },
        localAi: {
            ollamaEnabled: backend.ollama_enabled,
            ollamaUrl: backend.ollama_url,
            openaiCompatEnabled: backend.openai_compat_enabled,
            openaiCompatUrl: backend.openai_compat_url,
        },
    };
}

function frontendGlobalToBackend(frontend: SettingsState): ApiConfig {
    return {
        blade_url: '', // Frontend does not set this
        api_key: frontend.account.apiKey,
        user_id: frontend.account.userId,
        ollama_enabled: frontend.localAi.ollamaEnabled,
        ollama_url: frontend.localAi.ollamaUrl,
        openai_compat_enabled: frontend.localAi.openaiCompatEnabled,
        openai_compat_url: frontend.localAi.openaiCompatUrl,
        theme: frontend.account.theme,
        markdown_view: frontend.account.markdownView,
    };
}

function backendToFrontend(backend: BackendSettings): Omit<SettingsState, 'account' | 'localAi'> {
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
        editor: {},
        allowGitIgnoredFiles: backend.allow_gitignored_files,
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
            telemetry: false,
        },
        editor: {},
        allow_gitignored_files: frontend.allowGitIgnoredFiles || false,
    };
}

interface SettingsModalProps {
    isOpen: boolean;
    onClose: () => void;
    workspacePath?: string | null;
    onRefreshModels?: () => Promise<import('../types/chat').ModelInfo[]>;
}

type SettingsSection = 'account' | 'localai' | 'storage' | 'context' | 'privacy' | 'editor';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, workspacePath, onRefreshModels }) => {
    const [settings, setSettings] = useState<SettingsState>(defaultSettings);
    const [activeSection, setActiveSection] = useState<SettingsSection>('account');
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
        if (!isOpen) return;

        const loadSettings = async () => {
            setIsLoading(true);
            setError(null);
            try {
                // Load Global Settings (Account)
                const globalSettings = await invoke<ApiConfig>('get_global_settings');
                const globalFrontend = backendGlobalToFrontend(globalSettings);

                let mergedSettings = { ...defaultSettings, ...globalFrontend };

                // Load Project Settings (if workspace open)
                if (workspacePath) {
                    try {
                        const backendSettings = await invoke<BackendSettings>('load_project_settings', {
                            projectPath: workspacePath,
                        });
                        mergedSettings = {
                            ...mergedSettings,
                            ...backendToFrontend(backendSettings),
                        };
                        console.log('[Settings] Loaded project settings:', backendSettings);
                    } catch (e) {
                        console.error('[Settings] Failed to load project settings:', e);
                        // Don't fail completely, just use defaults for project
                    }
                }

                setSettings(mergedSettings);
                setHasChanges(false);
                console.log('[Settings] Loaded settings:', mergedSettings);
            } catch (e) {
                console.error('[Settings] Failed to load global settings:', e);
                setError(String(e));
                setSettings(defaultSettings);
            } finally {
                setIsLoading(false);
            }
        };

        loadSettings();
    }, [isOpen, workspacePath]);

    const updateSettings = <K extends 'storage' | 'context' | 'privacy' | 'editor' | 'account' | 'localAi'>(
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
        setIsSaving(true);
        setError(null);
        try {
            // Save Global Settings
            const globalSettings = frontendGlobalToBackend(settings);
            await invoke('save_global_settings', {
                settings: globalSettings,
            });
            await emit('global-settings-changed');

            // Save Project Settings (if workspace is open)
            if (workspacePath) {
                const backendSettings = frontendToBackend(settings);
                await invoke('save_project_settings', {
                    projectPath: workspacePath,
                    settings: backendSettings,
                });
            }

            // Refresh models if local AI settings changed
            if (onRefreshModels) {
                await onRefreshModels();
            }

            console.log('[Settings] Saved settings');
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
        { id: 'account', label: 'Account', icon: <Key className="w-4 h-4" /> },
        { id: 'localai', label: 'Local AI', icon: <Server className="w-4 h-4" /> },
        { id: 'storage', label: 'Storage', icon: <Database className="w-4 h-4" /> },
        ...(workspacePath ? [
            { id: 'context', label: 'Context', icon: <Zap className="w-4 h-4" /> },
            // { id: 'privacy', label: 'Privacy', icon: <Shield className="w-4 h-4" /> },
        ] as const : []),
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
                                className={`w-full flex items-center gap-3 px-4 py-2.5 text-sm transition-colors ${activeSection === section.id
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
                                {activeSection === 'localai' && (
                                    <LocalAiSettings
                                        settings={settings.localAi}
                                        onChange={(updates) => updateSettings('localAi', updates)}
                                        onRefreshModels={onRefreshModels}
                                    />
                                )}
                                {activeSection === 'context' && (
                                    <ContextSettings
                                        settings={settings.context}
                                        onChange={(updates) => updateSettings('context', updates)}
                                        allowGitIgnoredFiles={settings.allowGitIgnoredFiles || false}
                                        onAllowGitIgnoredFilesChange={(value) => {
                                            setSettings(prev => ({ ...prev, allowGitIgnoredFiles: value }));
                                            setHasChanges(true);
                                        }}
                                    />
                                )}
                                {activeSection === 'privacy' && (
                                    <PrivacySettings
                                        settings={settings.privacy}
                                        onChange={(updates) => updateSettings('privacy', updates)}
                                    />
                                )}
                                {activeSection === 'account' && (
                                    <AccountSettings
                                        settings={settings.account}
                                        onChange={(updates) => updateSettings('account', updates)}
                                    />
                                )}
                                {activeSection === 'editor' && (
                                    <EditorSettings
                                        settings={settings.editor}
                                        onChange={(updates) => updateSettings('editor', updates)}
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

interface LocalAiSettingsProps {
    settings: SettingsState['localAi'];
    onChange: (updates: Partial<SettingsState['localAi']>) => void;
    onRefreshModels?: () => Promise<import('../types/chat').ModelInfo[]>;
}

const LocalAiSettings: React.FC<LocalAiSettingsProps> = ({ settings, onChange, onRefreshModels }) => {
    const [isTestingOllama, setIsTestingOllama] = useState(false);
    const [ollamaTestResult, setOllamaTestResult] = useState<'idle' | 'success' | 'error'>('idle');
    const [ollamaTestMessage, setOllamaTestMessage] = useState<string | null>(null);
    const [isRefreshingOllama, setIsRefreshingOllama] = useState(false);

    const [isTestingOpenAI, setIsTestingOpenAI] = useState(false);
    const [openaiTestResult, setOpenaiTestResult] = useState<'idle' | 'success' | 'error'>('idle');
    const [openaiTestMessage, setOpenaiTestMessage] = useState<string | null>(null);
    const [isRefreshingOpenAI, setIsRefreshingOpenAI] = useState(false);

    const handleTestOllamaConnection = async () => {
        setIsTestingOllama(true);
        setOllamaTestResult('idle');
        setOllamaTestMessage(null);
        try {
            await invoke('test_ollama_connection', { ollamaUrl: settings.ollamaUrl });
            setOllamaTestResult('success');
            setOllamaTestMessage('Connection successful.');
        } catch (e) {
            setOllamaTestResult('error');
            setOllamaTestMessage(String(e));
        } finally {
            setIsTestingOllama(false);
        }
    };

    const handleRefreshOllamaModels = async () => {
        setIsRefreshingOllama(true);
        try {
            await invoke('refresh_ollama_models');
            if (onRefreshModels) {
                await onRefreshModels();
            }
        } catch (e) {
            console.error('[Settings] Failed to refresh Ollama models:', e);
        } finally {
            setIsRefreshingOllama(false);
        }
    };

    const handleTestOpenAIConnection = async () => {
        setIsTestingOpenAI(true);
        setOpenaiTestResult('idle');
        setOpenaiTestMessage(null);
        try {
            await invoke('test_openai_compat_connection', { serverUrl: settings.openaiCompatUrl });
            setOpenaiTestResult('success');
            setOpenaiTestMessage('Connection successful.');
        } catch (e) {
            setOpenaiTestResult('error');
            setOpenaiTestMessage(String(e));
        } finally {
            setIsTestingOpenAI(false);
        }
    };

    const handleRefreshOpenAIModels = async () => {
        setIsRefreshingOpenAI(true);
        try {
            await invoke('refresh_openai_compat_models');
            if (onRefreshModels) {
                await onRefreshModels();
            }
        } catch (e) {
            console.error('[Settings] Failed to refresh OpenAI-compatible models:', e);
        } finally {
            setIsRefreshingOpenAI(false);
        }
    };

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Local AI</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Configure local AI providers running on your machine or network.
                </p>
            </div>

            {/* Ollama Section */}
            <div className="border border-[var(--border-subtle)] rounded-lg p-4 space-y-4">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">Ollama</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Enable and connect to an Ollama server.
                        </div>
                    </div>
                    <Toggle
                        checked={settings.ollamaEnabled}
                        onChange={(checked) => onChange({ ollamaEnabled: checked })}
                    />
                </div>

                <div className="space-y-2">
                    <label className="text-xs text-[var(--fg-secondary)] block">Server URL</label>
                    <input
                        type="text"
                        value={settings.ollamaUrl}
                        onChange={(e) => onChange({ ollamaUrl: e.target.value })}
                        placeholder="http://localhost:11434"
                        disabled={!settings.ollamaEnabled}
                        className="w-full bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-lg py-2 px-3 text-sm text-[var(--fg-primary)] focus:outline-none focus:border-[var(--accent-primary)] placeholder-[var(--fg-tertiary)] disabled:opacity-60"
                    />
                </div>

                <div className="flex items-center gap-3">
                    <button
                        type="button"
                        onClick={handleTestOllamaConnection}
                        disabled={!settings.ollamaEnabled || isTestingOllama}
                        className="px-3 py-1.5 text-xs font-medium rounded-md border border-[var(--border-subtle)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isTestingOllama ? 'Testing...' : 'Test Connection'}
                    </button>
                    <button
                        type="button"
                        onClick={handleRefreshOllamaModels}
                        disabled={!settings.ollamaEnabled || isRefreshingOllama}
                        className="px-3 py-1.5 text-xs font-medium rounded-md border border-[var(--border-subtle)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isRefreshingOllama ? 'Refreshing...' : 'Refresh Models'}
                    </button>
                    {ollamaTestMessage && (
                        <span
                            className={`text-xs ${ollamaTestResult === 'success'
                                ? 'text-emerald-400'
                                : 'text-red-400'
                                }`}
                        >
                            {ollamaTestMessage}
                        </span>
                    )}
                </div>
            </div>

            {/* OpenAI-compatible Section */}
            <div className="border border-[var(--border-subtle)] rounded-lg p-4 space-y-4">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">OpenAI-compatible Server</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Connect to OpenAI-compatible servers (llama.cpp, LocalAI, vLLM, etc.)
                        </div>
                    </div>
                    <Toggle
                        checked={settings.openaiCompatEnabled}
                        onChange={(checked) => onChange({ openaiCompatEnabled: checked })}
                    />
                </div>

                <div className="space-y-2">
                    <label className="text-xs text-[var(--fg-secondary)] block">Server URL</label>
                    <input
                        type="text"
                        value={settings.openaiCompatUrl}
                        onChange={(e) => onChange({ openaiCompatUrl: e.target.value })}
                        placeholder="http://localhost:8080/v1"
                        disabled={!settings.openaiCompatEnabled}
                        className="w-full bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-lg py-2 px-3 text-sm text-[var(--fg-primary)] focus:outline-none focus:border-[var(--accent-primary)] placeholder-[var(--fg-tertiary)] disabled:opacity-60"
                    />
                    <p className="text-xs text-[var(--fg-tertiary)] mt-1">
                        No API key required - these are keyless local servers.
                    </p>
                </div>

                <div className="flex items-center gap-3">
                    <button
                        type="button"
                        onClick={handleTestOpenAIConnection}
                        disabled={!settings.openaiCompatEnabled || isTestingOpenAI}
                        className="px-3 py-1.5 text-xs font-medium rounded-md border border-[var(--border-subtle)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isTestingOpenAI ? 'Testing...' : 'Test Connection'}
                    </button>
                    <button
                        type="button"
                        onClick={handleRefreshOpenAIModels}
                        disabled={!settings.openaiCompatEnabled || isRefreshingOpenAI}
                        className="px-3 py-1.5 text-xs font-medium rounded-md border border-[var(--border-subtle)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isRefreshingOpenAI ? 'Refreshing...' : 'Refresh Models'}
                    </button>
                    {openaiTestMessage && (
                        <span
                            className={`text-xs ${openaiTestResult === 'success'
                                ? 'text-emerald-400'
                                : 'text-red-400'
                                }`}
                        >
                            {openaiTestMessage}
                        </span>
                    )}
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
                        className={`relative p-4 rounded-lg border-2 text-left transition-all ${settings.mode === 'local'
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
                        className={`relative p-4 rounded-lg border-2 text-left transition-all ${settings.mode === 'server'
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
    allowGitIgnoredFiles: boolean;
    onAllowGitIgnoredFilesChange: (value: boolean) => void;
}

const ContextSettings: React.FC<ContextSettingsProps> = ({ settings, onChange, allowGitIgnoredFiles, onAllowGitIgnoredFilesChange }) => {
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
                                className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${settings.compression.model === 'remote'
                                    ? 'bg-emerald-600 text-white'
                                    : 'bg-[var(--bg-app)] text-[var(--fg-secondary)] hover:bg-[var(--bg-surface-hover)]'
                                    }`}
                            >
                                <Cloud className="w-4 h-4 inline-block mr-2" />
                                Remote (Faster)
                            </button>
                            <button
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'local' } })}
                                className={`flex-1 px-3 py-2 rounded-md text-sm transition-colors ${settings.compression.model === 'local'
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

            {/* Gitignore Files */}
            <div className="border-t border-[var(--border-subtle)] pt-4">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-[var(--fg-primary)]">Allow .gitignored Files</div>
                        <div className="text-xs text-[var(--fg-tertiary)]">
                            Include files matched by .gitignore in context
                        </div>
                    </div>
                    <Toggle
                        checked={allowGitIgnoredFiles}
                        onChange={onAllowGitIgnoredFilesChange}
                    />
                </div>
                <p className="text-xs text-[var(--fg-tertiary)] mt-2">
                    {allowGitIgnoredFiles
                        ? 'Gitignored files (e.g., build outputs, secrets) will be accessible to the AI.'
                        : 'Gitignored files are hidden from the AI for security and relevance.'}
                </p>
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
                        We do not collect any telemetry data.
                    </div>
                </div>
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

interface EditorSettingsProps {
    settings: SettingsState['editor'];
    onChange: (updates: Partial<SettingsState['editor']>) => void;
}

const EditorSettings: React.FC<EditorSettingsProps> = ({ settings, onChange }) => {
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Editor</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Configure editor behavior.
                </p>
            </div>
            {/* No settings for now */}
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
            className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 ${checked ? 'bg-emerald-600' : 'bg-[var(--bg-app)]'
                }`}
        >
            <span
                className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow-lg ring-0 transition duration-200 ease-in-out ${checked ? 'translate-x-4' : 'translate-x-0'
                    }`}
            />
        </button>
    );
};

interface AccountSettingsProps {
    settings: SettingsState['account'];
    onChange: (updates: Partial<SettingsState['account']>) => void;
}

const AccountSettings: React.FC<AccountSettingsProps> = ({ settings, onChange }) => {
    const [showKey, setShowKey] = useState(false);

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-[var(--fg-primary)] mb-1">Account & API</h3>
                <p className="text-sm text-[var(--fg-tertiary)] mb-4">
                    Manage your Zaguán Blade connection and subscription.
                </p>
            </div>

            <div className={`border rounded-lg p-4 mb-6 ${settings.apiKey ? 'bg-emerald-500/5 border-emerald-500/20' : 'bg-[var(--bg-app)] border-[var(--border-subtle)]'}`}>
                <div className="flex gap-4">
                    <div className={`p-3 rounded-full h-fit ${settings.apiKey ? 'bg-emerald-500/20' : 'bg-[var(--bg-app)]'}`}>
                        {settings.apiKey ? (
                            <CheckCircle2 className="w-6 h-6 text-emerald-500" />
                        ) : (
                            <Key className="w-6 h-6 text-emerald-500" />
                        )}
                    </div>
                    <div className="flex-1">
                        <h4 className="font-medium text-[var(--fg-primary)] mb-1">
                            {settings.apiKey ? 'Active Subscription' : 'Zaguán Blade Pro'}
                        </h4>
                        <p className="text-sm text-[var(--fg-secondary)] mb-3">
                            {settings.apiKey
                                ? 'Your subscription is active. AI features are enabled.'
                                : 'You need an active subscription to use AI features.'}
                        </p>
                        <a
                            href={settings.apiKey ? "https://zaguanai.com/dashboard" : "https://zaguanai.com/pricing"}
                            target="_blank"
                            rel="noreferrer"
                            className="text-sm text-emerald-500 hover:text-emerald-400 font-medium"
                        >
                            {settings.apiKey ? "Manage Subscription →" : "Get Subscription →"}
                        </a>
                    </div>
                </div>
            </div>

            <div className="space-y-2">
                <label className="text-sm font-medium text-[var(--fg-primary)] block">
                    API Key
                </label>
                <div className="flex gap-2">
                    <div className="relative flex-1">
                        <input
                            type={showKey ? 'text' : 'password'}
                            value={settings.apiKey}
                            onChange={(e) => onChange({ apiKey: e.target.value })}
                            placeholder="sk-..."
                            className="w-full bg-[var(--bg-app)] border border-[var(--border-subtle)] rounded-lg py-2 pl-3 pr-10 text-sm text-[var(--fg-primary)] focus:outline-none focus:border-[var(--accent-primary)] placeholder-[var(--fg-tertiary)]"
                        />
                        <button
                            type="button"
                            onClick={() => setShowKey(!showKey)}
                            className="absolute right-3 top-2 text-[var(--fg-tertiary)] hover:text-[var(--fg-secondary)]"
                        >
                            {showKey ? (
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" /><path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" /><path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" /><line x1="2" x2="22" y1="2" y2="22" /></svg>
                            ) : (
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" /><circle cx="12" cy="12" r="3" /></svg>
                            )}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
