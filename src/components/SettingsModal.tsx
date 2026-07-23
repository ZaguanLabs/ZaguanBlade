'use client';
import React, { useId, useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { X, Database, Cloud, Shield, Zap, HardDrive, Server, ChevronRight, ChevronDown, Info, Loader2, Code, Key, CheckCircle2, Palette, Check, Smartphone, Eye, EyeOff } from 'lucide-react';
import type { BackendSettings, LocalAiConfig, RemoteAiConfig } from '../types/settings';
import i18n, { normalizeAppLanguage, supportedAppLanguages, languageI18nKey, type AppLanguage } from '../i18n';
import { QRCodeSVG } from 'qrcode.react';
import zbladeLogoUrl from '../assets/zblade-in-app-logo.png';
import { availableThemes, normalizeThemeId } from '../themes';
import { formatUnknownBackendError } from '../utils/backendErrors';
import { ScrollArea } from './ui/ScrollArea';
import { ConfirmModal } from './ui/Modal';
import { DEFAULT_CHAT_FONT_SIZE, DEFAULT_EDITOR_FONT_SIZE, MAX_CONTENT_FONT_SIZE, MIN_CONTENT_FONT_SIZE } from '../contexts/DisplaySettingsContext';

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
    skills: BackendSettings['skills'];
    configuration: {
        theme: string;
        markdownView: string;
        language: AppLanguage;
        editorFontSize: number;
        chatFontSize: number;
    };
    account: {
        apiKey: string;
        userId: string;
        email: string;
        tier: string;
    };
    localAi: {
        ollamaEnabled: boolean;
        ollamaUrl: string;
        ollamaCloudEnabled: boolean;
        ollamaCloudApiKey: string;
        openaiCompatEnabled: boolean;
        openaiCompatUrl: string;
        hiddenModels: string[];
    };
    allowGitIgnoredFiles?: boolean;  // Per-project setting
    autoApproveRunCommands?: boolean;
    warmupContextPrefetch?: boolean;  // Per-project setting
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
    skills: {
        config: [],
    },
    configuration: {
        theme: 'zaguan-dark',
        markdownView: 'split',
        language: normalizeAppLanguage(i18n.resolvedLanguage || i18n.language),
        editorFontSize: DEFAULT_EDITOR_FONT_SIZE,
        chatFontSize: DEFAULT_CHAT_FONT_SIZE,
    },
    account: {
        apiKey: '',
        userId: '',
        email: '',
        tier: '',
    },
    localAi: {
        ollamaEnabled: false,
        ollamaUrl: 'http://localhost:11434',
        ollamaCloudEnabled: false,
        ollamaCloudApiKey: '',
        openaiCompatEnabled: false,
        openaiCompatUrl: 'http://localhost:8080',
        hiddenModels: [],
    },
    allowGitIgnoredFiles: false,  // Default: respect .gitignore
    autoApproveRunCommands: false,
    warmupContextPrefetch: true,  // Default: enabled (opt-out)
};







function backendRemoteToFrontend(backend: RemoteAiConfig): Pick<SettingsState, 'account' | 'configuration'> {
    return {
        configuration: {
            theme: normalizeThemeId(backend.theme),
            markdownView: backend.markdown_view || 'split',
            language: normalizeAppLanguage(backend.language || i18n.resolvedLanguage || i18n.language),
            editorFontSize: Math.max(MIN_CONTENT_FONT_SIZE, Math.min(MAX_CONTENT_FONT_SIZE, Math.round(backend.editor_font_size || DEFAULT_EDITOR_FONT_SIZE))),
            chatFontSize: Math.max(MIN_CONTENT_FONT_SIZE, Math.min(MAX_CONTENT_FONT_SIZE, Math.round(backend.chat_font_size || DEFAULT_CHAT_FONT_SIZE))),
        },
        account: {
            apiKey: backend.api_key,
            userId: backend.user_id,
            email: backend.user_email,
            tier: backend.tier,
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
            hiddenModels: backend.hidden_local_models ?? [],
        },
    };
}

function frontendRemoteToBackend(frontend: SettingsState): RemoteAiConfig {
    return {
        api_key: frontend.account.apiKey,
        user_id: frontend.account.userId,
        user_email: frontend.account.email,
        tier: frontend.account.tier,
        theme: normalizeThemeId(frontend.configuration.theme),
        markdown_view: frontend.configuration.markdownView,
        language: normalizeAppLanguage(frontend.configuration.language),
        editor_font_size: frontend.configuration.editorFontSize,
        chat_font_size: frontend.configuration.chatFontSize,
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
        hidden_local_models: frontend.localAi.hiddenModels,
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
        skills: backend.skills ?? { config: [] },
        allowGitIgnoredFiles: backend.allow_gitignored_files,
        autoApproveRunCommands: backend.auto_approve_run_commands,
        warmupContextPrefetch: backend.warmup_context_prefetch ?? true,
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
        skills: frontend.skills,
        allow_gitignored_files: frontend.allowGitIgnoredFiles || false,
        auto_approve_run_commands: frontend.autoApproveRunCommands || false,
        warmup_context_prefetch: frontend.warmupContextPrefetch ?? true,
    };
}

interface SettingsModalProps {
    isOpen: boolean;
    onClose: () => void;
    initialSection?: SettingsSection;
    workspacePath?: string | null;
    onRefreshModels?: () => Promise<import('../types/chat').ModelInfo[]>;
}

