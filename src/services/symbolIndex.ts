import { invoke } from '@tauri-apps/api/core';

export interface IndexedPosition {
    line: number;
    character: number;
}

export interface IndexedRange {
    start: IndexedPosition;
    end: IndexedPosition;
}

export interface InspectorSymbol {
    id: string;
    name: string;
    qualified_name: string;
    symbol_type: string;
    file_path: string;
    range: IndexedRange;
    signature?: string | null;
}

export interface InspectorGraphNode {
    symbol: InspectorSymbol;
    depth: number;
}

export interface InspectorGraphEdge {
    source_symbol_id: string;
    target_symbol_id?: string | null;
    target_name: string;
    relationship_type: string;
    traversal_direction: 'incoming' | 'outgoing';
    line: number;
    resolved: boolean;
    observation_kind: 'syntax_extracted' | 'index_structural';
    resolution_strategy?: string | null;
    effective_confidence: number;
}

export interface InspectorGraphResponse {
    root: InspectorSymbol;
    nodes: InspectorGraphNode[];
    edges: InspectorGraphEdge[];
    direction: 'incoming' | 'outgoing' | 'both';
    relationship_types: string[];
    min_confidence: number;
    truncated: boolean;
    unresolved_edge_count: number;
}

export type InspectorGraphDirection = 'incoming' | 'outgoing' | 'both';

export class SymbolsIndexService {
    static resolveSymbolAt(
        filePath: string,
        line: number,
        character: number,
    ): Promise<InspectorSymbol | null> {
        return invoke<InspectorSymbol | null>('resolve_local_symbol_at', {
            filePath,
            line,
            character,
        });
    }

    static getSymbolGraph(options: {
        symbolId: string;
        direction: InspectorGraphDirection;
        relationships?: string[];
        minConfidence: number;
    }): Promise<InspectorGraphResponse> {
        return invoke<InspectorGraphResponse>('get_local_symbol_graph', options);
    }
}
