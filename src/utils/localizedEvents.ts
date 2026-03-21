import i18n from '../i18n';
import type {
    ChatErrorPayload,
    ContextLengthExceededPayload,
    MessageTooLargePayload,
    StructuredAction,
} from '../types/events';

function translateWithFallback(
    key?: string | null,
    fallback?: string | null,
    params?: Record<string, string> | null,
): string {
    if (!key) {
        return fallback ?? '';
    }

    return i18n.t(key, {
        defaultValue: fallback ?? key,
        ...(params ?? {}),
    });
}

export function formatChatErrorPayload(payload: string | ChatErrorPayload): string {
    if (typeof payload === 'string') {
        return payload;
    }

    const summary = translateWithFallback(
        payload.i18nKey,
        payload.error ?? payload.message ?? payload.detail ?? null,
        payload.i18nParams,
    );
    const detail = payload.detail ?? payload.message ?? null;

    if (summary && detail && detail !== summary) {
        return `${summary}: ${detail}`;
    }

    return summary || detail || i18n.t('common.error', { defaultValue: 'Error' });
}

export function buildContextLengthSystemMessage(payload: ContextLengthExceededPayload): string {
    const tokenInfo = payload.token_count && payload.max_tokens
        ? ` (${payload.token_count.toLocaleString()} / ${payload.max_tokens.toLocaleString()} tokens)`
        : '';
    const title = translateWithFallback(
        payload.titleKey,
        'Context Limit Reached',
    );
    const recoveryText = payload.recoverable
        ? (payload.recovery_hint || translateWithFallback(
            payload.recoverableHintKey,
            'The AI is attempting to recover automatically. You can also try:\n- Starting a new conversation\n- Asking the AI to summarize the conversation',
        ))
        : translateWithFallback(
            payload.nonRecoverableHintKey,
            'Please start a new conversation to continue.',
        );

    return `⚠️ **${title}**${tokenInfo}\n\n${payload.message}\n\n${recoveryText}`;
}

export function buildMessageTooLargeSystemMessage(payload: MessageTooLargePayload): string {
    const title = translateWithFallback(
        payload.titleKey,
        'Response Too Large',
    );
    const hintLabel = translateWithFallback(
        payload.recoveryHintLabelKey,
        'Recovery hint',
    );

    return `⚠️ **${title}**\n\n${payload.message}\n\n**${hintLabel}:** ${payload.recovery_hint}`;
}

export function getStructuredActionDescription(action: StructuredAction): string {
    const translated = translateWithFallback(
        action.descriptionKey,
        null,
        action.descriptionParams,
    );
    if (translated) {
        return translated;
    }

    if (action.actionKind === 'command') {
        return i18n.t('approval.runCommandDescription', {
            command: action.command,
            defaultValue: `Run command: ${action.command}`,
        });
    }

    return action.description;
}
