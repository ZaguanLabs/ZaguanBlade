import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { X, ArrowRight, ArrowLeft, Loader2, Network } from 'lucide-react';
import { ZLPService } from '../services/zlp';
import { ZLPGraphResponse, CallGraphNode, CallGraphEdge } from '../types/zlp';
import { formatUnknownBackendError } from '../utils/backendErrors';
import { useSmoothWheelScroll } from '../hooks/useSmoothWheelScroll';

interface GraphInspectorProps {
    symbolId: string;
    symbolName: string;
    filePath: string;
    onClose: () => void;
    onNavigate: (path: string, line: number, character: number) => void;
}

export const GraphInspector: React.FC<GraphInspectorProps> = ({
    symbolId,
    symbolName,
    filePath,
    onClose,
    onNavigate
}) => {
    const { t } = useTranslation();
    const [graph, setGraph] = useState<ZLPGraphResponse | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const handleGraphContentWheel = useSmoothWheelScroll<HTMLDivElement>();

    useEffect(() => {
        let isMounted = true;

        const fetchGraph = async () => {
            setLoading(true);
            setError(null);
            try {
                // In a real implementation, we might need to resolve the generic symbolId (e.g., 'main') 
                // to a specific qualified ID if the backend requires it. 
                // For now, we pass what we have.
                const data = await ZLPService.getCallGraph(symbolId);
                if (isMounted) setGraph(data);
            } catch (e: any) {
                if (isMounted) setError(`${t('gitGraph.loadFailed')}: ${formatUnknownBackendError(e)}`);
            } finally {
                if (isMounted) setLoading(false);
            }
        };

        if (symbolId) fetchGraph();

        return () => { isMounted = false; };
    }, [symbolId]);

    const renderNodeList = (nodes: CallGraphNode[], title: string, icon: React.ReactNode) => {
        if (nodes.length === 0) return null;

        return (
            <div className="mb-4">
                <div className="flex items-center gap-2 text-sm font-semibold text-(--fg-secondary) mb-2 px-1">
                    {icon}
                    {title}
                </div>
                <div className="space-y-1">
                    {nodes.map(node => (
                        <div
                            key={node.id}
                            className="p-2 rounded-[calc(var(--panel-radius)*0.4)] bg-(--bg-surface-hover) border border-(--border-subtle) cursor-pointer hover:border-(--accent-ai) transition-colors flex items-center justify-between group"
                            onClick={() => {
                                if (node.file && node.line) {
                                    onNavigate(node.file, node.line, 0); // Assuming 0 for column if not provided
                                }
                            }}
                        >
                            <div className="truncate text-xs font-mono text-(--fg-primary)">
                                {node.name}
                            </div>
                            {node.file && (
                                <div className="text-[10px] text-(--fg-tertiary) opacity-0 group-hover:opacity-100 transition-opacity truncate max-w-[150px] ml-2">
                                    {node.file.split('/').pop()}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            </div>
        );
    };

    // Helper to filter nodes based on edges
    const getIncomingNodes = () => {
        if (!graph) return [];
        const incomers = graph.edges
            .filter(e => e.to === graph.root_id)
            .map(e => graph.nodes.find(n => n.id === e.from))
            .filter((n): n is CallGraphNode => !!n);
        return incomers;
    };

    const getOutgoingNodes = () => {
        if (!graph) return [];
        const outgoers = graph.edges
            .filter(e => e.from === graph.root_id)
            .map(e => graph.nodes.find(n => n.id === e.to))
            .filter((n): n is CallGraphNode => !!n);
        return outgoers;
    };

    return (
        <div className="fixed inset-y-0 right-0 w-80 bg-(--bg-panel) border-l border-(--border-subtle) shadow-(--shadow-xl) z-50 flex flex-col animate-in slide-in-from-right duration-(--transition-base)">
            {/* Header */}
            <div className="h-12 border-b border-(--border-subtle) flex items-center justify-between px-4 bg-(--bg-app)">
                <div className="flex items-center gap-2 text-(--fg-primary) font-medium">
                    <Network className="w-4 h-4 text-(--accent-ai)" />
                    <span className="truncate max-w-[180px]">{symbolName}</span>
                </div>
                <button
                    onClick={onClose}
                    className="p-1 hover:bg-(--bg-surface-hover) rounded-[calc(var(--panel-radius)*0.35)] text-(--fg-tertiary) hover:text-(--fg-primary) transition-colors"
                >
                    <X className="w-4 h-4" />
                </button>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-y-auto p-4 custom-scrollbar" onWheel={handleGraphContentWheel}>
                {loading ? (
                    <div className="flex items-center justify-center py-10">
                        <Loader2 className="w-6 h-6 animate-spin text-(--accent-ai)" />
                    </div>
                ) : error ? (
                    <div className="p-3 bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] border border-(--state-danger)/20 rounded-[calc(var(--panel-radius)*0.45)] text-xs text-(--state-danger)">
                        {error}
                    </div>
                ) : graph ? (
                    <>
                        {renderNodeList(getIncomingNodes(), t('graphInspector.incomingCalls'), <ArrowLeft className="w-3.5 h-3.5 text-(--accent-warning)" />)}

                        {/* Current Node Representation */}
                        <div className="my-6 p-3 bg-[color-mix(in_srgb,var(--accent-ai)_12%,var(--bg-surface))] border border-(--accent-ai)/25 rounded-[calc(var(--panel-radius)*0.55)] text-center">
                            <div className="text-[10px] uppercase text-(--fg-secondary) font-bold mb-1">{t('graphInspector.focus')}</div>
                            <div className="font-mono text-sm font-bold text-(--fg-primary)">{symbolName}</div>
                        </div>

                        {renderNodeList(getOutgoingNodes(), t('graphInspector.outgoingCalls'), <ArrowRight className="w-3.5 h-3.5 text-(--accent-planning)" />)}

                        {getIncomingNodes().length === 0 && getOutgoingNodes().length === 0 && (
                            <div className="text-center text-xs text-(--fg-tertiary) italic py-4">
                                {t('graphInspector.noCallsFound')}
                            </div>
                        )}
                    </>
                ) : null}
            </div>
        </div>
    );
};
