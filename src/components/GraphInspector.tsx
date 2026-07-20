import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
    ArrowLeft,
    ArrowRight,
    ChevronLeft,
    ExternalLink,
    Loader2,
    Network,
    X,
} from 'lucide-react';
import {
    SymbolsIndexService,
    type InspectorGraphDirection,
    type InspectorGraphEdge,
    type InspectorGraphResponse,
    type InspectorSymbol,
} from '../services/symbolIndex';
import { formatUnknownBackendError } from '../utils/backendErrors';
import { ScrollArea } from './ui/ScrollArea';
import { Surface } from './ui/Surface';
import { IconButton } from './ui/IconButton';

interface GraphInspectorProps {
    symbol: InspectorSymbol;
    onClose: () => void;
    onNavigate: (path: string, line: number, character: number) => void;
}

type RelationshipFilter = 'all' | 'call' | 'dependency' | 'type' | 'handles' | 'structure';

const relationshipsForFilter = (filter: RelationshipFilter): string[] | undefined => {
    switch (filter) {
        case 'call':
            return ['call'];
        case 'dependency':
            return ['import', 'export', 'usage'];
        case 'type':
            return ['extends', 'implements', 'uses_type'];
        case 'handles':
            return ['handles'];
        case 'structure':
            return ['contains'];
        default:
            return undefined;
    }
};

interface RelatedSymbolGroup {
    key: string;
    symbol: InspectorSymbol | null;
    targetName: string;
    edges: InspectorGraphEdge[];
}

const groupEdges = (
    graph: InspectorGraphResponse,
    direction: 'incoming' | 'outgoing',
): RelatedSymbolGroup[] => {
    const nodes = new Map(graph.nodes.map(node => [node.symbol.id, node.symbol]));
    const groups = new Map<string, RelatedSymbolGroup>();

    for (const edge of graph.edges) {
        if (edge.traversal_direction !== direction) continue;
        const neighborId = direction === 'incoming'
            ? edge.source_symbol_id
            : edge.target_symbol_id ?? null;
        const symbol = neighborId ? nodes.get(neighborId) ?? null : null;
        const key = neighborId ?? `unresolved:${edge.target_name}:${edge.relationship_type}`;
        const existing = groups.get(key);
        if (existing) {
            existing.edges.push(edge);
        } else {
            groups.set(key, {
                key,
                symbol,
                targetName: edge.target_name,
                edges: [edge],
            });
        }
    }

    return [...groups.values()].sort((left, right) => {
        const leftConfidence = Math.max(...left.edges.map(edge => edge.effective_confidence));
        const rightConfidence = Math.max(...right.edges.map(edge => edge.effective_confidence));
        return rightConfidence - leftConfidence
            || (left.symbol?.name ?? left.targetName).localeCompare(right.symbol?.name ?? right.targetName);
    });
};