type SettingsSection = 'configuration' | 'account' | 'localai' | 'storage' | 'context' | 'privacy' | 'editor' | 'remote' | 'about';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, initialSection, workspacePath, onRefreshModels }) => {
    const { t } = useTranslation();
    const titleId = useId();
    const subtitleId = useId();
    const [settings, setSettings] = useState<SettingsState>(defaultSettings);
    const [loadedSettings, setLoadedSettings] = useState<SettingsState>(defaultSettings);
    const [activeSection, setActiveSection] = useState<SettingsSection>('configuration');
    const [hasChanges, setHasChanges] = useState(false);
    const [isLoading, setIsLoading] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showDiscardConfirm, setShowDiscardConfirm] = useState(false);

    // Intercept close requests: if there are unsaved changes, confirm before discarding.
    const requestClose = useCallback(() => {
        if (hasChanges) {
            setShowDiscardConfirm(true);
        } else {
            onClose();
        }
    }, [hasChanges, onClose]);

    useEffect(() => {
        if (!isOpen) return;

        const handleKeyDown = (e: KeyboardEvent) => {
            // While the discard confirm is open, let it own Escape (it cancels itself).
            if (e.key === 'Escape' && !showDiscardConfirm) {
                requestClose();
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, showDiscardConfirm, requestClose]);

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
                setShowDiscardConfirm(false);
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

    const syncSavedRemoteAccount = useCallback((remoteSettings: RemoteAiConfig) => {
        const { account } = backendRemoteToFrontend(remoteSettings);
        setSettings(prev => ({ ...prev, account }));
        setLoadedSettings(prev => ({ ...prev, account }));
    }, []);

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
                    || remoteSettings.user_email !== previousRemoteSettings.user_email
                    || remoteSettings.tier !== previousRemoteSettings.tier;
                const remoteConfigurationChanged =
                    remoteSettings.markdown_view !== previousRemoteSettings.markdown_view
                    || remoteSettings.editor_font_size !== previousRemoteSettings.editor_font_size
                    || remoteSettings.chat_font_size !== previousRemoteSettings.chat_font_size
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
        { id: 'configuration', label: t('settings.navigation.configuration'), icon: <Palette className="w-4 h-4" aria-hidden="true" /> },
        { id: 'account', label: t('settings.navigation.account'), icon: <Key className="w-4 h-4" aria-hidden="true" /> },
        { id: 'localai', label: t('settings.navigation.localAi'), icon: <Server className="w-4 h-4" aria-hidden="true" /> },
        { id: 'storage', label: t('settings.navigation.storage'), icon: <Database className="w-4 h-4" aria-hidden="true" /> },
        ...(workspacePath ? [
            { id: 'context', label: t('settings.navigation.context'), icon: <Zap className="w-4 h-4" aria-hidden="true" /> },
            // { id: 'privacy', label: 'Privacy', icon: <Shield className="w-4 h-4" /> },
        ] as const : []),
        { id: 'remote', label: t('settings.navigation.remote'), icon: <Smartphone className="w-4 h-4" aria-hidden="true" /> },
        { id: 'about', label: t('settings.navigation.about'), icon: <Info className="w-4 h-4" aria-hidden="true" /> },
    ];

    return (
        <div className="fixed inset-0 z-9999 flex items-center justify-center p-6">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/72"
                aria-hidden="true"
                onClick={requestClose}
            />

            {/* Modal */}
            <div
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={subtitleId}
                className="relative w-full max-w-[980px] h-[min(760px,92vh)] flex flex-col animate-in fade-in zoom-in-95 duration-150 rounded-(--panel-radius) border border-(--border-default) bg-(--bg-panel) shadow-(--shadow-xl) overflow-hidden"
            >
                {/* Header */}
                <div className="flex items-start justify-between px-6 py-4 border-b border-(--border-default) bg-[color-mix(in_srgb,var(--bg-panel)_82%,var(--bg-surface))]">
                    <div>
                        <h2 id={titleId} className="text-lg font-semibold text-(--fg-primary)">{t('settings.title')}</h2>
                        <p id={subtitleId} className="text-xs text-(--fg-tertiary) mt-0.5">{t('settings.subtitle')}</p>
                    </div>
                    <button
                        type="button"
                        onClick={requestClose}
                        aria-label={t('common.close')}
                        className="rounded-[calc(var(--panel-radius)*0.35)] p-1.5 text-(--fg-tertiary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                    >
                        <X className="w-5 h-5" aria-hidden="true" />
                    </button>
                </div>

                {/* Content */}
                <div className="flex flex-1 overflow-hidden">
                    {/* Sidebar */}
                    <div className="w-56 border-r border-(--border-default) py-3 px-2 bg-[color-mix(in_srgb,var(--bg-app)_82%,var(--bg-panel))]">
                        {sections.map(section => (
                            <button
                                type="button"
                                key={section.id}
                                onClick={() => setActiveSection(section.id)}
                                aria-current={activeSection === section.id ? 'page' : undefined}
                                className={`w-full flex items-center justify-between gap-3 rounded-[calc(var(--panel-radius)*0.65)] px-3 py-2.5 text-sm transition-colors border ${activeSection === section.id
                                    ? 'bg-(--bg-active) text-(--fg-primary) border-(--border-focus)'
                                    : 'text-(--fg-secondary) border-transparent hover:bg-(--bg-surface) hover:text-(--fg-primary)'
                                    }`}
                            >
                                <span className="flex items-center gap-2.5">
                                    {section.icon}
                                    {section.label}
                                </span>
                                {activeSection === section.id && <ChevronRight className="w-3.5 h-3.5 text-(--accent-ai)" aria-hidden="true" />}
                            </button>
                        ))}
                    </div>

                    {/* Main Content */}
                    <ScrollArea className="flex-1 p-6 bg-(--bg-editor)">
                        {isLoading ? (
                            <div role="status" aria-live="polite" aria-label={t('common.loading')} className="flex items-center justify-center h-full">
                                <Loader2 aria-hidden="true" className="w-6 h-6 text-(--fg-tertiary) animate-spin" />
                            </div>
                        ) : (
                            <>
                                {error && (
                                    <div role="alert" className="mb-4 rounded-[calc(var(--panel-radius)*0.75)] border border-[color-mix(in_srgb,var(--state-danger)_32%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] p-3 text-sm text-(--state-danger)">
                                        {error}
                                    </div>
                                )}
                                {activeSection === 'configuration' && (
                                    <ConfigurationSettings
                                        settings={settings.configuration}
                                        autoApproveRunCommands={settings.autoApproveRunCommands || false}
                                        onChange={(updates) => updateSettings('configuration', updates)}
                                        onAutoApproveRunCommandsChange={(value) => {
                                            setSettings(prev => ({ ...prev, autoApproveRunCommands: value }));
                                            setHasChanges(true);
                                        }}
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
                                        warmupContextPrefetch={settings.warmupContextPrefetch ?? true}
                                        onWarmupContextPrefetchChange={(value) => {
                                            setSettings(prev => ({ ...prev, warmupContextPrefetch: value }));
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
                                        onConnected={syncSavedRemoteAccount}
                                    />
                                )}
                                {activeSection === 'editor' && (
                                    <EditorSettings
                                        settings={settings.editor}
                                        onChange={(updates) => updateSettings('editor', updates)}
                                    />
                                )}
                                {activeSection === 'remote' && <RemoteSettings />}
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
                            type="button"
                            onClick={requestClose}
                            className="rounded-[calc(var(--panel-radius)*0.65)] px-4 py-2 text-sm font-medium text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                        >
                            {hasChanges ? t('common.cancel') : t('common.close')}
                        </button>
                        <button
                            type="button"
                            onClick={handleSave}
                            disabled={!hasChanges || isSaving}
                            className="flex items-center gap-2 rounded-[calc(var(--panel-radius)*0.65)] bg-(--accent-ai) px-4 py-2 text-sm font-medium text-(--fg-bright) transition-colors hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                            {isSaving && <Loader2 aria-hidden="true" className="w-4 h-4 animate-spin" />}
                            {isSaving ? t('statusBar.saving') : t('settings.saveChanges')}
                        </button>
                    </div>
                </div>
            </div>

            <ConfirmModal
                isOpen={showDiscardConfirm}
                title={t('settings.discardChanges.title')}
                message={t('settings.discardChanges.message')}
                confirmLabel={t('settings.discardChanges.confirm')}
                cancelLabel={t('settings.discardChanges.keepEditing')}
                confirmVariant="danger"
                onConfirm={() => { setShowDiscardConfirm(false); onClose(); }}
                onCancel={() => setShowDiscardConfirm(false)}
            />
        </div>
    );
};

interface ConfigurationSettingsProps {
    settings: SettingsState['configuration'];
    autoApproveRunCommands: boolean;
    onChange: (settings: Partial<SettingsState['configuration']>) => void;
    onAutoApproveRunCommandsChange: (value: boolean) => void;
}

function getThemeI18nLabel(t: ReturnType<typeof useTranslation>['t'], theme: { id: string; label: string }) {
    return t(`settings.configurationSection.themes.${theme.id}.label`, theme.label);
}

function clampContentFontSize(value: number, fallback: number) {
    if (!Number.isFinite(value)) {
        return fallback;
    }
    return Math.max(MIN_CONTENT_FONT_SIZE, Math.min(MAX_CONTENT_FONT_SIZE, Math.round(value)));
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
                        aria-pressed={isSelected}
                        className={`rounded-[calc(var(--panel-radius)+2px)] border p-3 text-left transition-[border-color,background-color,box-shadow] duration-200 focus:outline-none ${
                            isSelected
                                ? 'border-(--accent-ai) bg-[color-mix(in_srgb,var(--accent-ai)_10%,var(--bg-surface))] shadow-(--shadow-sm)'
                                : 'border-(--border-default) bg-(--bg-surface) hover:border-(--border-focus) hover:bg-(--bg-surface-hover)'
                        }`}
                    >
                        <div className="flex items-center justify-between gap-2 mb-2">
                            <span className="text-[12px] font-semibold text-(--fg-primary) truncate">{getThemeI18nLabel(t, theme)}</span>
                            {isSelected && <Check className="h-3.5 w-3.5 shrink-0 text-(--accent-ai)" aria-hidden="true" />}
                        </div>
                        <div className="flex gap-1">
                            <div className="h-3 flex-1 rounded-[calc(var(--panel-radius)*0.25)] border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-app'] }} />
                            <div className="h-3 flex-1 rounded-[calc(var(--panel-radius)*0.25)] border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-panel'] }} />
                            <div className="h-3 flex-1 rounded-[calc(var(--panel-radius)*0.25)] border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--bg-surface'] }} />
                            <div className="h-3 w-6 shrink-0 rounded-[calc(var(--panel-radius)*0.25)] border border-(--border-subtle)" style={{ backgroundColor: theme.tokens['--accent-ai'] }} />
                        </div>
                    </button>
                );
            })}
        </div>
    );
};

