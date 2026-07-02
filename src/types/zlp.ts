// Validation
export interface ZLPValidationResponse {
    tier?: 'syntax' | 'lint' | 'compile';
    errors: ZLPValidationError[];
    valid?: boolean;
}

export interface ZLPValidationError {
    range: { start: { line: number; column: number }; end: { line: number; column: number } };
    severity: 'error' | 'warning' | 'info';
    message: string;
    code?: string;
    source?: string;
}

// Structure (The "Outline")
export interface StructureNode {
    name: string;
    kind: string; // 'Function', 'Class', 'Method', 'Property', etc.
    detail?: string;
    range: {
        start: { line: number; column: number };
        end: { line: number; column: number }
    };
    selectionRange: {
        start: { line: number; column: number };
        end: { line: number; column: number }
    };
    children?: StructureNode[];
}

export type ZLPStructureResponse = StructureNode[];

// Graph (The "Architecture Map")
export interface CallGraphNode {
    id: string;
    name: string;
    kind: string;
    file?: string;
    line?: number;
}

export interface CallGraphEdge {
    from: string; // Node ID
    to: string;   // Node ID
    kind: 'calls' | 'uses' | 'imports' | 'extends';
}

export interface ZLPGraphResponse {
    nodes: CallGraphNode[];
    edges: CallGraphEdge[];
    root_id: string;
}