export const GraphInspector: React.FC<GraphInspectorProps> = ({
    symbol,
    onClose,
    onNavigate,
}) => {
    const { t } = useTranslation();
    const [focus, setFocus] = useState(symbol);
    const [history, setHistory] = useState<InspectorSymbol[]>([]);
    const [graph, setGraph] = useState<InspectorGraphResponse | null>(null);
    const [direction, setDirection] = useState<InspectorGraphDirection>('both');
    const [relationshipFilter, setRelationshipFilter] = useState<RelationshipFilter>('all');
    const [minConfidence, setMinConfidence] = useState(0.5);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        setFocus(symbol);
        setHistory([]);
    }, [symbol]);

    useEffect(() => {
        let isCurrent = true;
        setLoading(true);
        setError(null);

        void SymbolsIndexService.getSymbolGraph({
            symbolId: focus.id,
            direction,
            relationships: relationshipsForFilter(relationshipFilter),
            minConfidence,
        }).then(data => {
            if (isCurrent) setGraph(data);
        }).catch(reason => {
            if (isCurrent) {
                setGraph(null);
                setError(formatUnknownBackendError(reason));
            }
        }).finally(() => {
            if (isCurrent) setLoading(false);
        });

        return () => {
            isCurrent = false;
        };
    }, [direction, focus.id, minConfidence, relationshipFilter]);

    const incoming = useMemo(
        () => graph ? groupEdges(graph, 'incoming') : [],
        [graph],
    );
    const outgoing = useMemo(
        () => graph ? groupEdges(graph, 'outgoing') : [],
        [graph],
    );

    const expandSymbol = (next: InspectorSymbol) => {
        setHistory(previous => [...previous, focus]);
        setFocus(next);
    };

    const goBack = () => {
        const prior = history[history.length - 1];
        if (!prior) return;
        setFocus(prior);
        setHistory(previous => previous.slice(0, -1));
    };

    const renderNodeList = (
        groups: RelatedSymbolGroup[],
        title: string,
        icon: React.ReactNode,
    ) => {
        if (groups.length === 0) return null;

        return (
            <section className="mb-4">
                <div className="mb-2 flex items-center gap-2 px-1 text-sm font-semibold text-(--fg-secondary)">
                    {icon}
                    {title}
                    <span className="ml-auto text-[10px] font-normal text-(--fg-tertiary)">{groups.length}</span>
                </div>
                <div className="space-y-1.5">
                    {groups.map(group => {
                        const confidence = Math.max(...group.edges.map(edge => edge.effective_confidence));
                        const relationships = [...new Set(group.edges.map(edge => edge.relationship_type))];
                        const symbolLabel = group.symbol?.name ?? group.targetName;
                        const symbolPath = group.symbol?.file_path;
                        return (
                            <Surface
                                key={group.key}
                                variant="row"
                                className={`group p-2.5 transition-colors ${group.symbol
                                    ? 'cursor-pointer hover:border-(--accent-ai)'
                                    : 'opacity-70'
                                    }`}
                                onClick={() => {
                                    if (group.symbol) {
                                        onNavigate(
                                            group.symbol.file_path,
                                            group.symbol.range.start.line,
                                            group.symbol.range.start.character,
                                        );
                                    }
                                }}
                            >
                                <div className="flex items-start gap-2">
                                    <div className="min-w-0 flex-1">
                                        <div className="truncate font-mono text-xs text-(--fg-primary)">
                                            {symbolLabel}
                                        </div>
                                        <div className="mt-1 flex flex-wrap items-center gap-1">
                                            {relationships.map(relationship => (
                                                <span
                                                    key={relationship}
                                                    className="rounded bg-(--bg-surface-hover) px-1.5 py-0.5 text-[9px] uppercase text-(--fg-secondary)"
                                                >
                                                    {relationship}
                                                </span>
                                            ))}
                                            <span className="text-[9px] tabular-nums text-(--fg-tertiary)">
                                                {Math.round(confidence * 100)}%
                                            </span>
                                            <span className="text-[9px] text-(--fg-tertiary)">
                                                {group.edges.some(edge => edge.observation_kind === 'index_structural')
                                                    ? t('graphInspector.structural')
                                                    : t('graphInspector.syntax')}
                                            </span>
                                        </div>
                                        {symbolPath && (
                                            <div className="mt-1 truncate text-[10px] text-(--fg-tertiary)">
                                                {symbolPath}:{(group.symbol?.range.start.line ?? 0) + 1}
                                            </div>
                                        )}
                                    </div>
                                    {group.symbol && (
                                        <div className="flex shrink-0 items-center gap-0.5">
                                            <ExternalLink className="h-3 w-3 text-(--fg-tertiary) opacity-0 transition-opacity group-hover:opacity-100" aria-hidden="true" />
                                            <IconButton
                                                size="xs"
                                                tone="accent"
                                                title={t('graphInspector.expand')}
                                                onClick={event => {
                                                    event.stopPropagation();
                                                    expandSymbol(group.symbol!);
                                                }}
                                            >
                                                <Network className="h-3.5 w-3.5" aria-hidden="true" />
                                            </IconButton>
                                        </div>
                                    )}
                                </div>
                            </Surface>
                        );
                    })}
                </div>
            </section>
        );
    };

    const selectClassName = 'min-w-0 rounded-(--radius-control) border border-(--border-default) bg-(--bg-surface) px-2 py-1 text-[10px] text-(--fg-primary) focus:border-(--accent-ai) focus:outline-none';

    return (
        <aside
            aria-label={t('graphInspector.title')}
            className="fixed inset-y-0 right-0 z-50 flex w-96 flex-col border-l border-(--border-subtle) bg-(--bg-panel) shadow-(--shadow-xl) animate-in slide-in-from-right duration-(--transition-base)"
        >
            <header className="flex min-h-12 items-center justify-between border-b border-(--border-subtle) bg-(--bg-app) px-3">
                <div className="flex min-w-0 items-center gap-1.5 text-(--fg-primary)">
                    <IconButton
                        size="xs"
                        aria-label={t('common.back')}
                        disabled={history.length === 0}
                        onClick={goBack}
                        className={history.length === 0 ? 'opacity-30' : undefined}
                    >
                        <ChevronLeft className="h-4 w-4" aria-hidden="true" />
                    </IconButton>
                    <Network className="h-4 w-4 shrink-0 text-(--accent-ai)" aria-hidden="true" />
                    <div className="min-w-0">
                        <div className="truncate text-sm font-medium">{focus.name}</div>
                        <div className="truncate text-[9px] text-(--fg-tertiary)">{focus.symbol_type}</div>
                    </div>
                </div>
                <IconButton aria-label={t('common.close')} onClick={onClose}>
                    <X aria-hidden="true" className="h-4 w-4" />
                </IconButton>
            </header>

            <div className="grid grid-cols-3 gap-1.5 border-b border-(--border-subtle) bg-(--bg-app) px-3 py-2">
                <select
                    aria-label={t('graphInspector.direction')}
                    value={direction}
                    onChange={event => setDirection(event.target.value as InspectorGraphDirection)}
                    className={selectClassName}
                >
                    <option value="both">{t('graphInspector.both')}</option>
                    <option value="incoming">{t('graphInspector.incoming')}</option>
                    <option value="outgoing">{t('graphInspector.outgoing')}</option>
                </select>
                <select
                    aria-label={t('graphInspector.relationship')}
                    value={relationshipFilter}
                    onChange={event => setRelationshipFilter(event.target.value as RelationshipFilter)}
                    className={selectClassName}
                >
                    <option value="all">{t('graphInspector.allRelationships')}</option>
                    <option value="call">{t('graphInspector.calls')}</option>
                    <option value="dependency">{t('graphInspector.dependencies')}</option>
                    <option value="type">{t('graphInspector.types')}</option>
                    <option value="handles">{t('graphInspector.handlers')}</option>
                    <option value="structure">{t('graphInspector.structure')}</option>
                </select>
                <select
                    aria-label={t('graphInspector.confidence')}
                    value={minConfidence}
                    onChange={event => setMinConfidence(Number(event.target.value))}
                    className={selectClassName}
                >
                    <option value={0}>{t('graphInspector.anyConfidence')}</option>
                    <option value={0.5}>≥ 50%</option>
                    <option value={0.75}>≥ 75%</option>
                    <option value={0.9}>≥ 90%</option>
                </select>
            </div>

            <ScrollArea className="flex-1 p-4 custom-scrollbar">
                {loading ? (
                    <div role="status" aria-live="polite" aria-label={t('common.loading')} className="flex items-center justify-center py-10">
                        <Loader2 aria-hidden="true" className="h-6 w-6 animate-spin text-(--accent-ai)" />
                    </div>
                ) : error ? (
                    <div role="alert" className="rounded-(--radius-control) border border-(--state-danger)/20 bg-[color-mix(in_srgb,var(--state-danger)_10%,transparent)] p-3 text-xs text-(--state-danger)">
                        {t('graphInspector.loadFailed')}: {error}
                    </div>
                ) : graph ? (
                    <>
                        {renderNodeList(
                            incoming,
                            t('graphInspector.incomingRelationships'),
                            <ArrowLeft className="h-3.5 w-3.5 text-(--accent-warning)" aria-hidden="true" />,
                        )}

                        <Surface variant="row" className="my-5 border-(--accent-ai)/25 bg-[color-mix(in_srgb,var(--accent-ai)_12%,var(--bg-surface))] p-3 text-center">
                            <div className="mb-1 text-[10px] font-bold uppercase text-(--fg-secondary)">{t('graphInspector.focus')}</div>
                            <div className="truncate font-mono text-sm font-bold text-(--fg-primary)">
                                {focus.qualified_name === '__file__' ? focus.name : focus.qualified_name}
                            </div>
                            <button
                                type="button"
                                className="mt-1 text-[10px] text-(--fg-tertiary) hover:text-(--accent-ai)"
                                onClick={() => onNavigate(focus.file_path, focus.range.start.line, focus.range.start.character)}
                            >
                                {focus.file_path}:{focus.range.start.line + 1}
                            </button>
                        </Surface>

                        {renderNodeList(
                            outgoing,
                            t('graphInspector.outgoingRelationships'),
                            <ArrowRight className="h-3.5 w-3.5 text-(--accent-planning)" aria-hidden="true" />,
                        )}

                        {incoming.length === 0 && outgoing.length === 0 && (
                            <div className="py-4 text-center text-xs italic text-(--fg-tertiary)">
                                {t('graphInspector.noRelationshipsFound')}
                            </div>
                        )}

                        {(graph.truncated || graph.unresolved_edge_count > 0) && (
                            <div className="mt-4 border-t border-(--border-subtle) pt-3 text-[10px] text-(--fg-tertiary)">
                                {graph.truncated && <div>{t('graphInspector.truncated')}</div>}
                                {graph.unresolved_edge_count > 0 && (
                                    <div>{t('graphInspector.unresolved', { count: graph.unresolved_edge_count })}</div>
                                )}
                            </div>
                        )}
                    </>
                ) : null}
            </ScrollArea>
        </aside>
    );
};
