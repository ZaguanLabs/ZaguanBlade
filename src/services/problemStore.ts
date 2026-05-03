import { subscribeBladeNestedEventType } from './bladeEvents';
import type { ZLPValidationError } from '../types/zlp';

export type WorkspaceDiagnosticSeverity = 'error' | 'warning' | 'info';

export interface WorkspaceDiagnostic {
    id: string;
    source: string;
    severity: WorkspaceDiagnosticSeverity;
    message: string;
    file?: string;
    line?: number;
    column?: number;
    raw?: string;
    terminalId?: string;
    timestamp: number;
}

const MAX_BUFFER_LENGTH = 30000;
const MAX_DIAGNOSTICS = 80;
const ACTIVE_DIAGNOSTIC_TTL_MS = 20 * 60 * 1000;

const diagnosticsById = new Map<string, WorkspaceDiagnostic>();
const terminalBuffers = new Map<string, string>();
let terminalCaptureUnsubscribe: (() => void) | null = null;

function stripAnsi(value: string): string {
    return value
        .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
        .replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, '')
        .replace(/\r/g, '\n');
}

function normalizePath(path: string): string {
    return path.replace(/\\/g, '/');
}

function diagnosticKey(diagnostic: Omit<WorkspaceDiagnostic, 'id' | 'timestamp'>): string {
    return [
        diagnostic.source,
        diagnostic.severity,
        diagnostic.file ? normalizePath(diagnostic.file) : '',
        diagnostic.line ?? '',
        diagnostic.column ?? '',
        diagnostic.message,
        diagnostic.terminalId ?? '',
    ].join('\u001f');
}

function upsertDiagnostic(diagnostic: Omit<WorkspaceDiagnostic, 'id' | 'timestamp'>): void {
    const id = diagnosticKey(diagnostic);
    diagnosticsById.set(id, {
        ...diagnostic,
        id,
        file: diagnostic.file ? normalizePath(diagnostic.file) : undefined,
        timestamp: Date.now(),
    });

    if (diagnosticsById.size <= MAX_DIAGNOSTICS) {
        return;
    }

    const oldest = [...diagnosticsById.values()].sort((a, b) => a.timestamp - b.timestamp)[0];
    if (oldest) {
        diagnosticsById.delete(oldest.id);
    }
}

function importTraceFrom(lines: string[], index: number): string | undefined {
    const traceIndex = lines.findIndex((line, lineIndex) => lineIndex > index && line.trim() === 'Import trace:');
    if (traceIndex === -1) {
        return undefined;
    }

    const traceLines = [lines[traceIndex]];
    for (let lineIndex = traceIndex + 1; lineIndex < lines.length; lineIndex += 1) {
        const line = lines[lineIndex];
        if (!line.trim()) {
            break;
        }
        traceLines.push(line);
        if (traceLines.length >= 8) {
            break;
        }
    }

    return traceLines.join('\n');
}

function classifySource(line: string): string {
    if (/CssSyntaxError|postcss/i.test(line)) return 'postcss';
    if (/turbopack|next\.js|next\/dist/i.test(line)) return 'next';
    if (/vite/i.test(line)) return 'vite';
    if (/TS\d{4}/.test(line)) return 'typescript';
    if (/eslint/i.test(line)) return 'eslint';
    if (/rustc|cargo/i.test(line)) return 'cargo';
    return 'terminal';
}

function parseTerminalDiagnostics(terminalId: string, buffer: string): void {
    const lines = buffer.split('\n');

    for (let index = 0; index < lines.length; index += 1) {
        const line = lines[index];
        const cssMatch = line.match(/CssSyntaxError:\s+(.+?):(\d+):(\d+):\s+(.+)/);
        const tsMatch = line.match(/(.+?)\((\d+),(\d+)\):\s+(error|warning)\s+(TS\d+):\s+(.+)/);
        const genericMatch = line.match(/(?:Error:\s+)?(.+\.(?:ts|tsx|js|jsx|css|scss|sass|json|rs|py|go|md|mdx)):(\d+):(\d+):\s+(.+)/);
        const match = cssMatch || tsMatch || genericMatch;

        if (!match) {
            continue;
        }

        const isTsMatch = match === tsMatch;
        const file = match[1];
        const lineNumber = Number(match[2]);
        const columnNumber = Number(match[3]);
        const severity = isTsMatch && match[4] === 'warning' ? 'warning' : 'error';
        const message = isTsMatch ? `${match[5]}: ${match[6]}` : match[4];
        const trace = importTraceFrom(lines, index);
        const raw = trace ? `${line}\n${trace}` : line;

        upsertDiagnostic({
            source: classifySource(line),
            severity,
            file,
            line: lineNumber,
            column: columnNumber,
            message,
            raw,
            terminalId,
        });
    }
}

export function ensureProblemCaptureStarted(): void {
    if (terminalCaptureUnsubscribe || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
        return;
    }

    terminalCaptureUnsubscribe = subscribeBladeNestedEventType('Terminal', 'Output', (payload) => {
        const cleaned = stripAnsi(payload.data);
        if (!cleaned) {
            return;
        }

        const previous = terminalBuffers.get(payload.id) ?? '';
        const next = (previous + cleaned).slice(-MAX_BUFFER_LENGTH);
        terminalBuffers.set(payload.id, next);
        parseTerminalDiagnostics(payload.id, next);
    });
}

export function recordZlpDiagnostics(file: string, errors: ZLPValidationError[]): void {
    const now = Date.now();
    const normalizedFile = normalizePath(file);

    for (const [id, diagnostic] of diagnosticsById) {
        if (diagnostic.source.startsWith('zlp') && diagnostic.file === normalizedFile) {
            diagnosticsById.delete(id);
        }
    }

    for (const error of errors) {
        const severity = error.severity === 'warning' ? 'warning' : error.severity === 'info' ? 'info' : 'error';
        const line = error.range.start.line + 1;
        const column = error.range.start.column + 1;
        const source = error.source ? `zlp/${error.source}` : 'zlp';
        const id = [source, severity, normalizedFile, line, column, error.message].join('\u001f');
        diagnosticsById.set(id, {
            id,
            source,
            severity,
            file: normalizedFile,
            line,
            column,
            message: error.code ? `${error.code}: ${error.message}` : error.message,
            timestamp: now,
        });
    }
}

export function getActiveDiagnostics(activeFile: string | null, openFiles: string[], limit = 12): WorkspaceDiagnostic[] {
    const now = Date.now();
    const normalizedActiveFile = activeFile ? normalizePath(activeFile) : null;
    const normalizedOpenFiles = new Set(openFiles.map(normalizePath));

    return [...diagnosticsById.values()]
        .filter((diagnostic) => now - diagnostic.timestamp <= ACTIVE_DIAGNOSTIC_TTL_MS)
        .filter((diagnostic) => diagnostic.severity === 'error' || diagnostic.severity === 'warning')
        .filter((diagnostic) => {
            if (!diagnostic.file) {
                return true;
            }
            if (diagnostic.severity === 'error' && diagnostic.source !== 'zlp') {
                return true;
            }
            return diagnostic.file === normalizedActiveFile || normalizedOpenFiles.has(diagnostic.file);
        })
        .sort((a, b) => {
            const severityDelta = Number(a.severity !== 'error') - Number(b.severity !== 'error');
            if (severityDelta !== 0) return severityDelta;
            return b.timestamp - a.timestamp;
        })
        .slice(0, limit);
}
