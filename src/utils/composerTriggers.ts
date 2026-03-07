export type ComposerTriggerKind = 'command' | 'path' | 'slash-command';

export interface ComposerTrigger {
    kind: ComposerTriggerKind;
    query: string;
    rangeStart: number;
    rangeEnd: number;
}

const SLASH_COMMANDS = ['model', 'plan', 'default'] as const;

function clampCursor(text: string, cursor: number): number {
    if (!Number.isFinite(cursor)) {
        return text.length;
    }
    return Math.max(0, Math.min(text.length, Math.floor(cursor)));
}

function isWhitespace(char: string): boolean {
    return char === ' ' || char === '\n' || char === '\t' || char === '\r';
}

function tokenStartForCursor(text: string, cursor: number): number {
    let index = cursor - 1;
    while (index >= 0 && !isWhitespace(text[index] ?? '')) {
        index -= 1;
    }
    return index + 1;
}

export function detectComposerTrigger(text: string, cursorInput: number): ComposerTrigger | null {
    const cursor = clampCursor(text, cursorInput);
    const lineStart = text.lastIndexOf('\n', Math.max(0, cursor - 1)) + 1;
    const linePrefix = text.slice(lineStart, cursor);

    if (linePrefix.startsWith('/')) {
        const commandMatch = /^\/(\S*)$/.exec(linePrefix);
        if (commandMatch) {
            const query = commandMatch[1] ?? '';
            if (SLASH_COMMANDS.some((command) => command.startsWith(query.toLowerCase()))) {
                return {
                    kind: 'slash-command',
                    query,
                    rangeStart: lineStart,
                    rangeEnd: cursor,
                };
            }
        }
    }

    const tokenStart = tokenStartForCursor(text, cursor);
    const token = text.slice(tokenStart, cursor);
    if (!token.startsWith('@')) {
        return null;
    }

    return {
        kind: 'path',
        query: token.slice(1),
        rangeStart: tokenStart,
        rangeEnd: cursor,
    };
}

export function replaceTextRange(
    text: string,
    rangeStart: number,
    rangeEnd: number,
    replacement: string,
): { text: string; cursor: number } {
    const safeStart = Math.max(0, Math.min(text.length, rangeStart));
    const safeEnd = Math.max(safeStart, Math.min(text.length, rangeEnd));
    const nextText = `${text.slice(0, safeStart)}${replacement}${text.slice(safeEnd)}`;
    return {
        text: nextText,
        cursor: safeStart + replacement.length,
    };
}
