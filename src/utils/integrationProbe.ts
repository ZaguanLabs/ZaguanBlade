import type { IntegrationLaunchReview, IntegrationProbeResult } from '../types/integrations';

const errors = new Set(['desktop_only', 'config_changed', 'workspace_changed', 'unsupported_transport',
    'executable_not_found', 'invalid_directory', 'secret_unavailable', 'secret_missing', 'invalid_secret',
    'approval_expired', 'busy', 'launch_failed', 'protocol_failed', 'unsupported_version', 'output_limit',
    'timed_out', 'cancelled', 'invalid_catalog', 'catalog_changed', 'integration_disabled', 'policy_unavailable', 'connection_not_ready']);

export function integrationProbeErrorKey(error: unknown): string {
    return `settings.integrations.test.errors.${typeof error === 'string' && errors.has(error) ? error : 'protocol_failed'}`;
}

type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;

/** One active request per form. Disposal cancels even a prepare response that
 * arrives after unmount; stale results never become a newer request's result. */
export class IntegrationProbeRequest {
    private disposed = false;
    private ticket: string | null = null;
    private running = false;

    constructor(private readonly invoke: Invoke) {}

    async prepare(integrationId: string, expectedRevision: string, workspacePath: string): Promise<IntegrationLaunchReview | null> {
        const review = await this.invoke<IntegrationLaunchReview>('prepare_integration_test', { integrationId, expectedRevision, workspacePath });
        if (this.disposed) {
            await this.cancelTicket(review.ticket_id);
            return null;
        }
        this.ticket = review.ticket_id;
        return review;
    }

    async run(): Promise<IntegrationProbeResult | null> {
        if (this.disposed || !this.ticket || this.running) return null;
        this.running = true;
        try {
            const result = await this.invoke<IntegrationProbeResult>('run_integration_test', { ticketId: this.ticket });
            return this.disposed ? null : result;
        } finally { this.ticket = null; }
    }

    dispose(): void {
        this.disposed = true;
        if (this.ticket) {
            void this.cancelTicket(this.ticket);
            this.ticket = null;
        }
    }

    private async cancelTicket(ticketId: string): Promise<void> {
        // Backend TTL/deadline/workspace cancellation remain authoritative if
        // this best-effort IPC fails during webview shutdown.
        try { await this.invoke('cancel_integration_test', { ticketId }); } catch { /* no raw IPC errors */ }
    }
}
