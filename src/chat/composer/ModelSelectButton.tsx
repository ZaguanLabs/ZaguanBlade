import React from 'react';
import { CompactModelSelector } from '../../components/CompactModelSelector';
import type { ModelInfo } from '../../types/chat';

export const ModelSelectButton: React.FC<{
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    disabled?: boolean;
}> = ({ models, selectedModelId, setSelectedModelId, disabled }) => (
    <CompactModelSelector
        models={models}
        selectedId={selectedModelId || ''}
        onSelect={setSelectedModelId}
        disabled={disabled}
    />
);
