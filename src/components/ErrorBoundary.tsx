
import React, { Component, ErrorInfo, ReactNode } from 'react';

interface Props {
    children?: ReactNode;
    fallback?: (error: Error) => ReactNode;
}

interface State {
    hasError: boolean;
    error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
    public state: State = {
        hasError: false,
        error: null
    };

    public static getDerivedStateFromError(error: Error): State {
        return { hasError: true, error };
    }

    public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
        console.error('Uncaught error:', error, errorInfo);
    }

    public render() {
        if (this.state.hasError) {
            if (this.props.fallback) {
                return this.props.fallback(this.state.error!);
            }
            return (
                <div className="p-4 text-red-500 overflow-auto h-full text-xs font-mono bg-zinc-950">
                    <h2 className="font-bold mb-2">Something went wrong.</h2>
                    <pre>{this.state.error?.toString()}</pre>
                    <pre className="mt-2 text-zinc-500">{this.state.error?.stack}</pre>
                </div>
            );
        }

        return this.props.children;
    }
}
