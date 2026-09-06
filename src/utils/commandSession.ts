export type CommandSessionAction = 'poll' | 'write' | 'interrupt' | 'kill';

export function getCommandSessionAction(args: Record<string, unknown> | null): CommandSessionAction {
    if (args?.kill === true) return 'kill';
    const input = args?.input;
    if (typeof input !== 'string' || input.length === 0) return 'poll';
    // Match the interrupt spellings normalized by the backend.
    if (['\u0003', '\\x03', '\\u0003', '^C'].includes(input)) return 'interrupt';
    return 'write';
}

export interface CommandSessionResult {
    state: 'running' | 'exited' | 'unknown';
    sessionId?: string;
    elapsedSeconds?: number;
    exitCode?: number;
    output: string;
}

export function parseCommandSessionResult(result?: string): CommandSessionResult {
    const text = (result ?? '').replace(/\r\n/g, '\n');
    // Parse only the backend envelope, never status-like lines inside command output.
    const match = /^Wall time: (\d+) seconds\nProcess (?:running with session ID ([^\n]+)|exited with code (-?\d+))\nOutput:\n/.exec(text);
    if (!match) return { state: 'unknown', output: text };

    const output = text.slice(match[0].length);
    return {
        state: match[2] ? 'running' : 'exited',
        ...(match[2] ? { sessionId: match[2] } : { exitCode: Number(match[3]) }),
        elapsedSeconds: Number(match[1]),
        output: output.trim() === '(no new output)' ? '' : output,
    };
}
