'use client';
import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { X, Database, Cloud, Shield, Zap, HardDrive, Server, ChevronRight, ChevronDown, Info, Loader2, Code, Key, CheckCircle2, Palette, Check } from 'lucide-react';
import type { BackendSettings, LocalAiConfig, RemoteAiConfig } from '../types/settings';
import i18n, { normalizeAppLanguage, supportedAppLanguages, languageI18nKey, type AppLanguage } from '../i18n';
import zbladeLogoUrl from '../assets/zblade-in-app-logo.png';
import { availableThemes, normalizeThemeId } from '../themes';
import { formatUnknownBackendError } from '../utils/backendErrors';
import { ScrollArea } from './ui/ScrollArea';

type StorageMode = 'local' | 'server';

function normalizeOpenAiCompatUrl(url: string): string {
    const trimmed = url.trim().replace(/\/+$/, '');
    return trimmed.replace(/\/v1$/i, '').replace(/\/+$/, '');
}

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
    configuration: {
        theme: string;
        markdownView: string;
        language: AppLanguage;
    };
    account: {
        bladeUrl: string;
        apiKey: string;
        userId: string;
    };
    localAi: {
        ollamaEnabled: boolean;
        ollamaUrl: string;
        ollamaCloudEnabled: boolean;
        ollamaCloudApiKey: string;
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
    configuration: {
        theme: 'zaguan-dark',
        markdownView: 'split',
        language: normalizeAppLanguage(i18n.resolvedLanguage || i18n.language),
    },
    account: {
        bladeUrl: '',
        apiKey: '',
        userId: '',
    },
    localAi: {
        ollamaEnabled: false,
        ollamaUrl: 'http://localhost:11434',
        ollamaCloudEnabled: false,
        ollamaCloudApiKey: '',
        openaiCompatEnabled: false,
        openaiCompatUrl: 'http://localhost:8080',
    },
    allowGitIgnoredFiles: false,  // Default: respect .gitignore
};







function backendRemoteToFrontend(backend: RemoteAiConfig): Pick<SettingsState, 'account' | 'configuration'> {
    return {
        configuration: {
            theme: normalizeThemeId(backend.theme),
            markdownView: backend.markdown_view || 'split',
            language: normalizeAppLanguage(backend.language || i18n.resolvedLanguage || i18n.language),
        },
        account: {
            bladeUrl: '', // Always empty, internal only
            apiKey: backend.api_key,
            userId: backend.user_id,
        },
    };
}

function backendLocalToFrontend(backend: LocalAiConfig): Pick<SettingsState, 'localAi'> {
    return {
        localAi: {
            ollamaEnabled: backend.ollama_enabled,
            ollamaUrl: backend.ollama_url,
            ollamaCloudEnabled: backend.ollama_cloud_enabled,
            ollamaCloudApiKey: backend.ollama_cloud_api_key,
            openaiCompatEnabled: backend.openai_compat_enabled,
            openaiCompatUrl: normalizeOpenAiCompatUrl(backend.openai_compat_url),
        },
    };
}

function frontendRemoteToBackend(frontend: SettingsState): RemoteAiConfig {
    return {
        blade_url: '', // Frontend does not set this
        api_key: frontend.account.apiKey,
        user_id: frontend.account.userId,
        theme: normalizeThemeId(frontend.configuration.theme),
        markdown_view: frontend.configuration.markdownView,
        language: normalizeAppLanguage(frontend.configuration.language),
    };
}

function frontendLocalToBackend(frontend: SettingsState): LocalAiConfig {
    return {
        ollama_enabled: frontend.localAi.ollamaEnabled,
        ollama_url: frontend.localAi.ollamaUrl,
        ollama_cloud_enabled: frontend.localAi.ollamaCloudEnabled,
        ollama_cloud_api_key: frontend.localAi.ollamaCloudApiKey,
        openai_compat_enabled: frontend.localAi.openaiCompatEnabled,
        openai_compat_url: normalizeOpenAiCompatUrl(frontend.localAi.openaiCompatUrl),
    };
}

function backendToFrontend(backend: BackendSettings): Omit<SettingsState, 'account' | 'localAi' | 'configuration'> {
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
    initialSection?: SettingsSection;
    workspacePath?: string | null;
    onRefreshModels?: () => Promise<import('../types/chat').ModelInfo[]>;
}

