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

/** What a one-use approval authorizes. Environment and header values are never included. */
export type IntegrationLaunchReview = { ticket_id: string } & (
    | { kind: 'process'; executable: string; cwd: string; args: string[] }
    | { kind: 'http'; url: string; header_names: string[]; insecure: boolean }
);

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

export interface McpConnectionStatus {
    connection_id: string;
    integration_id: string;
    name: string;
    workspace: { workspace_id: string; generation: string };
    phase: 'connecting' | 'connected' | 'refreshing' | 'stopping' | 'disconnected' | 'failed';
    protocol_version: string | null;
    catalog_revision: string | null;
    tools: number;
    resources: boolean;
    prompts: boolean;
    error: string | null;
}

/** A connected MCP server in the current workspace that offers resources. */
export interface McpResourceServer {
    connection_id: string;
    integration_id: string;
    name: string;
}

export interface McpResourceEntry {
    uri: string;
    name: string;
    title: string | null;
    description: string | null;
    mime_type: string | null;
    size: number | null;
}

export interface McpResourceTemplateEntry {
    uri_template: string;
    name: string;
    title: string | null;
    description: string | null;
    mime_type: string | null;
    variables: string[];
}

export interface McpResourceListing {
    resources: McpResourceEntry[];
    templates: McpResourceTemplateEntry[];
    truncated: boolean;
}

export type McpResourcePart =
    | { kind: 'text'; uri: string; mime_type: string | null; text: string }
    | { kind: 'image'; uri: string; mime_type: string; data: string }
    | { kind: 'unsupported'; uri: string; mime_type: string | null };

export interface McpResourceRead {
    uri: string;
    parts: McpResourcePart[];
    truncated: boolean;
}

export type McpPromptServer = McpResourceServer;

export interface McpPromptArgument {
    name: string;
    title: string | null;
    description: string | null;
    required: boolean;
}

export interface McpPromptEntry {
    name: string;
    title: string | null;
    description: string | null;
    arguments: McpPromptArgument[];
}

export interface McpPromptListing {
    prompts: McpPromptEntry[];
    truncated: boolean;
}

export type McpPromptPart =
    | { kind: 'text'; text: string }
    | { kind: 'image'; mime_type: string; data: string }
    | { kind: 'resource'; uri: string; mime_type: string | null; text: string }
    | { kind: 'link'; uri: string; name: string }
    | { kind: 'unsupported' };

export interface McpPromptMessage {
    role: 'user' | 'assistant';
    part: McpPromptPart;
}

export interface McpPromptRender {
    name: string;
    description: string | null;
    messages: McpPromptMessage[];
    truncated: boolean;
}
