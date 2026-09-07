import type { IntegrationConfig, IntegrationDefinition, IntegrationProcess } from '../types/integrations';

export const emptyIntegrationConfig: IntegrationConfig = { schema_version: 1, entries: [] };

export function newIntegrationProcess(command = '', args: string[] = []): IntegrationProcess {
    return { command, args, cwd: null, env: {} };
}

/** IDs, including ACP forwarding references, survive display-name edits. */
export function removeIntegration(config: IntegrationConfig, id: string): IntegrationConfig {
    return {
        ...config,
        entries: config.entries.filter(entry => entry.id !== id).map(entry => (
            entry.connection.protocol === 'acp'
                ? { ...entry, connection: { ...entry.connection, mcp_servers: entry.connection.mcp_servers.filter(serverId => serverId !== id) } }
                : entry
        )),
    };
}

export function replaceIntegration(config: IntegrationConfig, entry: IntegrationDefinition): IntegrationConfig {
    return { ...config, entries: config.entries.map(existing => existing.id === entry.id ? entry : existing) };
}

const errorCodes = new Set([
    'unsupported_version', 'invalid_config', 'invalid_id', 'duplicate_id', 'invalid_name',
    'invalid_command', 'invalid_arguments', 'invalid_directory', 'invalid_environment',
    'invalid_url', 'invalid_headers', 'invalid_reference', 'too_large', 'read_failed',
    'write_failed', 'conflict', 'desktop_only',
]);

/** Only known backend codes become translation keys; never display raw config. */
export function integrationErrorKey(error: unknown): string {
    return `settings.integrations.errors.${typeof error === 'string' && errorCodes.has(error) ? error : 'unknown'}`;
}