type SettingsSection = 'configuration' | 'account' | 'localai' | 'storage' | 'context' | 'privacy' | 'editor' | 'about';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, initialSection, workspacePath, onRefreshModels }) => {
    const { t } = useTranslation();
    const [settings, setSettings] = useState<SettingsState>(defaultSettings);
    const [loadedSettings, setLoadedSettings] = useState<SettingsState>(defaultSettings);
    const [activeSection, setActiveSection] = useState<SettingsSection>('configuration');
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
                // Load split settings (remote account + local AI)
                const [remoteSettings, localSettings] = await Promise.all([
                    invoke<RemoteAiConfig>('get_remote_ai_settings'),
                    invoke<LocalAiConfig>('get_local_ai_settings'),
                ]);
                let mergedSettings = {
                    ...defaultSettings,
                    ...backendRemoteToFrontend(remoteSettings),
                    ...backendLocalToFrontend(localSettings),
                };

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
                        console.debug('[Settings] Loaded project settings:', backendSettings);
                    } catch (e) {
                        console.error('[Settings] Failed to load project settings:', e);
                        // Don't fail completely, just use defaults for project
                    }
                }

                setSettings(mergedSettings);
                setLoadedSettings(mergedSettings);
                setHasChanges(false);
                console.debug('[Settings] Loaded settings:', mergedSettings);
            } catch (e) {
                console.error('[Settings] Failed to load global settings:', e);
                setError(formatUnknownBackendError(e));
                setSettings(defaultSettings);
                setLoadedSettings(defaultSettings);
            } finally {
                setIsLoading(false);
            }
        };

        loadSettings();
    }, [isOpen, workspacePath]);

    useEffect(() => {
        if (!isOpen) return;
        setActiveSection(initialSection ?? 'configuration');
    }, [initialSection, isOpen]);

    const updateSettings = <K extends 'storage' | 'context' | 'privacy' | 'editor' | 'configuration' | 'account' | 'localAi'>(
        section: K,
        updates: Partial<SettingsState[K]>
    ) => {
        setSettings(prev => ({
            ...prev,
            [section]: { ...prev[section], ...updates },
        }));
        setHasChanges(true);
    };

    const handleSave = () => {
        if (isSaving || !hasChanges) return;

        const settingsSnapshot = settings;
        const loadedSettingsSnapshot = loadedSettings;
        const workspacePathSnapshot = workspacePath;
        const refreshModels = onRefreshModels;

        setIsSaving(true);
        setError(null);
        setHasChanges(false);
        onClose();

        void (async () => {
            try {
                const remoteSettings = frontendRemoteToBackend(settingsSnapshot);
                const previousRemoteSettings = frontendRemoteToBackend(loadedSettingsSnapshot);
                const localSettings = frontendLocalToBackend(settingsSnapshot);
                const previousLocalSettings = frontendLocalToBackend(loadedSettingsSnapshot);
                const projectSettings = frontendToBackend(settingsSnapshot);
                const previousProjectSettings = frontendToBackend(loadedSettingsSnapshot);

                const remoteSettingsChanged = JSON.stringify(remoteSettings) !== JSON.stringify(previousRemoteSettings);
                const localSettingsChanged = JSON.stringify(localSettings) !== JSON.stringify(previousLocalSettings);
                const projectSettingsChanged = JSON.stringify(projectSettings) !== JSON.stringify(previousProjectSettings);
                const themeChanged = remoteSettings.theme !== previousRemoteSettings.theme;
                const languageChanged = remoteSettings.language !== previousRemoteSettings.language;
                const remoteAccountChanged =
                    remoteSettings.api_key !== previousRemoteSettings.api_key
                    || remoteSettings.user_id !== previousRemoteSettings.user_id
                    || remoteSettings.blade_url !== previousRemoteSettings.blade_url;
                const remoteConfigurationChanged =
                    remoteSettings.markdown_view !== previousRemoteSettings.markdown_view
                    || languageChanged;

                if (remoteSettingsChanged) {
                    await invoke('save_remote_ai_settings', {
                        settings: remoteSettings,
                    });
                }

                if (localSettingsChanged) {
                    await invoke('save_local_ai_settings', {
                        settings: localSettings,
                    });
                }

                // Save Project Settings (if workspace is open)
                if (workspacePathSnapshot && projectSettingsChanged) {
                    await invoke('save_project_settings', {
                        projectPath: workspacePathSnapshot,
                        settings: projectSettings,
                    });
                }

                if (themeChanged) {
                    await emit('theme-changed');
                }

                if (languageChanged) {
                    await i18n.changeLanguage(normalizeAppLanguage(remoteSettings.language));
                }

                if (remoteAccountChanged || remoteConfigurationChanged) {
                    await emit('remote-settings-changed');
                }

                if (localSettingsChanged) {
                    await emit('local-ai-settings-changed');
                }

                if (projectSettingsChanged) {
                    await emit('project-settings-changed');
                }

                if (refreshModels && localSettingsChanged) {
                    await refreshModels();
                }

                setLoadedSettings(settingsSnapshot);
                console.debug('[Settings] Saved settings');
            } catch (e) {
                console.error('[Settings] Failed to save in background:', e);
            }
        })();
    };

    if (!isOpen) return null;

    const sections: { id: SettingsSection; label: string; icon: React.ReactNode }[] = [
        { id: 'configuration', label: t('settings.navigation.configuration'), icon: <Palette className="w-4 h-4" /> },
        { id: 'account', label: t('settings.navigation.account'), icon: <Key className="w-4 h-4" /> },
        { id: 'localai', label: t('settings.navigation.localAi'), icon: <Server className="w-4 h-4" /> },
        { id: 'storage', label: t('settings.navigation.storage'), icon: <Database className="w-4 h-4" /> },
        ...(workspacePath ? [
            { id: 'context', label: t('settings.navigation.context'), icon: <Zap className="w-4 h-4" /> },
            // { id: 'privacy', label: 'Privacy', icon: <Shield className="w-4 h-4" /> },
        ] as const : []),
        { id: 'about', label: t('settings.navigation.about'), icon: <Info className="w-4 h-4" /> },
    ];

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center p-6">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/72"
                onClick={onClose}
            />

            {/* Modal */}
            <div className="relative w-full max-w-[980px] h-[min(760px,92vh)] flex flex-col animate-in fade-in zoom-in-95 duration-150 rounded-(--panel-radius) border border-(--border-default) bg-(--bg-panel) shadow-(--shadow-xl) overflow-hidden">
                {/* Header */}
                <div className="flex items-start justify-between px-6 py-4 border-b border-(--border-default) bg-[color-mix(in_srgb,var(--bg-panel)_82%,var(--bg-surface))]">
                    <div>
                        <h2 className="text-lg font-semibold text-(--fg-primary)">{t('settings.title')}</h2>
                        <p className="text-xs text-(--fg-tertiary) mt-0.5">{t('settings.subtitle')}</p>
                    </div>
                    <button
                        onClick={onClose}
                        className="rounded-md p-1.5 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="flex flex-1 overflow-hidden">
                    {/* Sidebar */}
                    <div className="w-56 border-r border-(--border-default) py-3 px-2 bg-[color-mix(in_srgb,var(--bg-app)_82%,var(--bg-panel))]">
                        {sections.map(section => (
                            <button
                                key={section.id}
                                onClick={() => setActiveSection(section.id)}
                                className={`w-full flex items-center justify-between gap-3 rounded-[calc(var(--panel-radius)*0.65)] px-3 py-2.5 text-sm transition-colors border ${activeSection === section.id
                                    ? 'bg-(--bg-active) text-(--fg-primary) border-(--border-focus)'
                                    : 'text-(--fg-secondary) border-transparent hover:bg-(--bg-surface) hover:text-(--fg-primary)'
                                    }`}
                            >
                                <span className="flex items-center gap-2.5">
                                    {section.icon}
                                    {section.label}
                                </span>
                                {activeSection === section.id && <ChevronRight className="w-3.5 h-3.5 text-(--accent-ai)" />}
                            </button>
                        ))}
                    </div>

                    {/* Main Content */}
                    <ScrollArea className="flex-1 p-6 bg-(--bg-editor)">
                        {isLoading ? (
                            <div className="flex items-center justify-center h-full">
                                <Loader2 className="w-6 h-6 text-(--fg-tertiary) animate-spin" />
                            </div>
                        ) : (
                            <>
                                {error && (
                                    <div className="mb-4 rounded-[calc(var(--panel-radius)*0.75)] border border-[color-mix(in_srgb,var(--state-danger)_32%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] p-3 text-sm text-(--state-danger)">
                                        {error}
                                    </div>
                                )}
                                {activeSection === 'configuration' && (
                                    <ConfigurationSettings
                                        settings={settings.configuration}
                                        onChange={(updates) => updateSettings('configuration', updates)}
                                    />
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
                                {activeSection === 'about' && <AboutSettings />}
                            </>
                        )}
                    </ScrollArea>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-between gap-3 px-6 py-3 border-t border-(--border-default) bg-[color-mix(in_srgb,var(--bg-panel)_84%,var(--bg-surface))]">
                    <div className="text-xs text-(--fg-tertiary)">
                        {hasChanges ? t('settings.unsavedChangesNotice') : t('settings.allChangesSaved')}
                    </div>
                    <div className="flex items-center gap-2">
                        <button
                            onClick={onClose}
                            className="rounded-[calc(var(--panel-radius)*0.65)] px-4 py-2 text-sm font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                        >
                            {t('common.cancel')}
                        </button>
                        <button
                            onClick={handleSave}
                            disabled={!hasChanges || isSaving}
                            className="flex items-center gap-2 rounded-[calc(var(--panel-radius)*0.65)] bg-(--accent-ai) px-4 py-2 text-sm font-medium text-(--fg-bright) transition-colors hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                            {isSaving && <Loader2 className="w-4 h-4 animate-spin" />}
                            {isSaving ? t('statusBar.saving') : t('settings.saveChanges')}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};

interface ConfigurationSettingsProps {
    settings: SettingsState['configuration'];
    onChange: (settings: Partial<SettingsState['configuration']>) => void;
}

function getThemeI18nLabel(t: ReturnType<typeof useTranslation>['t'], theme: { id: string; label: string }) {
    return t(`settings.configurationSection.themes.${theme.id}.label`, theme.label);
}

const ThemeGrid: React.FC<{
    value: string;
    onChange: (themeId: string) => void;
}> = ({ value, onChange }) => {
    const { t } = useTranslation();
    const selected = normalizeThemeId(value);

    return (
        <div className="grid grid-cols-2 gap-2">
            {availableThemes.map((theme) => {
                const isSelected = theme.id === selected;
                return (
                    <button
                        key={theme.id}
                        type="button"
                        onClick={() => onChange(theme.id)}
                        className={`rounded-[calc(var(--panel-radius)+2px)] border p-3 text-left transition-[border-color,background-color,box-shadow] duration-200 focus:outline-none ${
                            isSelected
                                ? 'border-(--accent-ai) bg-[color-mix(in_srgb,var(--accent-ai)_10%,var(--bg-surface))] shadow-(--shadow-sm)'
                                : 'border-(--border-default) bg-(--bg-surface) hover:border-(--border-focus) hover:bg-(--bg-surface-hover)'
                        }`}
                    >
                        <div className="flex items-center justify-between gap-2 mb-2">
                            <span className="text-[12px] font-semibold text-(--fg-primary) truncate">{getThemeI18nLabel(t, theme)}</span>
                            {isSelected && <Check className="h-3.5 w-3.5 shrink-0 text-(--accent-ai)" />}
                        </div>
                        <div className="flex gap-1">
                            <div className="h-3 flex-1 rounded-sm border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-app'] }} />
                            <div className="h-3 flex-1 rounded-sm border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-panel'] }} />
                            <div className="h-3 flex-1 rounded-sm border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-surface'] }} />
                            <div className="h-3 w-6 shrink-0 rounded-sm border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--accent-primary'] }} />
                        </div>
                    </button>
                );
            })}
        </div>
    );
};

const ConfigurationSettings: React.FC<ConfigurationSettingsProps> = ({ settings, onChange }) => {
    const { t } = useTranslation();

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.configurationSection.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.configurationSection.description')}
                </p>
            </div>

            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+4px)] p-5 space-y-5 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--panel-shadow)">
                <div>
                    <div className="text-sm font-medium text-(--fg-primary)">{t('settings.configurationSection.themeTitle')}</div>
                    <div className="text-xs text-(--fg-tertiary) mt-1">
                        {t('settings.configurationSection.themeDescription')}
                    </div>
                </div>

                <ThemeGrid
                    value={normalizeThemeId(settings.theme)}
                    onChange={(theme) => onChange({ theme: normalizeThemeId(theme) })}
                />

                <div className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                    <span className="h-2 w-2 rounded-full bg-(--accent-ai)" />
                    <span>{t('settings.configurationSection.themeScopeHelp')}</span>
                </div>
            </div>

            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+4px)] p-5 space-y-5 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--panel-shadow)">
                <div>
                    <div className="text-sm font-medium text-(--fg-primary)">{t('settings.configurationSection.languageTitle')}</div>
                    <div className="text-xs text-(--fg-tertiary) mt-1">
                        {t('settings.configurationSection.languageDescription')}
                    </div>
                </div>

                <div className="space-y-2.5">
                    <label className="text-xs font-medium uppercase tracking-[0.16em] text-(--fg-secondary) block">
                        {t('settings.configurationSection.interfaceLanguage')}
                    </label>

                    <div className="relative">
                        <select
                            value={settings.language}
                            onChange={(e) => onChange({ language: e.target.value as AppLanguage })}
                            className="w-full appearance-none rounded-[calc(var(--panel-radius)+2px)] border border-(--border-default) bg-(--bg-surface) px-3 py-2 pr-9 text-sm text-(--fg-primary) transition-[border-color,background-color] duration-200 focus:border-(--accent-ai) focus:outline-none hover:bg-(--bg-surface-hover) cursor-pointer"
                        >
                            {supportedAppLanguages.map((lang) => (
                                <option key={lang} value={lang}>{t(languageI18nKey[lang])}</option>
                            ))}
                        </select>
                        <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3 text-(--fg-secondary)">
                            <ChevronDown className="h-4 w-4" />
                        </div>
                    </div>
                </div>

                <div className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                    <span className="h-2 w-2 rounded-full bg-(--accent-ai)" />
                    <span>{t('settings.configurationSection.languageSaveHint')}</span>
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
    const { t } = useTranslation();
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
            await invoke('test_local_ollama_connection', { ollamaUrl: settings.ollamaUrl });
            setOllamaTestResult('success');
            setOllamaTestMessage(t('settings.connectionSuccessful'));
        } catch (e) {
            setOllamaTestResult('error');
            setOllamaTestMessage(formatUnknownBackendError(e));
        } finally {
            setIsTestingOllama(false);
        }
    };

    const handleRefreshOllamaModels = async () => {
        setIsRefreshingOllama(true);
        try {
            await invoke('refresh_local_ollama_models');
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
            await invoke('test_local_openai_compat_connection', { serverUrl: settings.openaiCompatUrl });
            setOpenaiTestResult('success');
            setOpenaiTestMessage(t('settings.connectionSuccessful'));
        } catch (e) {
            setOpenaiTestResult('error');
            setOpenaiTestMessage(formatUnknownBackendError(e));
        } finally {
            setIsTestingOpenAI(false);
        }
    };

    const handleRefreshOpenAIModels = async () => {
        setIsRefreshingOpenAI(true);
        try {
            await invoke('refresh_local_openai_compat_models');
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
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.localAi.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.localAi.description')}
                </p>
            </div>

            {/* Ollama Section */}
            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 space-y-4 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--shadow-sm)">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.ollama.title')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.ollama.description')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.ollamaEnabled}
                        onChange={(checked) => onChange({ ollamaEnabled: checked })}
                    />
                </div>

                <div className="space-y-2">
                    <label className="text-xs text-(--fg-secondary) block">{t('settings.serverUrl')}</label>
                    <input
                        type="text"
                        value={settings.ollamaUrl}
                        onChange={(e) => onChange({ ollamaUrl: e.target.value })}
                        placeholder={t('settings.ollama.urlPlaceholder')}
                        disabled={!settings.ollamaEnabled}
                        className="w-full rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-default) py-2 px-3 text-sm text-(--fg-primary) focus:outline-none focus:border-(--accent-ai) placeholder-(--fg-tertiary) disabled:opacity-60"
                    />
                </div>

                <div className="flex items-center gap-3">
                    <button
                        type="button"
                        onClick={handleTestOllamaConnection}
                        disabled={!settings.ollamaEnabled || isTestingOllama}
                        className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-xs font-medium border border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isTestingOllama ? t('settings.testing') : t('settings.testConnection')}
                    </button>
                    <button
                        type="button"
                        onClick={handleRefreshOllamaModels}
                        disabled={!settings.ollamaEnabled || isRefreshingOllama}
                        className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-xs font-medium border border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isRefreshingOllama ? t('settings.refreshing') : t('settings.refreshModels')}
                    </button>
                    {ollamaTestMessage && (
                        <span
                            className={`text-xs ${ollamaTestResult === 'success'
                                ? 'text-(--accent-mention)'
                                : 'text-(--state-danger)'
                                }`}
                        >
                            {ollamaTestMessage}
                        </span>
                    )}
                </div>
            </div>

            {/* Ollama Cloud Section */}
            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 space-y-4 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--shadow-sm)">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.ollamaCloud.title')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.ollamaCloud.description')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.ollamaCloudEnabled}
                        onChange={(checked) => onChange({ ollamaCloudEnabled: checked })}
                    />
                </div>

                <div className="space-y-2">
                    <label className="text-xs text-(--fg-secondary) block">{t('settings.apiKey')}</label>
                    <input
                        type="password"
                        value={settings.ollamaCloudApiKey}
                        onChange={(e) => onChange({ ollamaCloudApiKey: e.target.value })}
                        placeholder={t('settings.apiKeyPlaceholder')}
                        disabled={!settings.ollamaCloudEnabled}
                        className="w-full rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-default) py-2 px-3 text-sm text-(--fg-primary) focus:outline-none focus:border-(--accent-ai) placeholder-(--fg-tertiary) disabled:opacity-60"
                    />
                    <p className="text-xs text-(--fg-tertiary) mt-1">
                        {t('settings.ollamaCloud.apiKeyHelp')}
                    </p>
                </div>
            </div>

            {/* OpenAI-compatible Section */}
            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 space-y-4 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--shadow-sm)">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.openaiCompat.title')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.openaiCompat.description')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.openaiCompatEnabled}
                        onChange={(checked) => onChange({ openaiCompatEnabled: checked })}
                    />
                </div>

                <div className="space-y-2">
                    <label className="text-xs text-(--fg-secondary) block">{t('settings.serverUrl')}</label>
                    <input
                        type="text"
                        value={settings.openaiCompatUrl}
                        onChange={(e) => onChange({ openaiCompatUrl: e.target.value })}
                        placeholder={t('settings.openaiCompat.urlPlaceholder')}
                        disabled={!settings.openaiCompatEnabled}
                        className="w-full rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-default) py-2 px-3 text-sm text-(--fg-primary) focus:outline-none focus:border-(--accent-ai) placeholder-(--fg-tertiary) disabled:opacity-60"
                    />
                    <p className="text-xs text-(--fg-tertiary) mt-1">
                        {t('settings.openaiCompat.apiKeyHelp')}
                    </p>
                </div>

                <div className="flex items-center gap-3">
                    <button
                        type="button"
                        onClick={handleTestOpenAIConnection}
                        disabled={!settings.openaiCompatEnabled || isTestingOpenAI}
                        className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-xs font-medium border border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isTestingOpenAI ? t('settings.testing') : t('settings.testConnection')}
                    </button>
                    <button
                        type="button"
                        onClick={handleRefreshOpenAIModels}
                        disabled={!settings.openaiCompatEnabled || isRefreshingOpenAI}
                        className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-xs font-medium border border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isRefreshingOpenAI ? t('settings.refreshing') : t('settings.refreshModels')}
                    </button>
                    {openaiTestMessage && (
                        <span
                            className={`text-xs ${openaiTestResult === 'success'
                                ? 'text-(--accent-mention)'
                                : 'text-(--state-danger)'
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
    const { t } = useTranslation();
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.storage.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.storage.description')}
                </p>

                <div className="grid grid-cols-2 gap-4">
                    {/* Local Storage Option */}
                    <button
                        onClick={() => onChange({ mode: 'local' })}
                        className={`relative rounded-[calc(var(--panel-radius)+2px)] p-4 border text-left transition-all ${settings.mode === 'local'
                            ? 'border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-active)_78%,transparent)] shadow-(--shadow-sm)'
                            : 'border-(--border-default) hover:border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-panel)_86%,var(--bg-editor))]'
                            }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`rounded-[calc(var(--panel-radius)*0.55)] p-2 ${settings.mode === 'local' ? 'bg-[color-mix(in_srgb,var(--accent-ai)_22%,transparent)]' : 'bg-(--bg-surface)'}`}>
                                <HardDrive className={`w-5 h-5 ${settings.mode === 'local' ? 'text-(--accent-ai)' : 'text-(--fg-secondary)'}`} />
                            </div>
                            <div>
                                <div className="font-medium text-(--fg-primary)">{t('settings.storage.local')}</div>
                                <div className="text-xs text-(--fg-tertiary)">{t('settings.storage.localSubtitle')}</div>
                            </div>
                        </div>
                        <ul className="text-xs text-(--fg-secondary) space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.localBullet1')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.localBullet2')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.localBullet3')}
                            </li>
                        </ul>
                        {settings.mode === 'local' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-(--accent-ai)" />
                        )}
                    </button>

                    {/* Server Storage Option */}
                    <button
                        onClick={() => onChange({ mode: 'server' })}
                        className={`relative rounded-[calc(var(--panel-radius)+2px)] p-4 border text-left transition-all ${settings.mode === 'server'
                            ? 'border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-active)_78%,transparent)] shadow-(--shadow-sm)'
                            : 'border-(--border-default) hover:border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-panel)_86%,var(--bg-editor))]'
                            }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`rounded-[calc(var(--panel-radius)*0.55)] p-2 ${settings.mode === 'server' ? 'bg-[color-mix(in_srgb,var(--accent-ai)_22%,transparent)]' : 'bg-(--bg-surface)'}`}>
                                <Server className={`w-5 h-5 ${settings.mode === 'server' ? 'text-(--accent-ai)' : 'text-(--fg-secondary)'}`} />
                            </div>
                            <div>
                                <div className="font-medium text-(--fg-primary)">{t('settings.storage.server')}</div>
                                <div className="text-xs text-(--fg-tertiary)">{t('settings.storage.serverSubtitle')}</div>
                            </div>
                        </div>
                        <ul className="text-xs text-(--fg-secondary) space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.serverBullet1')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.serverBullet2')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" />
                                {t('settings.storage.serverBullet3')}
                            </li>
                        </ul>
                        {settings.mode === 'server' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-(--accent-ai)" />
                        )}
                    </button>
                </div>

                {/* Info box */}
                <div className="mt-4 p-3 bg-(--bg-surface) border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] flex gap-3">
                    <Info className="w-4 h-4 text-(--fg-tertiary) shrink-0 mt-0.5" />
                    <p className="text-xs text-(--fg-tertiary)">
                        {settings.mode === 'local'
                            ? t('settings.storage.localInfo')
                            : t('settings.storage.serverInfo')}
                    </p>
                </div>
            </div>

            {/* Sync Metadata Toggle (only for local mode) */}
            {settings.mode === 'local' && (
                <div className="flex items-center justify-between py-3 border-t border-(--border-subtle)">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.storage.syncMetadata')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.storage.syncMetadataDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.syncMetadata}
                        onChange={(checked) => onChange({ syncMetadata: checked })}
                    />
                </div>
            )}

            {/* Cache Settings */}
            <div className="border-t border-(--border-subtle) pt-4">
                <div className="flex items-center justify-between mb-3">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.storage.enableCache')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.storage.cacheDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.cache.enabled}
                        onChange={(checked) => onChange({ cache: { ...settings.cache, enabled: checked } })}
                    />
                </div>

                {settings.cache.enabled && (
                    <div className="mt-3">
                        <label className="text-xs text-(--fg-secondary) mb-1 block">
                            {t('settings.storage.maxCacheSize', { size: settings.cache.maxSizeMb })}
                        </label>
                        <input
                            type="range"
                            min="10"
                            max="500"
                            step="10"
                            value={settings.cache.maxSizeMb}
                            onChange={(e) => onChange({ cache: { ...settings.cache, maxSizeMb: parseInt(e.target.value) } })}
                            className="w-full h-1.5 bg-(--bg-app) rounded-full appearance-none cursor-pointer accent-(--accent-ai)"
                        />
                        <div className="flex justify-between text-[10px] text-(--fg-tertiary) mt-1">
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
    const { t } = useTranslation();
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.context.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.context.description')}
                </p>
            </div>

            {/* Max Tokens */}
            <div>
                <label className="text-sm font-medium text-(--fg-primary) mb-2 block">
                    {t('settings.context.maxContextTokens', { count: settings.maxTokens })}
                </label>
                <input
                    type="range"
                    min="2000"
                    max="32000"
                    step="1000"
                    value={settings.maxTokens}
                    onChange={(e) => onChange({ maxTokens: parseInt(e.target.value) })}
                    className="w-full h-1.5 bg-(--bg-app) rounded-full appearance-none cursor-pointer accent-(--accent-ai)"
                />
                <div className="flex justify-between text-[10px] text-(--fg-tertiary) mt-1">
                    <span>2K</span>
                    <span>32K</span>
                </div>
                <p className="text-xs text-(--fg-tertiary) mt-2">
                    {t('settings.context.tokenHelp')}
                </p>
            </div>

            {/* Compression */}
            <div className="border-t border-(--border-subtle) pt-4">
                <div className="flex items-center justify-between mb-3">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.context.enableCompression')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.context.compressionDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={settings.compression.enabled}
                        onChange={(checked) => onChange({ compression: { ...settings.compression, enabled: checked } })}
                    />
                </div>

                {settings.compression.enabled && (
                    <div className="mt-4 space-y-2">
                        <label className="text-xs text-(--fg-secondary) block">{t('settings.context.compressionModel')}</label>
                        <div className="flex gap-3">
                            <button
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'remote' } })}
                                className={`flex-1 px-3 py-2 rounded-[calc(var(--panel-radius)*0.65)] text-sm transition-colors ${settings.compression.model === 'remote'
                                    ? 'bg-(--accent-ai) text-(--fg-bright)'
                                    : 'bg-(--bg-surface) text-(--fg-secondary) hover:bg-(--bg-surface-hover)'
                                    }`}
                            >
                                <Cloud className="w-4 h-4 inline-block mr-2" />
                                {t('settings.context.compressionRemote')}
                            </button>
                            <button
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'local' } })}
                                className={`flex-1 px-3 py-2 rounded-[calc(var(--panel-radius)*0.65)] text-sm transition-colors ${settings.compression.model === 'local'
                                    ? 'bg-(--accent-ai) text-(--fg-bright)'
                                    : 'bg-(--bg-surface) text-(--fg-secondary) hover:bg-(--bg-surface-hover)'
                                    }`}
                            >
                                <HardDrive className="w-4 h-4 inline-block mr-2" />
                                {t('settings.context.compressionLocal')}
                            </button>
                        </div>
                        <p className="text-xs text-(--fg-tertiary)">
                            {settings.compression.model === 'remote'
                                ? t('settings.context.compressionRemoteHelp')
                                : t('settings.context.compressionLocalHelp')}
                        </p>
                    </div>
                )}
            </div>

            {/* Gitignore Files */}
            <div className="border-t border-(--border-subtle) pt-4">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.context.allowGitignored')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.context.allowGitignoredDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={allowGitIgnoredFiles}
                        onChange={onAllowGitIgnoredFilesChange}
                    />
                </div>
                <p className="text-xs text-(--fg-tertiary) mt-2">
                    {allowGitIgnoredFiles
                        ? t('settings.context.gitignoredEnabledHelp')
                        : t('settings.context.gitignoredDisabledHelp')}
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
    const { t } = useTranslation();
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.privacy.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.privacy.description')}
                </p>
            </div>

            {/* Telemetry */}
            <div className="flex items-center justify-between py-3">
                <div>
                    <div className="text-sm font-medium text-(--fg-primary)">{t('settings.privacy.usageTelemetry')}</div>
                    <div className="text-xs text-(--fg-tertiary)">
                        {t('settings.privacy.noTelemetry')}
                    </div>
                </div>
            </div>

            <div className="p-3 bg-(--bg-app) border border-(--border-subtle) rounded-[calc(var(--panel-radius)*0.75)]">
                <div className="flex gap-3">
                    <Shield className="w-4 h-4 text-(--accent-mention) shrink-0 mt-0.5" />
                    <div className="text-xs text-(--fg-tertiary)">
                        <p className="font-medium text-(--fg-secondary) mb-1">{t('settings.privacy.codeNeverShared')}</p>
                        <p>
                            {t('settings.privacy.telemetryDetail')}
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
    const { t } = useTranslation();
    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.editorSection.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.editorSection.description')}
                </p>
            </div>
            {/* No settings for now */}
        </div>
    );
};

const AboutSettings: React.FC = () => {
    const { t } = useTranslation();
    const [version, setVersion] = useState('dev');

    useEffect(() => {
        let mounted = true;

        void (async () => {
            try {
                const appVersion = await getVersion();
                if (mounted) {
                    setVersion(appVersion);
                }
            } catch {
                // Keep fallback for non-Tauri/test environments.
            }
        })();

        return () => {
            mounted = false;
        };
    }, []);

    return (
        <div className="space-y-6">
            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-5 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--shadow-sm)">
                <div className="flex items-center gap-4">
                    <img
                        src={zbladeLogoUrl}
                        alt={t('settings.aboutSection.appName')}
                        className="w-16 h-16 object-contain"
                        draggable={false}
                    />
                    <div>
                        <div className="text-lg font-semibold text-(--fg-primary)">{t('settings.aboutSection.appName')}</div>
                        <div className="text-sm text-(--fg-secondary)">{t('settings.aboutSection.appTagline')}</div>
                        <div className="text-xs text-(--fg-tertiary) mt-1">{t('settings.aboutSection.version', { version })}</div>
                    </div>
                </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] p-3 bg-(--bg-surface)">
                    <div className="text-[10px] uppercase tracking-wider text-(--fg-tertiary) mb-1">{t('settings.aboutSection.runtime')}</div>
                    <div className="text-sm text-(--fg-primary)">{t('settings.aboutSection.runtimeValue')}</div>
                </div>
                <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] p-3 bg-(--bg-surface)">
                    <div className="text-[10px] uppercase tracking-wider text-(--fg-tertiary) mb-1">{t('settings.aboutSection.engine')}</div>
                    <div className="text-sm text-(--fg-primary)">{t('settings.aboutSection.engineValue')}</div>
                </div>
                <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] p-3 bg-(--bg-surface)">
                    <div className="text-[10px] uppercase tracking-wider text-(--fg-tertiary) mb-1">{t('settings.aboutSection.mode')}</div>
                    <div className="text-sm text-(--fg-primary)">{t('settings.aboutSection.modeValue')}</div>
                </div>
            </div>

            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 bg-(--bg-surface) space-y-3">
                <div className="text-sm font-medium text-(--fg-primary)">{t('settings.aboutSection.tidbits')}</div>
                <ul className="space-y-2 text-sm text-(--fg-secondary)">
                    <li className="flex items-start gap-2">
                        <span className="w-1.5 h-1.5 mt-2 rounded-full bg-(--accent-ai)" />
                        <span>
                            {t('settings.aboutSection.website')}:{' '}
                            <a href="https://zblade.dev/" target="_blank" rel="noreferrer" className="text-(--accent-ai) hover:brightness-110">
                                zblade.dev
                            </a>
                        </span>
                    </li>
                    <li className="flex items-start gap-2">
                        <span className="w-1.5 h-1.5 mt-2 rounded-full bg-(--accent-ai)" />
                        <span>
                            {t('settings.aboutSection.github')}:{' '}
                            <a href="https://github.com/ZaguanLabs/ZaguanBlade" target="_blank" rel="noreferrer" className="text-(--accent-ai) hover:brightness-110">
                                ZaguanLabs/ZaguanBlade
                            </a>
                        </span>
                    </li>
                    <li className="flex items-start gap-2">
                        <span className="w-1.5 h-1.5 mt-2 rounded-full bg-(--accent-ai)" />
                        <span>
                            {t('settings.aboutSection.support')}:{' '}
                            <a href="https://github.com/ZaguanLabs/ZaguanBlade/issues" target="_blank" rel="noreferrer" className="text-(--accent-ai) hover:brightness-110">
                                {t('settings.aboutSection.githubIssues')}
                            </a>
                        </span>
                    </li>
                </ul>
                <p className="text-xs text-(--fg-tertiary) pt-1 border-t border-(--border-subtle)">
                    {t('settings.aboutSection.prsWelcome')}
                </p>
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
            className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus-visible:ring-2 focus-visible:ring-(--accent-ai) focus-visible:ring-offset-2 ${checked ? 'bg-(--accent-ai)' : 'bg-(--bg-app)'
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
    const { t } = useTranslation();
    const [showKey, setShowKey] = useState(false);

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.account.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.account.description')}
                </p>
            </div>

            <div className={`border rounded-[calc(var(--panel-radius)+2px)] p-4 mb-6 ${settings.apiKey ? 'bg-[color-mix(in_srgb,var(--accent-ai)_10%,transparent)] border-(--border-focus)' : 'bg-(--bg-surface) border-(--border-default)'}`}>
                <div className="flex gap-4">
                    <div className={`p-3 h-fit rounded-[calc(var(--panel-radius)*0.65)] ${settings.apiKey ? 'bg-[color-mix(in_srgb,var(--accent-ai)_22%,transparent)]' : 'bg-(--bg-editor)'}`}>
                        {settings.apiKey ? (
                            <CheckCircle2 className="w-6 h-6 text-(--accent-ai)" />
                        ) : (
                            <Key className="w-6 h-6 text-(--accent-ai)" />
                        )}
                    </div>
                    <div className="flex-1">
                        <h4 className="font-medium text-(--fg-primary) mb-1">
                            {settings.apiKey ? t('settings.account.activeSubscription') : t('settings.account.zaguanPro')}
                        </h4>
                        <p className="text-sm text-(--fg-secondary) mb-3">
                            {settings.apiKey
                                ? t('settings.account.subscriptionActive')
                                : t('settings.account.subscriptionNeeded')}
                        </p>
                        <a
                            href={settings.apiKey ? "https://zaguanai.com/dashboard" : "https://zaguanai.com/pricing"}
                            target="_blank"
                            rel="noreferrer"
                            className="text-sm text-(--accent-ai) hover:brightness-110 font-medium"
                        >
                            {settings.apiKey ? t('settings.account.manageSubscription') : t('settings.account.getSubscription')}
                        </a>
                    </div>
                </div>
            </div>

            <div className="space-y-2">
                <label className="text-sm font-medium text-(--fg-primary) block">
                    {t('settings.apiKey')}
                </label>
                <div className="flex gap-2">
                    <div className="relative flex-1">
                        <input
                            type={showKey ? 'text' : 'password'}
                            value={settings.apiKey}
                            onChange={(e) => onChange({ apiKey: e.target.value })}
                            placeholder={t('settings.apiKeyPlaceholder')}
                            className="w-full rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-default) py-2 pl-3 pr-10 text-sm text-(--fg-primary) focus:outline-none focus:border-(--accent-ai) placeholder-(--fg-tertiary)"
                        />
                        <button
                            type="button"
                            onClick={() => setShowKey(!showKey)}
                            className="absolute right-3 top-2 text-(--fg-tertiary) hover:text-(--fg-secondary)"
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
