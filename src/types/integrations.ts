export type ConfigValue = { kind: 'text'; value: string } | { kind: 'secret'; name: string };

export interface IntegrationProcess {
    command: string;
    args: string[];
    cwd: string | null;
    env: Record<string, ConfigValue>;
}

export type McpTransport =
    | { type: 'stdio'; process: IntegrationProcess }
    | { type: 'streamable_http'; url: string; headers: Record<string, ConfigValue> };

export type IntegrationConnection =
    | { protocol: 'mcp'; transport: McpTransport }
    | { protocol: 'acp'; process: IntegrationProcess; mcp_servers: string[] };

export interface IntegrationDefinition {
    id: string;
    name: string;
    enabled: boolean;
    connection: IntegrationConnection;
}

export interface IntegrationConfig {
    symbols_index_enabled?: boolean;
    schema_version: 1;
    entries: IntegrationDefinition[];
}

export interface IntegrationConfigSnapshot {
    revision: string;
    config: IntegrationConfig;
}

export interface WorkspaceIntegrationSettings {
    symbols_index_enabled?: boolean | null;
    disabled_ids: string[];
}

export interface IntegrationLaunchReview {
    ticket_id: string;
    executable: string;
    cwd: string;
    args: string[];
}

export interface IntegrationProbeResult {
    protocol_version: string;
    tools: number;
    resources: boolean;
    prompts: boolean;
    authentication_methods: number;
    catalog?: McpCatalog | null;
}

export interface McpCatalogTool {
    alias: string;
    definition: {
        name: string;
        title?: string;
        description?: string;
        inputSchema: Record<string, unknown>;
        outputSchema?: Record<string, unknown>;
        annotations?: Record<string, unknown>;
        icons?: Array<Record<string, unknown>>;
        _meta?: Record<string, unknown>;
    };
}

export interface McpCatalog {
    schema_version: 1;
    integration_id: string;
    revision: string;
    tools: McpCatalogTool[];
}
