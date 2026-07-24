import React, { Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import type { ModelInfo } from '../types/chat';
import type { SettingsSection } from './SettingsModal';

const SettingsModal = React.lazy(() =>
    import('./SettingsModal').then(module => ({ default: module.SettingsModal }))
);

interface SettingsModalLazyProps {
    isOpen: boolean;
    onClose: () => void;
    initialSection?: SettingsSection;
    workspacePath?: string | null;
    onRefreshModels?: () => Promise<ModelInfo[]>;
}

export function SettingsModalLazy({ isOpen, onClose, initialSection, workspacePath, onRefreshModels }: SettingsModalLazyProps) {
    const { t } = useTranslation();
    if (!isOpen) return null;
    return (
        <Suspense fallback={
            <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
                <div className="text-(--fg-tertiary) text-sm">{t('common.loading', 'Loading...')}</div>
            </div>
        }>
            <SettingsModal
                isOpen={isOpen}
                onClose={onClose}
                initialSection={initialSection}
                workspacePath={workspacePath}
                onRefreshModels={onRefreshModels}
            />
        </Suspense>
    );
}
