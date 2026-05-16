'use client';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { Search, FileText, Scale, Sparkles, CheckCircle2, Loader2 } from 'lucide-react';
import { ProgressInfo } from '../types/chat';

interface ProgressIndicatorProps {
    progress: ProgressInfo;
}

export const ProgressIndicator: React.FC<ProgressIndicatorProps> = ({ progress }) => {
    const { t } = useTranslation();
    const getPrettyStageLabel = () => {
        const rawStage = progress.stage || '';
        const stage = rawStage.toLowerCase();

        const prettyStageMap: Record<string, string> = {
            considering_next_steps: t('progress.planningNextStep'),
        };

        if (prettyStageMap[stage]) {
            return prettyStageMap[stage];
        }

        if (rawStage.includes('_')) {
            return rawStage
                .toLowerCase()
                .split('_')
                .filter(Boolean)
                .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
                .join(' ');
        }

        return rawStage;
    };

    const getStageTone = () => {
        const stage = progress.stage.toLowerCase();

        if (stage.includes('search') || stage.includes('query')) {
            return 'var(--accent-ai)';
        }
        if (stage.includes('extract') || stage.includes('fetch')) {
            return 'var(--accent-planning)';
        }
        if (stage.includes('grad') || stage.includes('analyz')) {
            return 'var(--accent-warning)';
        }
        if (stage.includes('synth') || stage.includes('generat')) {
            return 'var(--accent-mention)';
        }
        if (stage.includes('done') || stage.includes('complete')) {
            return 'var(--accent-mention)';
        }

        return 'var(--accent-ai)';
    };

    const getStageIcon = () => {
        const stage = progress.stage.toLowerCase();
        const color = getStageTone();

        if (stage.includes('search') || stage.includes('query')) {
            return <Search className="w-4 h-4" style={{ color }} />;
        }
        if (stage.includes('extract') || stage.includes('fetch')) {
            return <FileText className="w-4 h-4" style={{ color }} />;
        }
        if (stage.includes('grad') || stage.includes('analyz')) {
            return <Scale className="w-4 h-4" style={{ color }} />;
        }
        if (stage.includes('synth') || stage.includes('generat')) {
            return <Sparkles className="w-4 h-4" style={{ color }} />;
        }
        if (stage.includes('done') || stage.includes('complete')) {
            return <CheckCircle2 className="w-4 h-4" style={{ color }} />;
        }

        return <Loader2 className="w-4 h-4 animate-spin" style={{ color }} />;
    };

    const stageTone = getStageTone();

    return (
        <div
            className="my-3 overflow-hidden rounded-[calc(var(--panel-radius)*1.2)] border px-4 py-3 transition-all duration-(--transition-slow)"
            style={{
                borderColor: `color-mix(in srgb, ${stageTone} 38%, var(--border-default))`,
                backgroundColor: `color-mix(in srgb, ${stageTone} 10%, var(--bg-surface))`,
                boxShadow: 'var(--shadow-md)',
            }}
        >
            <div className="mb-2 flex items-center gap-3">
                <div
                    className="flex h-8 w-8 items-center justify-center rounded-[calc(var(--panel-radius)*0.8)] border animate-pulse"
                    style={{
                        borderColor: 'var(--border-subtle)',
                        backgroundColor: 'color-mix(in srgb, var(--bg-surface) 82%, transparent)',
                    }}
                >
                    {getStageIcon()}
                </div>
                <div className="min-w-0 flex-1">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-tertiary)">
                        {getPrettyStageLabel()}
                    </div>
                    <div className="truncate text-sm font-semibold text-(--fg-primary)">
                        {progress.message}
                    </div>
                </div>
                <div
                    className="ml-auto flex items-center gap-2 rounded-full border px-2.5 py-1"
                    style={{
                        borderColor: 'var(--border-subtle)',
                        backgroundColor: 'color-mix(in srgb, var(--bg-surface) 82%, transparent)',
                    }}
                >
                    <span className="font-mono text-xs text-(--fg-secondary)">
                        {progress.percent}%
                    </span>
                </div>
            </div>

            <div
                className="relative h-2 overflow-hidden rounded-full"
                style={{ backgroundColor: 'color-mix(in srgb, var(--fg-tertiary) 18%, transparent)' }}
            >
                <div
                    className="absolute inset-y-0 left-0 rounded-full transition-all duration-500 ease-out"
                    style={{ width: `${progress.percent}%` }}
                >
                    <div
                        className="absolute inset-0"
                        style={{
                            background: `linear-gradient(90deg, ${stageTone} 0%, color-mix(in srgb, ${stageTone} 76%, var(--fg-bright)) 100%)`,
                        }}
                    />
                    <div
                        className="absolute inset-0 animate-shimmer"
                        style={{
                            background: 'linear-gradient(90deg, transparent 0%, color-mix(in srgb, var(--fg-bright) 32%, transparent) 50%, transparent 100%)',
                        }}
                    />
                </div>
            </div>
        </div>
    );
};
