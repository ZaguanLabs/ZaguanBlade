import type { IntegrationLaunchReview, McpConnectionStatus } from '../types/integrations';
type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;

/** Owns only the pending launch request. A successfully returned connection is
 * owned by the backend workspace, so closing Settings leaves it running. */
export class McpConnectionRequest {
    private disposed = false;
    private ticket: string | null = null;
    private running = false;
    constructor(private readonly invoke: Invoke) {}
    async prepare(integrationId: string, expectedRevision: string, workspacePath: string): Promise<IntegrationLaunchReview | null> {
        const review = await this.invoke<IntegrationLaunchReview>('prepare_mcp_connection', { integrationId, expectedRevision, workspacePath });
        if (this.disposed) { await this.cancel(review.ticket_id); return null; }
        this.ticket = review.ticket_id;
        return review;
    }
    async connect(): Promise<McpConnectionStatus | null> {
        if (this.disposed || !this.ticket || this.running) return null;
        this.running = true;
        const ticket = this.ticket;
        try {
            const status = await this.invoke<McpConnectionStatus>('connect_mcp', { ticketId: ticket });
            if (this.disposed) { await this.cancel(ticket); return null; }
            return status;
        } finally { this.ticket = null; }
    }
    dispose(): void {
        this.disposed = true;
        if (this.ticket) { void this.cancel(this.ticket); this.ticket = null; }
    }
    private async cancel(ticketId: string): Promise<void> {
        try { await this.invoke('cancel_integration_test', { ticketId }); } catch { /* backend expiry/workspace ownership remain authoritative */ }
    }
}
