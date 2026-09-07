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
    schema_version: 1;
    entries: IntegrationDefinition[];
}

export interface IntegrationConfigSnapshot {
    revision: string;
    config: IntegrationConfig;
}

export interface WorkspaceIntegrationSettings {
    disabled_ids: string[];
}