const ConfigurationSettings: React.FC<ConfigurationSettingsProps> = ({
    settings,
    autoApproveRunCommands,
    onChange,
    onAutoApproveRunCommandsChange,
}) => {
    const { t } = useTranslation();
    const languageSelectId = useId();
    const editorFontSizeInputId = useId();
    const chatFontSizeInputId = useId();
    const handleFontSizeChange = (key: 'editorFontSize' | 'chatFontSize', value: number) => {
        const fallback = key === 'editorFontSize' ? DEFAULT_EDITOR_FONT_SIZE : DEFAULT_CHAT_FONT_SIZE;
        onChange({ [key]: clampContentFontSize(value, fallback) });
    };

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
                    <span className="h-2 w-2 rounded-full bg-(--accent-ai)" aria-hidden="true" />
                    <span>{t('settings.configurationSection.themeScopeHelp')}</span>
                </div>
            </div>

            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+4px)] p-5 space-y-5 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--panel-shadow)">
                <div>
                    <div className="text-sm font-medium text-(--fg-primary)">{t('settings.configurationSection.textSizeTitle')}</div>
                    <div className="text-xs text-(--fg-tertiary) mt-1">
                        {t('settings.configurationSection.textSizeDescription')}
                    </div>
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                    <div className="space-y-2.5">
                        <div className="flex items-center justify-between gap-3">
                            <label htmlFor={editorFontSizeInputId} className="text-xs font-medium uppercase tracking-[0.16em] text-(--fg-secondary)">
                                {t('settings.configurationSection.editorFontSize')}
                            </label>
                            <input
                                type="number"
                                min={MIN_CONTENT_FONT_SIZE}
                                max={MAX_CONTENT_FONT_SIZE}
                                value={settings.editorFontSize}
                                onChange={(event) => handleFontSizeChange('editorFontSize', Number(event.target.value))}
                                className="w-16 rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-default) bg-(--bg-surface) px-2 py-1 text-right text-xs font-mono text-(--fg-primary) outline-none focus:border-(--accent-ai)"
                            />
                        </div>
                        <input
                            id={editorFontSizeInputId}
                            type="range"
                            min={MIN_CONTENT_FONT_SIZE}
                            max={MAX_CONTENT_FONT_SIZE}
                            value={settings.editorFontSize}
                            onChange={(event) => handleFontSizeChange('editorFontSize', Number(event.target.value))}
                            className="w-full accent-[var(--accent-ai)]"
                        />
                    </div>

                    <div className="space-y-2.5">
                        <div className="flex items-center justify-between gap-3">
                            <label htmlFor={chatFontSizeInputId} className="text-xs font-medium uppercase tracking-[0.16em] text-(--fg-secondary)">
                                {t('settings.configurationSection.chatFontSize')}
                            </label>
                            <input
                                type="number"
                                min={MIN_CONTENT_FONT_SIZE}
                                max={MAX_CONTENT_FONT_SIZE}
                                value={settings.chatFontSize}
                                onChange={(event) => handleFontSizeChange('chatFontSize', Number(event.target.value))}
                                className="w-16 rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-default) bg-(--bg-surface) px-2 py-1 text-right text-xs font-mono text-(--fg-primary) outline-none focus:border-(--accent-ai)"
                            />
                        </div>
                        <input
                            id={chatFontSizeInputId}
                            type="range"
                            min={MIN_CONTENT_FONT_SIZE}
                            max={MAX_CONTENT_FONT_SIZE}
                            value={settings.chatFontSize}
                            onChange={(event) => handleFontSizeChange('chatFontSize', Number(event.target.value))}
                            className="w-full accent-[var(--accent-ai)]"
                        />
                    </div>
                </div>

                <div className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                    <span className="h-2 w-2 rounded-full bg-(--accent-ai)" aria-hidden="true" />
                    <span>{t('settings.configurationSection.textSizeScopeHelp')}</span>
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
                    <label htmlFor={languageSelectId} className="text-xs font-medium uppercase tracking-[0.16em] text-(--fg-secondary) block">
                        {t('settings.configurationSection.interfaceLanguage')}
                    </label>

                    <div className="relative">
                        <select
                            id={languageSelectId}
                            value={settings.language}
                            onChange={(e) => onChange({ language: e.target.value as AppLanguage })}
                            className="w-full appearance-none rounded-[calc(var(--panel-radius)+2px)] border border-(--border-default) bg-(--bg-surface) px-3 py-2 pr-9 text-sm text-(--fg-primary) transition-[border-color,background-color] duration-200 focus:border-(--accent-ai) focus:outline-none hover:bg-(--bg-surface-hover) cursor-pointer"
                        >
                            {supportedAppLanguages.map((lang) => (
                                <option key={lang} value={lang}>{t(languageI18nKey[lang])}</option>
                            ))}
                        </select>
                        <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3 text-(--fg-secondary)">
                            <ChevronDown className="h-4 w-4" aria-hidden="true" />
                        </div>
                    </div>
                </div>

                <div className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                    <span className="h-2 w-2 rounded-full bg-(--accent-ai)" aria-hidden="true" />
                    <span>{t('settings.configurationSection.languageSaveHint')}</span>
                </div>
            </div>

            <div className="border border-[color-mix(in_srgb,var(--accent-warning)_28%,var(--border-default))] rounded-[calc(var(--panel-radius)+4px)] p-5 space-y-4 bg-[color-mix(in_srgb,var(--accent-warning)_7%,var(--bg-panel))] shadow-(--panel-shadow)">
                <div className="flex items-start justify-between gap-4">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.configurationSection.yoloModeTitle')}</div>
                        <div className="text-xs text-(--fg-tertiary) mt-1">
                            {t('settings.configurationSection.yoloModeDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={autoApproveRunCommands}
                        onChange={onAutoApproveRunCommandsChange}
                    />
                </div>

                <div className="flex items-center gap-2 text-[11px] text-(--accent-warning)">
                    <span className="h-2 w-2 rounded-full bg-(--accent-warning)" aria-hidden="true" />
                    <span>{t('settings.configurationSection.yoloModeScopeHelp')}</span>
                </div>
            </div>
        </div>
    );
};

