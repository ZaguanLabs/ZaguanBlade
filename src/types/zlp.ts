export type ZLPMode = 'fast' | 'full' | 'auto';

export interface ZLPRequest<T = any> {
    method: string;
    params: T;
}

export interface ZLPResponse<T = any> {
    result?: T;
    error?: ZLPError;
}

export interface ZLPError {
    code: string;
    message: string;
    data?: any;
}

// Capabilities (The "Ping")
export interface ZLPCapabilitiesParams {
    client_name: string;
    version: string;
}

export interface ZLPCapabilitiesResult {
    server_version: string;
    features: string[];
}

// Validation
export interface ZLPValidationParams {
    path: string;
    content: string;
    language: string;
    mode: ZLPMode;
    overlay?: Record<string, string>;
}

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
export interface ZLPStructureParams {
    file: string;
    content: string; // We send content because the server is stateless/might not be in sync yet
}

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
export interface ZLPGraphParams {
    symbol_id: string; // The fully qualified name or ID returned by structure/search
    direction: 'incoming' | 'outgoing' | 'both';
    depth: number;
}

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
