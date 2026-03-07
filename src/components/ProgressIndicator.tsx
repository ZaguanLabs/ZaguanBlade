'use client';
import React from 'react';
import { Search, FileText, Scale, Sparkles, CheckCircle2, Loader2 } from 'lucide-react';
import { ProgressInfo } from '../types/chat';

interface ProgressIndicatorProps {
    progress: ProgressInfo;
}

export const ProgressIndicator: React.FC<ProgressIndicatorProps> = ({ progress }) => {
    const getPrettyStageLabel = () => {
        const rawStage = progress.stage || '';
        const stage = rawStage.toLowerCase();

        const prettyStageMap: Record<string, string> = {
            considering_next_steps: 'Planning Next Step',
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

    const getStageIcon = () => {
        const stage = progress.stage.toLowerCase();
        
        if (stage.includes('search') || stage.includes('query')) {
            return <Search className="w-4 h-4 text-blue-400" />;
        }
        if (stage.includes('extract') || stage.includes('fetch')) {
            return <FileText className="w-4 h-4 text-purple-400" />;
        }
        if (stage.includes('grad') || stage.includes('analyz')) {
            return <Scale className="w-4 h-4 text-yellow-400" />;
        }
        if (stage.includes('synth') || stage.includes('generat')) {
            return <Sparkles className="w-4 h-4 text-emerald-400" />;
        }
        if (stage.includes('done') || stage.includes('complete')) {
            return <CheckCircle2 className="w-4 h-4 text-emerald-400" />;
        }
        
        return <Loader2 className="w-4 h-4 text-blue-400 animate-spin" />;
    };

    const getStageColor = () => {
        const stage = progress.stage.toLowerCase();
        
        if (stage.includes('search') || stage.includes('query')) {
            return 'border-blue-500/50 bg-blue-950/20';
        }
        if (stage.includes('extract') || stage.includes('fetch')) {
            return 'border-purple-500/50 bg-purple-950/20';
        }
        if (stage.includes('grad') || stage.includes('analyz')) {
            return 'border-yellow-500/50 bg-yellow-950/20';
        }
        if (stage.includes('synth') || stage.includes('generat')) {
            return 'border-emerald-500/50 bg-emerald-950/20';
        }
        if (stage.includes('done') || stage.includes('complete')) {
            return 'border-emerald-500/50 bg-emerald-950/20';
        }
        
        return 'border-blue-500/50 bg-blue-950/20';
    };

    const getProgressBarColor = () => {
        const stage = progress.stage.toLowerCase();
        
        if (stage.includes('search') || stage.includes('query')) {
            return 'from-blue-500 to-blue-400';
        }
        if (stage.includes('extract') || stage.includes('fetch')) {
            return 'from-purple-500 to-purple-400';
        }
        if (stage.includes('grad') || stage.includes('analyz')) {
            return 'from-yellow-500 to-yellow-400';
        }
        if (stage.includes('synth') || stage.includes('generat')) {
            return 'from-emerald-500 to-emerald-400';
        }
        
        return 'from-blue-500 to-blue-400';
    };

    return (
        <div className={`my-3 overflow-hidden rounded-2xl border px-4 py-3 transition-all duration-300 shadow-[0_16px_40px_rgba(0,0,0,0.18)] ${getStageColor()}`}>
            <div className="mb-2 flex items-center gap-3">
                <div className="flex h-8 w-8 items-center justify-center rounded-2xl border border-white/5 bg-black/10 animate-pulse">
                    {getStageIcon()}
                </div>
                <div className="min-w-0 flex-1">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-zinc-400">
                        {getPrettyStageLabel()}
                    </div>
                    <div className="truncate text-sm font-semibold text-zinc-100">
                        {progress.message}
                    </div>
                </div>
                <div className="ml-auto flex items-center gap-2 rounded-full border border-white/5 bg-black/10 px-2.5 py-1">
                    <span className="font-mono text-xs text-zinc-300">
                        {progress.percent}%
                    </span>
                </div>
            </div>

            <div className="relative h-2 rounded-full bg-zinc-950/45 overflow-hidden">
                <div 
                    className={`absolute inset-y-0 left-0 bg-gradient-to-r ${getProgressBarColor()} transition-all duration-500 ease-out rounded-full`}
                    style={{ width: `${progress.percent}%` }}
                >
                    <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/20 to-transparent animate-shimmer" />
                </div>
            </div>
        </div>
    );
};