interface LocalModelPromptStatus {
    id: string;
    name: string;
    provider?: string;
    has_local_prompt: boolean;
    prompt_file: string | null;
    builtin_kind: string;
}

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
    const ollamaUrlId = useId();
    const ollamaCloudApiKeyId = useId();
    const openaiCompatUrlId = useId();

    const [modelStatuses, setModelStatuses] = useState<LocalModelPromptStatus[]>([]);
    const [isLoadingModels, setIsLoadingModels] = useState(false);

    const loadModelStatuses = useCallback(async () => {
        setIsLoadingModels(true);
        try {
            const statuses = await invoke<LocalModelPromptStatus[]>('list_local_models_with_prompt_status');
            setModelStatuses(statuses);
        } catch (e) {
            console.error('[Settings] Failed to load local model prompt status:', e);
        } finally {
            setIsLoadingModels(false);
        }
    }, []);

    useEffect(() => {
        void loadModelStatuses();
    }, [loadModelStatuses]);

    const toggleModelVisibility = (id: string, visible: boolean) => {
        const next = visible
            ? settings.hiddenModels.filter((m) => m !== id)
            : [...settings.hiddenModels, id];
        onChange({ hiddenModels: next });
    };

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
            await loadModelStatuses();
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
            await loadModelStatuses();
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
                    <label htmlFor={ollamaUrlId} className="text-xs text-(--fg-secondary) block">{t('settings.serverUrl')}</label>
                    <input
                        id={ollamaUrlId}
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
                    <label htmlFor={ollamaCloudApiKeyId} className="text-xs text-(--fg-secondary) block">{t('settings.apiKey')}</label>
                    <input
                        id={ollamaCloudApiKeyId}
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
                    <label htmlFor={openaiCompatUrlId} className="text-xs text-(--fg-secondary) block">{t('settings.serverUrl')}</label>
                    <input
                        id={openaiCompatUrlId}
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

            {/* Models to display in the model selector */}
            <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 space-y-3 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--shadow-sm)">
                <div className="flex items-center justify-between gap-3">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.localAiModels.title')}</div>
                        <div className="text-xs text-(--fg-tertiary)">{t('settings.localAiModels.description')}</div>
                    </div>
                    <button
                        type="button"
                        onClick={loadModelStatuses}
                        disabled={isLoadingModels}
                        className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-xs font-medium border border-(--border-default) text-(--fg-secondary) hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
                    >
                        {isLoadingModels ? t('settings.refreshing') : t('settings.localAiModels.refresh')}
                    </button>
                </div>

                {modelStatuses.length === 0 ? (
                    <p className="text-xs text-(--fg-tertiary)">
                        {isLoadingModels ? t('settings.localAiModels.loading') : t('settings.localAiModels.empty')}
                    </p>
                ) : (
                    <ul className="rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-default) overflow-hidden divide-y divide-(--border-default)">
                        {modelStatuses.map((model) => {
                            const isVisible = !settings.hiddenModels.includes(model.id);
                            const checkboxId = `model-visible-${model.id}`;
                            return (
                                <li key={model.id} className="flex items-center gap-3 px-3 py-2 bg-(--bg-surface)">
                                    <input
                                        type="checkbox"
                                        id={checkboxId}
                                        checked={isVisible}
                                        onChange={(e) => toggleModelVisibility(model.id, e.target.checked)}
                                        className="h-4 w-4 shrink-0 accent-(--accent-ai) cursor-pointer"
                                    />
                                    <label htmlFor={checkboxId} className="flex-1 min-w-0 cursor-pointer">
                                        <span className="block truncate text-sm text-(--fg-primary)">{model.name}</span>
                                    </label>
                                    {model.has_local_prompt ? (
                                        <span
                                            className="inline-flex items-center gap-1 text-xs text-(--accent-mention) shrink-0 max-w-[45%]"
                                            title={t('settings.localAiModels.customPromptTooltip', { file: model.prompt_file ?? '' })}
                                        >
                                            <CheckCircle2 className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
                                            <span className="truncate">{model.prompt_file}</span>
                                        </span>
                                    ) : (
                                        <span
                                            className="inline-flex items-center gap-1 text-xs text-(--fg-tertiary) shrink-0"
                                            title={t('settings.localAiModels.builtinPromptTooltip')}
                                        >
                                            <Info className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
                                            {t('settings.localAiModels.builtinPrompt')}
                                        </span>
                                    )}
                                </li>
                            );
                        })}
                    </ul>
                )}
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
    const cacheSizeInputId = useId();

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
                        type="button"
                        onClick={() => onChange({ mode: 'local' })}
                        aria-pressed={settings.mode === 'local'}
                        className={`relative rounded-[calc(var(--panel-radius)+2px)] p-4 border text-left transition-all ${settings.mode === 'local'
                            ? 'border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-active)_78%,transparent)] shadow-(--shadow-sm)'
                            : 'border-(--border-default) hover:border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-panel)_86%,var(--bg-editor))]'
                            }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`rounded-[calc(var(--panel-radius)*0.55)] p-2 ${settings.mode === 'local' ? 'bg-[color-mix(in_srgb,var(--accent-ai)_22%,transparent)]' : 'bg-(--bg-surface)'}`}>
                                <HardDrive className={`w-5 h-5 ${settings.mode === 'local' ? 'text-(--accent-ai)' : 'text-(--fg-secondary)'}`} aria-hidden="true" />
                            </div>
                            <div>
                                <div className="font-medium text-(--fg-primary)">{t('settings.storage.local')}</div>
                                <div className="text-xs text-(--fg-tertiary)">{t('settings.storage.localSubtitle')}</div>
                            </div>
                        </div>
                        <ul className="text-xs text-(--fg-secondary) space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.localBullet1')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.localBullet2')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.localBullet3')}
                            </li>
                        </ul>
                        {settings.mode === 'local' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-(--accent-ai)" aria-hidden="true" />
                        )}
                    </button>

                    {/* Server Storage Option */}
                    <button
                        type="button"
                        onClick={() => onChange({ mode: 'server' })}
                        aria-pressed={settings.mode === 'server'}
                        className={`relative rounded-[calc(var(--panel-radius)+2px)] p-4 border text-left transition-all ${settings.mode === 'server'
                            ? 'border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-active)_78%,transparent)] shadow-(--shadow-sm)'
                            : 'border-(--border-default) hover:border-(--border-focus) bg-[color-mix(in_srgb,var(--bg-panel)_86%,var(--bg-editor))]'
                            }`}
                    >
                        <div className="flex items-center gap-3 mb-3">
                            <div className={`rounded-[calc(var(--panel-radius)*0.55)] p-2 ${settings.mode === 'server' ? 'bg-[color-mix(in_srgb,var(--accent-ai)_22%,transparent)]' : 'bg-(--bg-surface)'}`}>
                                <Server className={`w-5 h-5 ${settings.mode === 'server' ? 'text-(--accent-ai)' : 'text-(--fg-secondary)'}`} aria-hidden="true" />
                            </div>
                            <div>
                                <div className="font-medium text-(--fg-primary)">{t('settings.storage.server')}</div>
                                <div className="text-xs text-(--fg-tertiary)">{t('settings.storage.serverSubtitle')}</div>
                            </div>
                        </div>
                        <ul className="text-xs text-(--fg-secondary) space-y-1">
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.serverBullet1')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.serverBullet2')}
                            </li>
                            <li className="flex items-center gap-1.5">
                                <ChevronRight className="w-3 h-3 text-(--accent-ai)" aria-hidden="true" />
                                {t('settings.storage.serverBullet3')}
                            </li>
                        </ul>
                        {settings.mode === 'server' && (
                            <div className="absolute top-2 right-2 w-2 h-2 rounded-full bg-(--accent-ai)" aria-hidden="true" />
                        )}
                    </button>
                </div>

                {/* Info box */}
                <div className="mt-4 p-3 bg-(--bg-surface) border border-(--border-default) rounded-[calc(var(--panel-radius)*0.75)] flex gap-3">
                    <Info className="w-4 h-4 text-(--fg-tertiary) shrink-0 mt-0.5" aria-hidden="true" />
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
                        <label htmlFor={cacheSizeInputId} className="text-xs text-(--fg-secondary) mb-1 block">
                            {t('settings.storage.maxCacheSize', { size: settings.cache.maxSizeMb })}
                        </label>
                        <input
                            id={cacheSizeInputId}
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
    warmupContextPrefetch: boolean;
    onWarmupContextPrefetchChange: (value: boolean) => void;
}

const ContextSettings: React.FC<ContextSettingsProps> = ({ settings, onChange, allowGitIgnoredFiles, onAllowGitIgnoredFilesChange, warmupContextPrefetch, onWarmupContextPrefetchChange }) => {
    const { t } = useTranslation();
    const maxTokensInputId = useId();
    const compressionModelLabelId = useId();

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
                <label htmlFor={maxTokensInputId} className="text-sm font-medium text-(--fg-primary) mb-2 block">
                    {t('settings.context.maxContextTokens', { count: settings.maxTokens })}
                </label>
                <input
                    id={maxTokensInputId}
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
                        <div id={compressionModelLabelId} className="text-xs text-(--fg-secondary)">{t('settings.context.compressionModel')}</div>
                        <div role="group" aria-labelledby={compressionModelLabelId} className="flex gap-3">
                            <button
                                type="button"
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'remote' } })}
                                aria-pressed={settings.compression.model === 'remote'}
                                className={`flex-1 px-3 py-2 rounded-[calc(var(--panel-radius)*0.65)] text-sm transition-colors ${settings.compression.model === 'remote'
                                    ? 'bg-(--accent-ai) text-(--fg-bright)'
                                    : 'bg-(--bg-surface) text-(--fg-secondary) hover:bg-(--bg-surface-hover)'
                                    }`}
                            >
                                <Cloud className="w-4 h-4 inline-block mr-2" aria-hidden="true" />
                                {t('settings.context.compressionRemote')}
                            </button>
                            <button
                                type="button"
                                onClick={() => onChange({ compression: { ...settings.compression, model: 'local' } })}
                                aria-pressed={settings.compression.model === 'local'}
                                className={`flex-1 px-3 py-2 rounded-[calc(var(--panel-radius)*0.65)] text-sm transition-colors ${settings.compression.model === 'local'
                                    ? 'bg-(--accent-ai) text-(--fg-bright)'
                                    : 'bg-(--bg-surface) text-(--fg-secondary) hover:bg-(--bg-surface-hover)'
                                    }`}
                            >
                                <HardDrive className="w-4 h-4 inline-block mr-2" aria-hidden="true" />
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

            {/* Warmup Context Prefetch */}
            <div className="border-t border-(--border-subtle) pt-4">
                <div className="flex items-center justify-between">
                    <div>
                        <div className="text-sm font-medium text-(--fg-primary)">{t('settings.context.warmupPrefetch')}</div>
                        <div className="text-xs text-(--fg-tertiary)">
                            {t('settings.context.warmupPrefetchDescription')}
                        </div>
                    </div>
                    <Toggle
                        checked={warmupContextPrefetch}
                        onChange={onWarmupContextPrefetchChange}
                    />
                </div>
                <p className="text-xs text-(--fg-tertiary) mt-2">
                    {warmupContextPrefetch
                        ? t('settings.context.warmupPrefetchEnabledHelp')
                        : t('settings.context.warmupPrefetchDisabledHelp')}
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
                    <Shield className="w-4 h-4 text-(--accent-mention) shrink-0 mt-0.5" aria-hidden="true" />
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
            type="button"
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

// ─── Remote Control Settings ─────────────────────────────────────────

interface RemoteControlStatusData {
    state: string;
    paired: boolean;
    telegram_chat_id: string | null;
    connected_at: number | null;
    bot_username: string | null;
}

const RemoteSettings: React.FC = () => {
    const { t } = useTranslation();
    const [status, setStatus] = useState<RemoteControlStatusData | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [tokenInput, setTokenInput] = useState('');
    const botTokenInputId = useId();

    // Load initial status
    useEffect(() => {
        void (async () => {
            try {
                const s = await invoke<RemoteControlStatusData>('get_remote_control_status');
                setStatus(s);
            } catch (e) {
                console.error('[Remote] Failed to load status:', e);
            }
        })();
    }, []);

    const handleConnectBot = async () => {
        if (!tokenInput.trim()) return;
        setIsLoading(true);
        setError(null);
        try {
            const s = await invoke<RemoteControlStatusData>('set_telegram_bot_token', { token: tokenInput });
            setStatus(s);
            setTokenInput('');
        } catch (e) {
            setError(String(e));
        } finally {
            setIsLoading(false);
        }
    };

    const handleDisconnect = async () => {
        setIsLoading(true);
        setError(null);
        try {
            const s = await invoke<RemoteControlStatusData>('disconnect_remote_control');
            setStatus(s);
        } catch (e) {
            setError(String(e));
        } finally {
            setIsLoading(false);
        }
    };

    const isDisconnected = !status || status.state === 'disconnected';
    const isConnected = status?.state === 'connected';

    const botUrl = status?.bot_username
        ? `https://t.me/${status.bot_username}`
        : null;

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-base font-semibold text-(--fg-primary) mb-1">{t('settings.remote.title')}</h3>
                <p className="text-sm text-(--fg-tertiary) mb-4">
                    {t('settings.remote.description')}
                </p>
            </div>

            {error && (
                <div className="rounded-[calc(var(--panel-radius)*0.75)] border border-[color-mix(in_srgb,var(--state-danger)_32%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] p-3 text-sm text-(--state-danger)">
                    {error}
                </div>
            )}

            {/* Disconnected State */}
            {isDisconnected && (
                <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+4px)] p-6 bg-[color-mix(in_srgb,var(--bg-panel)_88%,var(--bg-editor))] shadow-(--panel-shadow)">
                    <div className="flex flex-col gap-5">
                        <div className="flex items-center gap-4">
                            <div className="p-3 rounded-full bg-[color-mix(in_srgb,var(--accent-ai)_12%,transparent)] shrink-0">
                                <Smartphone className="w-8 h-8 text-(--accent-ai)" aria-hidden="true" />
                            </div>
                            <div>
                                <div className="text-sm font-medium text-(--fg-primary) mb-1">{t('settings.remote.createBotTitle')}</div>
                                <div className="text-xs text-(--fg-tertiary)">
                                    {t('settings.remote.createBotDescription')}
                                </div>
                            </div>
                        </div>

                        <div className="text-xs text-(--fg-secondary) space-y-2 bg-[color-mix(in_srgb,var(--bg-surface)_50%,transparent)] p-3 rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle)">
                            <p className="font-medium">{t('settings.remote.howToGetToken')}</p>
                            <ol className="list-decimal pl-4 space-y-1 text-(--fg-tertiary)">
                                <li>{t('settings.remote.step1')} <a href="https://t.me/BotFather" target="_blank" rel="noreferrer" className="text-(--accent-ai) hover:underline">@BotFather</a></li>
                                <li>{t('settings.remote.step2')} <code className="bg-(--bg-editor) px-1 rounded">/newbot</code> {t('settings.remote.step2Suffix')}</li>
                                <li>{t('settings.remote.step3')}</li>
                            </ol>
                        </div>

                        <div className="flex flex-col gap-2">
                            <label htmlFor={botTokenInputId} className="text-xs font-medium text-(--fg-secondary)">{t('settings.remote.botTokenLabel')}</label>
                            <div className="flex gap-3">
                                <input
                                    id={botTokenInputId}
                                    type="password"
                                    value={tokenInput}
                                    onChange={(e) => setTokenInput(e.target.value)}
                                    placeholder="1234567890:AAH..."
                                    className="flex-1 px-3 py-2 bg-(--bg-input) border border-(--border-input) rounded-[calc(var(--panel-radius)*0.75)] text-sm text-(--fg-primary) placeholder:text-(--fg-tertiary) focus:outline-none focus:border-(--border-focus) focus:ring-1 focus:ring-(--border-focus) transition-all"
                                />
                                <button
                                    type="button"
                                    onClick={handleConnectBot}
                                    disabled={isLoading || !tokenInput.trim()}
                                    className="shrink-0 flex items-center gap-2 rounded-[calc(var(--panel-radius)*0.65)] bg-(--accent-ai) px-5 py-2.5 text-sm font-medium text-(--fg-bright) transition-all hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
                                >
                                    {isLoading ? <Loader2 aria-hidden="true" className="w-4 h-4 animate-spin" /> : t('settings.remote.connect')}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            )}

            {/* Connected State */}
            {isConnected && (
                <div className="space-y-4">
                    {/* Connection Card */}
                    <div className="border border-[color-mix(in_srgb,var(--state-success)_30%,var(--border-default))] rounded-[calc(var(--panel-radius)+4px)] p-5 bg-[color-mix(in_srgb,var(--state-success)_8%,var(--bg-panel))] shadow-(--panel-shadow)">
                        <div className="flex items-start gap-4">
                            <div className="p-2.5 rounded-[calc(var(--panel-radius)*0.65)] bg-[color-mix(in_srgb,var(--state-success)_18%,transparent)]">
                                <CheckCircle2 className="w-6 h-6 text-[var(--state-success)]" aria-hidden="true" />
                            </div>
                            <div className="flex-1">
                                <div className="text-sm font-medium text-(--fg-primary)">
                                    {t('settings.remote.botConfigured')}
                                </div>
                                {status.bot_username && (
                                    <div className="text-xs text-(--fg-secondary) mt-0.5">
                                        {t('settings.remote.botUsername')} <span className="font-medium text-(--fg-primary)">@{status.bot_username}</span>
                                    </div>
                                )}
                                <div className="text-[10px] text-(--fg-tertiary) mt-1">
                                    {t('settings.remote.statusLabel')} {status.paired ? t('settings.remote.statusPaired') : t('settings.remote.statusWaiting')}
                                </div>
                            </div>
                            <button
                                type="button"
                                onClick={handleDisconnect}
                                disabled={isLoading}
                                className="rounded-[calc(var(--panel-radius)*0.65)] px-3 py-1.5 text-xs font-medium text-(--state-danger) border border-[color-mix(in_srgb,var(--state-danger)_30%,var(--border-default))] bg-[color-mix(in_srgb,var(--state-danger)_8%,transparent)] hover:bg-[color-mix(in_srgb,var(--state-danger)_16%,transparent)] transition-colors disabled:opacity-50"
                            >
                                {t('settings.remote.disconnect')}
                            </button>
                        </div>
                    </div>
                    
                    {/* QR Code to chat with bot */}
                    {botUrl && (
                        <div className="border border-(--border-focus) rounded-[calc(var(--panel-radius)+4px)] p-6 bg-[color-mix(in_srgb,var(--accent-ai)_5%,var(--bg-panel))] shadow-(--panel-shadow)">
                            <div className="flex flex-col items-center gap-5">
                                <div className="text-center">
                                    <div className="text-sm font-medium text-(--fg-primary) mb-1">{t('settings.remote.openChatTitle')}</div>
                                    <div className="text-xs text-(--fg-tertiary)">
                                        {t('settings.remote.qrDescription')} <code className="bg-(--bg-editor) px-1 rounded">/start</code> {t('settings.remote.qrDescriptionSuffix')}
                                    </div>
                                </div>

                                <div className="rounded-[calc(var(--panel-radius)+2px)] border-2 border-(--border-focus) p-3 bg-[#1a1a2e] shadow-(--shadow-lg)">
                                    <QRCodeSVG 
                                        value={botUrl} 
                                        size={180} 
                                        fgColor="#ffffff" 
                                        bgColor="#1a1a2e" 
                                        level="L"
                                    />
                                </div>

                                <a
                                    href={botUrl}
                                    target="_blank"
                                    rel="noreferrer"
                                    className="text-xs text-(--accent-ai) hover:brightness-110 font-mono break-all text-center max-w-[320px] underline underline-offset-2"
                                >
                                    {botUrl}
                                </a>
                            </div>
                        </div>
                    )}

                    {/* Feature Info */}
                    <div className="border border-(--border-default) rounded-[calc(var(--panel-radius)+2px)] p-4 bg-(--bg-surface) space-y-3">
                        <div className="text-xs font-medium uppercase tracking-[0.16em] text-(--fg-secondary)">
                            {t('settings.remote.featuresTitle')}
                        </div>
                        <div className="space-y-2">
                            {[
                                t('settings.remote.featureCommands'),
                                t('settings.remote.featureApprovals'),
                                t('settings.remote.featureOutput'),
                            ].map((item, i) => (
                                <div key={i} className="flex items-center gap-2 text-xs text-(--fg-secondary)">
                                    <span className="h-1.5 w-1.5 rounded-full bg-(--accent-ai) shrink-0" />
                                    {item}
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
            )}

            {/* Info footer */}
            <div className="flex items-center gap-2 text-[11px] text-(--fg-tertiary)">
                <span className="h-2 w-2 rounded-full bg-(--accent-ai)" />
                <span>{t('settings.remote.requiresRunning')}</span>
            </div>
        </div>
    );
};

interface AccountSettingsProps {
    settings: SettingsState['account'];
    onChange: (updates: Partial<SettingsState['account']>) => void;
    onConnected: (remoteSettings: RemoteAiConfig) => void;
}

type SsoLoginProgress = {
    status: string;
    user_code?: string;
    verification_uri?: string;
    message?: string;
    email?: string;
    tier?: string;
};

type SsoLoginResult = {
    user_id: string;
    email: string;
    tier?: string;
};

function getSsoStatusText(t: ReturnType<typeof useTranslation>['t'], progress: SsoLoginProgress | null): string | null {
    if (!progress) return null;
    if (progress.message) return progress.message;

    switch (progress.status) {
        case 'starting':
            return t('settings.account.ssoStatus.starting');
        case 'verification_required':
            return t('settings.account.ssoStatus.verificationRequired');
        case 'pending':
            return t('settings.account.ssoStatus.pending');
        case 'pending_subscription':
            return t('settings.account.ssoStatus.pendingSubscription');
        case 'rate_limited':
            return t('settings.account.ssoStatus.rateLimited');
        case 'approved':
            return t('settings.account.ssoStatus.approved');
        case 'cancelled':
            return t('settings.account.ssoStatus.cancelled');
        default:
            return null;
    }
}

const AccountSettings: React.FC<AccountSettingsProps> = ({ settings, onChange, onConnected }) => {
    const { t } = useTranslation();
    const [showKey, setShowKey] = useState(false);
    const [isSsoLoading, setIsSsoLoading] = useState(false);
    const [isSigningOut, setIsSigningOut] = useState(false);
    const [ssoProgress, setSsoProgress] = useState<SsoLoginProgress | null>(null);
    const [ssoError, setSsoError] = useState<string | null>(null);
    const apiKeyInputId = useId();
    const isConnected = settings.apiKey.trim().length > 0;
    const accountLabel = settings.email || settings.userId;
    const statusText = getSsoStatusText(t, ssoProgress);

    useEffect(() => {
        const unlistenPromise = listen<SsoLoginProgress>('sso-login-progress', (event) => {
            setSsoProgress(event.payload);
            if (event.payload.status === 'error') {
                setSsoError(event.payload.message || t('settings.account.ssoFailed'));
                setIsSsoLoading(false);
            }
            if (event.payload.status === 'approved' || event.payload.status === 'cancelled') {
                setIsSsoLoading(false);
            }
        });

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [t]);

    const startSsoLogin = async () => {
        if (isSsoLoading) return;
        setIsSsoLoading(true);
        setSsoError(null);
        setSsoProgress({ status: 'starting' });

        try {
            const result = await invoke<SsoLoginResult>('start_sso_login');
            const remoteSettings = await invoke<RemoteAiConfig>('get_remote_ai_settings');
            onConnected(remoteSettings);
            await emit('remote-settings-changed');
            setSsoProgress({
                status: 'approved',
                email: result.email,
                tier: result.tier || remoteSettings.tier,
            });
        } catch (error) {
            const message = formatUnknownBackendError(error);
            if (!message.toLowerCase().includes('cancelled')) {
                setSsoError(message);
            }
        } finally {
            setIsSsoLoading(false);
        }
    };

    const cancelSsoLogin = async () => {
        try {
            await invoke('cancel_sso_login');
        } catch (error) {
            setSsoError(formatUnknownBackendError(error));
        }
    };

    const signOut = async () => {
        if (isSigningOut) return;
        setIsSigningOut(true);
        setSsoError(null);
        try {
            await invoke('sign_out_zaguan');
            const remoteSettings = await invoke<RemoteAiConfig>('get_remote_ai_settings');
            onConnected(remoteSettings);
            await emit('remote-settings-changed');
            setSsoProgress(null);
        } catch (error) {
            setSsoError(formatUnknownBackendError(error));
        } finally {
            setIsSigningOut(false);
        }
    };

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
                            <CheckCircle2 className="w-6 h-6 text-(--accent-ai)" aria-hidden="true" />
                        ) : (
                            <Key className="w-6 h-6 text-(--accent-ai)" aria-hidden="true" />
                        )}
                    </div>
                    <div className="flex-1">
                        <h4 className="font-medium text-(--fg-primary) mb-1">
                            {isConnected ? t('settings.account.activeSubscription') : t('settings.account.zaguanPro')}
                        </h4>
                        <p className="text-sm text-(--fg-secondary) mb-3">
                            {isConnected
                                ? (accountLabel
                                    ? t('settings.account.connectedAs', { account: accountLabel })
                                    : t('settings.account.subscriptionActive'))
                                : t('settings.account.subscriptionNeeded')}
                        </p>
                        {isConnected && settings.tier && (
                            <div className="mb-3 inline-flex max-w-full items-center gap-2 rounded-[calc(var(--panel-radius)*0.5)] border border-(--border-default) bg-(--bg-editor) px-2 py-1 text-xs text-(--fg-secondary)">
                                <span className="shrink-0 text-(--fg-tertiary)">{t('settings.account.planLabel')}</span>
                                <span className="min-w-0 truncate font-medium text-(--fg-primary)">{settings.tier}</span>
                            </div>
                        )}
                        {isConnected ? (
                            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                                <a
                                    href="https://zaguanai.com/dashboard"
                                    target="_blank"
                                    rel="noreferrer"
                                    className="text-sm text-(--accent-ai) hover:brightness-110 font-medium"
                                >
                                    {t('settings.account.manageSubscription')}
                                </a>
                                <a
                                    href="https://zaguanai.com/dashboard/api-keys"
                                    target="_blank"
                                    rel="noreferrer"
                                    className="text-sm text-(--accent-ai) hover:brightness-110 font-medium"
                                >
                                    {t('settings.account.manageDevices')}
                                </a>
                                <a
                                    href="https://zaguanai.com/dashboard/usage"
                                    target="_blank"
                                    rel="noreferrer"
                                    className="text-sm text-(--accent-ai) hover:brightness-110 font-medium"
                                >
                                    {t('settings.account.usageCredits')}
                                </a>
                                <button
                                    type="button"
                                    onClick={signOut}
                                    disabled={isSigningOut}
                                    className="text-sm font-medium text-(--fg-secondary) transition-colors hover:text-(--fg-primary) disabled:cursor-not-allowed disabled:opacity-60"
                                >
                                    {isSigningOut ? t('settings.account.signingOut') : t('settings.account.signOut')}
                                </button>
                            </div>
                        ) : (
                            <a
                                href="https://zaguanai.com/pricing"
                                target="_blank"
                                rel="noreferrer"
                                className="text-sm text-(--accent-ai) hover:brightness-110 font-medium"
                            >
                                {t('settings.account.getSubscription')}
                            </a>
                        )}
                    </div>
                </div>
            </div>

            <div className="rounded-[calc(var(--panel-radius)+2px)] border border-(--border-default) bg-(--bg-surface) p-4">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                    <div className="min-w-0">
                        <h4 className="text-sm font-medium text-(--fg-primary)">{t('settings.account.ssoTitle')}</h4>
                        <p className="mt-1 text-xs leading-5 text-(--fg-tertiary)">
                            {t('settings.account.ssoDescription')}
                        </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                        {isSsoLoading && (
                            <button
                                type="button"
                                onClick={cancelSsoLogin}
                                className="rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-default) px-3 py-2 text-sm font-medium text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                            >
                                {t('settings.account.cancelSso')}
                            </button>
                        )}
                        <button
                            type="button"
                            onClick={startSsoLogin}
                            disabled={isSsoLoading}
                            className="flex items-center gap-2 rounded-[calc(var(--panel-radius)*0.65)] bg-(--accent-ai) px-4 py-2 text-sm font-medium text-(--fg-bright) transition-colors hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-60"
                        >
                            {isSsoLoading ? (
                                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                            ) : (
                                <Cloud className="h-4 w-4" aria-hidden="true" />
                            )}
                            {isConnected ? t('settings.account.reconnectSso') : t('settings.account.signInWithZaguan')}
                        </button>
                    </div>
                </div>

                {(statusText || ssoProgress?.user_code || ssoError) && (
                    <div className="mt-4 rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-default) bg-(--bg-editor) p-3" aria-live="polite">
                        {statusText && (
                            <div className="text-sm text-(--fg-secondary)">{statusText}</div>
                        )}
                        {ssoProgress?.user_code && (
                            <div className="mt-3 flex flex-wrap items-center gap-2">
                                <span className="text-xs font-medium uppercase tracking-[0.14em] text-(--fg-tertiary)">
                                    {t('settings.account.userCode')}
                                </span>
                                <span className="rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-default) bg-(--bg-surface) px-2 py-1 font-mono text-sm text-(--fg-primary)">
                                    {ssoProgress.user_code}
                                </span>
                            </div>
                        )}
                        {ssoProgress?.verification_uri && (
                            <a
                                href={ssoProgress.verification_uri}
                                target="_blank"
                                rel="noreferrer"
                                className="mt-3 inline-flex text-sm font-medium text-(--accent-ai) hover:brightness-110"
                            >
                                {t('settings.account.openVerification')}
                            </a>
                        )}
                        {ssoError && (
                            <div className="mt-3 rounded-[calc(var(--panel-radius)*0.55)] border border-[color-mix(in_srgb,var(--state-danger)_32%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] px-3 py-2 text-sm text-(--state-danger)">
                                {ssoError}
                            </div>
                        )}
                    </div>
                )}
            </div>

            <details className="rounded-[calc(var(--panel-radius)+2px)] border border-(--border-default) bg-(--bg-surface) p-4" open={!isConnected}>
                <summary className="cursor-pointer text-sm font-medium text-(--fg-primary)">
                    {t('settings.account.manualApiKey')}
                </summary>
                <p className="mt-2 text-xs leading-5 text-(--fg-tertiary)">
                    {t('settings.account.manualApiKeyHint')}
                </p>
                <div className="mt-3 space-y-2">
                    <label htmlFor={apiKeyInputId} className="text-sm font-medium text-(--fg-primary) block">
                        {t('settings.apiKey')}
                    </label>
                    <div className="flex gap-2">
                        <div className="relative flex-1">
                            <input
                                id={apiKeyInputId}
                                type={showKey ? 'text' : 'password'}
                                value={settings.apiKey}
                                onChange={(e) => onChange({
                                    apiKey: e.target.value,
                                    userId: '',
                                    email: '',
                                    tier: '',
                                })}
                                placeholder={t('settings.apiKeyPlaceholder')}
                                className="w-full rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-default) py-2 pl-3 pr-10 text-sm text-(--fg-primary) focus:outline-none focus:border-(--accent-ai) placeholder-(--fg-tertiary)"
                            />
                            <button
                                type="button"
                                aria-label={showKey ? t('settings.account.hideApiKey') : t('settings.account.showApiKey')}
                                aria-pressed={showKey}
                                onClick={() => setShowKey(!showKey)}
                                className="absolute right-3 top-2 text-(--fg-tertiary) hover:text-(--fg-secondary)"
                            >
                                {showKey ? (
                                    <EyeOff className="w-4 h-4" aria-hidden="true" />
                                ) : (
                                    <Eye className="w-4 h-4" aria-hidden="true" />
                                )}
                            </button>
                        </div>
                    </div>
                </div>
            </details>
        </div>
    );
};
