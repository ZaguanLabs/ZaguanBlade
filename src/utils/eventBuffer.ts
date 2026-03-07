/**
 * Event Ordering Buffer for v1.1 Protocol
 * 
 * Handles out-of-order streaming events by buffering and applying them in sequence.
 * Used for MessageDelta and TerminalOutput events with sequence numbers.
 */

export interface BufferedChunk<T> {
    seq: number;
    data: T;
    is_final?: boolean;
}

export interface MessageChunk {
    chunk: string;
    is_final: boolean;
    type?: 'content' | 'reasoning';
}

export interface TerminalChunk {
    data: string;
}

export class EventBuffer<T> {
    private chunks: Map<number, BufferedChunk<T>>;
    private nextSeq: number;
    private onApply: (data: T, is_final?: boolean, seq?: number) => void;
    private onComplete?: () => void;
    private initialSeq: number;

    constructor(
        onApply: (data: T, is_final?: boolean, seq?: number) => void,
        onComplete?: () => void,
        initialSeq = 0,
    ) {
        this.chunks = new Map();
        this.nextSeq = initialSeq;
        this.onApply = onApply;
        this.onComplete = onComplete;
        this.initialSeq = initialSeq;
    }

    /**
     * Add a chunk to the buffer and apply all sequential chunks
     */
    add(seq: number, data: T, is_final?: boolean): void {
        // Drop stale/already-applied chunks (late delivery or duplicate replay).
        // A true new stream should use a new message ID or explicitly clear the buffer.
        if (seq < this.nextSeq) {
            return;
        }

        // Avoid re-buffering exact same sequence key repeatedly.
        if (!this.chunks.has(seq)) {
            this.chunks.set(seq, { seq, data, is_final });
        }

        // Apply all sequential chunks starting from nextSeq
        while (this.chunks.has(this.nextSeq)) {
            const chunk = this.chunks.get(this.nextSeq)!;
            this.onApply(chunk.data, chunk.is_final, chunk.seq);
            this.chunks.delete(this.nextSeq);
            this.nextSeq++;

            // If this was the final chunk, call completion callback
            if (chunk.is_final && this.onComplete) {
                this.onComplete();
                this.clear();
                break;
            }
        }
    }

    /**
     * Clear the buffer (useful for new streams)
     */
    clear(): void {
        this.chunks.clear();
        this.nextSeq = this.initialSeq;
    }

    /**
     * Get the number of buffered chunks waiting to be applied
     */
    getPendingCount(): number {
        return this.chunks.size;
    }

    /**
     * Get the next expected sequence number
     */
    getNextSeq(): number {
        return this.nextSeq;
    }
}

/**
 * Manager for multiple event buffers (e.g., multiple messages or terminals)
 */
export class BufferManager<T> {
    private buffers: Map<string, EventBuffer<T>>;
    private onApply: (id: string, data: T, is_final?: boolean, seq?: number) => void;
    private onComplete?: (id: string) => void;
    private initialSeq: number;

    constructor(
        onApply: (id: string, data: T, is_final?: boolean, seq?: number) => void,
        onComplete?: (id: string) => void,
        initialSeq = 0,
    ) {
        this.buffers = new Map();
        this.onApply = onApply;
        this.onComplete = onComplete;
        this.initialSeq = initialSeq;
    }

    /**
     * Add a chunk for a specific ID
     */
    add(id: string, seq: number, data: T, is_final?: boolean): void {
        if (!this.buffers.has(id)) {
            this.buffers.set(
                id,
                new EventBuffer<T>(
                    (data, is_final, seq) => this.onApply(id, data, is_final, seq),
                    () => {
                        if (this.onComplete) {
                            this.onComplete(id);
                        }
                        // Clean up completed buffer
                        this.buffers.delete(id);
                    },
                    this.initialSeq,
                )
            );
        }

        this.buffers.get(id)!.add(seq, data, is_final);
    }

    /**
     * Clear a specific buffer
     */
    clear(id: string): void {
        const buffer = this.buffers.get(id);
        if (buffer) {
            buffer.clear();
            this.buffers.delete(id);
        }
    }

    /**
     * Clear all buffers
     */
    clearAll(): void {
        this.buffers.clear();
    }

    /**
     * Get buffer statistics for debugging
     */
    getStats(): { id: string; pending: number; nextSeq: number }[] {
        const stats: { id: string; pending: number; nextSeq: number }[] = [];
        this.buffers.forEach((buffer, id) => {
            stats.push({
                id,
                pending: buffer.getPendingCount(),
                nextSeq: buffer.getNextSeq()
            });
        });
        return stats;
    }
}

/**
 * Specialized buffer for chat messages
 */
export class MessageBuffer extends BufferManager<MessageChunk> {
    constructor(
        onChunk: (id: string, seq: number, chunk: string, is_final: boolean, type: 'content' | 'reasoning') => void,
        onComplete?: (id: string) => void
    ) {
        super(
            (id, data, is_final, seq) => onChunk(id, seq ?? 0, data.chunk, is_final ?? false, data.type || 'content'),
            onComplete
        );
    }

    addMessageDelta(id: string, seq: number, chunk: string, is_final: boolean): void {
        this.add(id, seq, { chunk, is_final, type: 'content' }, is_final);
    }

    addReasoningDelta(id: string, seq: number, chunk: string, is_final: boolean): void {
        this.add(id, seq, { chunk, is_final, type: 'reasoning' }, is_final);
    }
}

/**
 * Specialized buffer for terminal output
 */
export class TerminalBuffer extends BufferManager<TerminalChunk> {
    constructor(
        onOutput: (id: string, data: string) => void
    ) {
        super((id, data) => onOutput(id, data.data));
    }

    addOutput(id: string, seq: number, data: string): void {
        this.add(id, seq, { data });
    }
}
