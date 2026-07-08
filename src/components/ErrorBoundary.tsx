import React, { Component, ErrorInfo, ReactNode } from 'react';
import { RefreshCw, Trash2, Copy, AlertTriangle } from 'lucide-react';
import i18n from '../i18n';

interface Props {
    children?: ReactNode;
    fallback?: (error: Error, reset: () => void) => ReactNode;
    onReset?: () => void;
}

interface State {
    hasError: boolean;
    error: Error | null;
    errorInfo: ErrorInfo | null;
    resetKey: number;
}

export class ErrorBoundary extends Component<Props, State> {
    public state: State = {
        hasError: false,
        error: null,
        errorInfo: null,
        resetKey: 0,
    };

    public static getDerivedStateFromError(error: Error): Partial<State> {
        return { hasError: true, error };
    }

    public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
        console.error('[ErrorBoundary] Uncaught error:', error, errorInfo);
        this.setState({ errorInfo });
    }

    private handleReset = () => {
        console.debug('[ErrorBoundary] Resetting UI...');
        this.props.onReset?.();
        this.setState({
            hasError: false,
            error: null,
            errorInfo: null,
            resetKey: this.state.resetKey + 1,
        });
    };

    private handleHardReset = () => {
        console.debug('[ErrorBoundary] Hard reset - clearing sessionStorage...');
        try {
            sessionStorage.clear();
        } catch (e) {
            console.warn('[ErrorBoundary] Failed to clear sessionStorage:', e);
        }
        this.handleReset();
    };

    private handleFullReload = () => {
        console.debug('[ErrorBoundary] Full page reload...');
        window.location.reload();
    };

    private handleCopyError = async () => {
        const { error, errorInfo } = this.state;
        const errorText = [
            `Error: ${error?.toString()}`,
            '',
            'Stack:',
            error?.stack || '(no stack)',
            '',
            'Component Stack:',
            errorInfo?.componentStack || '(no component stack)',
        ].join('\n');

        try {
            await navigator.clipboard.writeText(errorText);
        } catch (e) {
            console.warn('[ErrorBoundary] Failed to copy to clipboard:', e);
        }
    };

    public render() {
        if (this.state.hasError) {
            if (this.props.fallback) {
                return this.props.fallback(this.state.error!, this.handleReset);
            }

            return (
                <div className="flex items-center justify-center h-full w-full bg-(--bg-app) p-4">
                    <div className="max-w-lg w-full bg-(--bg-panel) border border-(--border-subtle) rounded-(--panel-radius) shadow-(--shadow-xl) overflow-hidden">
                        <div className="flex items-center gap-3 px-4 py-3 bg-[color-mix(in_srgb,var(--state-danger)_12%,transparent)] border-b border-(--border-subtle)">
                            <AlertTriangle className="w-5 h-5 text-(--state-danger)" aria-hidden="true" />
                            <h2 className="text-sm font-semibold text-(--fg-primary)">
                                {i18n.t('errorBoundary.title', { defaultValue: 'Something went wrong' })}
                            </h2>
                        </div>

                        <div className="p-4 space-y-4">
                            <p className="text-xs text-(--fg-secondary)">
                                {i18n.t('errorBoundary.description', { defaultValue: 'The UI encountered an error. You can try recovering without restarting the app.' })}
                            </p>

                            <div className="flex flex-wrap gap-2">
                                <button
                                    type="button"
                                    onClick={this.handleReset}
                                    className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium rounded-[calc(var(--panel-radius)*0.45)] bg-(--accent-ai) text-(--fg-bright) hover:opacity-90 transition-opacity"
                                >
                                    <RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />
                                    {i18n.t('errorBoundary.reloadUi', { defaultValue: 'Reload UI' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={this.handleHardReset}
                                    className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium rounded-[calc(var(--panel-radius)*0.45)] bg-(--bg-surface) text-(--fg-primary) border border-(--border-subtle) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
                                    {i18n.t('errorBoundary.clearCacheReload', { defaultValue: 'Clear Cache & Reload' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={this.handleFullReload}
                                    className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium rounded-[calc(var(--panel-radius)*0.45)] bg-(--bg-surface) text-(--fg-primary) border border-(--border-subtle) hover:bg-(--bg-surface-hover) transition-colors"
                                >
                                    <RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />
                                    {i18n.t('errorBoundary.fullReload', { defaultValue: 'Full Reload' })}
                                </button>
                            </div>

                            <details className="group">
                                <summary className="text-xs text-(--fg-tertiary) cursor-pointer hover:text-(--fg-secondary) select-none">
                                    {i18n.t('errorBoundary.showDetails', { defaultValue: 'Show error details' })}
                                </summary>
                                <div className="mt-2 p-2 bg-(--bg-app) rounded-[calc(var(--panel-radius)*0.4)] border border-(--border-subtle) overflow-auto max-h-48">
                                    <div className="flex justify-end mb-1">
                                        <button
                                            type="button"
                                            onClick={this.handleCopyError}
                                            className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-(--fg-tertiary) hover:text-(--fg-secondary) transition-colors"
                                        >
                                            <Copy className="w-3 h-3" aria-hidden="true" />
                                            {i18n.t('common.copy', { defaultValue: 'Copy' })}
                                        </button>
                                    </div>
                                    <pre className="text-[10px] font-mono text-(--state-danger) whitespace-pre-wrap break-all">
                                        {this.state.error?.toString()}
                                    </pre>
                                    {this.state.error?.stack && (
                                        <pre className="mt-2 text-[10px] font-mono text-(--fg-tertiary) whitespace-pre-wrap break-all">
                                            {this.state.error.stack}
                                        </pre>
                                    )}
                                </div>
                            </details>
                        </div>
                    </div>
                </div>
            );
        }

        return (
            <React.Fragment key={this.state.resetKey}>
                {this.props.children}
            </React.Fragment>
        );
    }
}
