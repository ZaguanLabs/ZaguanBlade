'use client';
import React from 'react';
import { Search, FileText, Scale, Sparkles, CheckCircle2, Loader2 } from 'lucide-react';
import { ProgressInfo } from '../types/chat';

interface ProgressIndicatorProps {
    progress: ProgressInfo;
}

export const ProgressIndicator: React.FC<ProgressIndicatorProps> = ({ progress }) => {
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
        <div className={`border-l-2 rounded-r-lg pl-3 py-2.5 my-2 transition-all duration-300 ${getStageColor()}`}>
            <div className="flex items-center gap-2 mb-1.5">
                <div className="animate-pulse">
                    {getStageIcon()}
                </div>
                <span className="font-mono text-xs text-zinc-200 uppercase tracking-wider font-semibold">
                    {progress.stage}
                </span>
                <div className="ml-auto flex items-center gap-2">
                    <span className="text-xs text-zinc-400 font-mono">
                        {progress.percent}%
                    </span>
                </div>
            </div>
            
            <div className="text-sm text-zinc-300 font-medium mb-2">
                {progress.message}
            </div>
            
            <div className="relative h-1.5 bg-zinc-900/50 rounded-full overflow-hidden">
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
