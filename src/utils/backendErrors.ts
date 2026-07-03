import i18n from '../i18n';
import type { BladeError } from '../types/blade';

function extractStringMessage(value: unknown): string | null {
    if (typeof value === 'string') {
        return value;
    }
    if (value instanceof Error) {
        return value.message;
    }
    if (value && typeof value === 'object' && 'message' in value && typeof (value as { message?: unknown }).message === 'string') {
        return (value as { message: string }).message;
    }
    return null;
}

export function isBladeError(value: unknown): value is BladeError {
    if (!value || typeof value !== 'object') {
        return false;
    }
    const candidate = value as { code?: unknown; details?: unknown };
    return typeof candidate.code === 'string' && candidate.details !== undefined;
}

export function formatBackendMessage(message: string): string {
    const trimmed = message.trim();

    if (trimmed === 'No workspace open' || trimmed === 'No workspace is open') {
        return i18n.t('errors.noWorkspaceOpen', {
            defaultValue: 'No workspace is open.',
        });
    }

    if (trimmed === 'Not a Git repository') {
        return i18n.t('git.notRepository', {
            defaultValue: 'Not a Git repository.',
        });
    }

    if (trimmed === 'Commit message is required') {
        return i18n.t('git.commitMessageRequired', {
            defaultValue: 'Commit message is required.',
        });
    }

    if (trimmed === 'No model selected') {
        return i18n.t('git.noModelSelected', {
            defaultValue: 'Select a model in the Command Center before generating a commit message.',
        });
    }

    if (trimmed.startsWith('Model not found:')) {
        const model = trimmed.slice('Model not found:'.length).trim();
        return i18n.t('errors.modelNotFound', {
            model,
            defaultValue: `Model not found: ${model}`,
        });
    }

    if (trimmed.startsWith('Path is outside workspace')) {
        return i18n.t('errors.pathOutsideWorkspace', {
            defaultValue: 'Path is outside the current workspace.',
        });
    }

    return trimmed;
}

export function formatBladeError(error: BladeError): string {
    switch (error.code) {
        case 'ValidationError':
            return i18n.t('errors.invalidInput', {
                message: error.details.message,
                defaultValue: `Invalid input: ${error.details.message}`,
            });
        case 'PermissionDenied':
            return i18n.t('errors.permissionDeniedGeneric', {
                defaultValue: 'Permission denied.',
            });
        case 'ResourceNotFound':
            return i18n.t('errors.fileNotFound', {
                path: error.details.id,
                defaultValue: `File not found: ${error.details.id}`,
            });
        case 'Conflict':
            return i18n.t('errors.conflict', {
                reason: error.details.reason,
                defaultValue: `Conflict: ${error.details.reason}`,
            });
        case 'Internal':
            return formatBackendMessage(error.details.message);
        case 'VersionMismatch':
            return i18n.t('errors.versionMismatch', {
                expected: `${error.details.expected.major}.${error.details.expected.minor}.${error.details.expected.patch}`,
                received: `${error.details.received.major}.${error.details.received.minor}.${error.details.received.patch}`,
                defaultValue: 'Protocol version mismatch.',
            });
        case 'Timeout':
            return i18n.t('errors.timeout', {
                defaultValue: 'Operation timed out.',
            });
        case 'RateLimited':
            return i18n.t('errors.rateLimitExceeded', {
                defaultValue: 'Rate limit exceeded. Please try again later.',
            });
        default:
            return i18n.t('errors.unknownError', {
                defaultValue: 'An unknown error occurred.',
            });
    }
}

export function formatUnknownBackendError(error: unknown): string {
    if (isBladeError(error)) {
        return formatBladeError(error);
    }

    const message = extractStringMessage(error);
    if (message) {
        return formatBackendMessage(message);
    }

    return i18n.t('errors.unknownError', {
        defaultValue: 'An unknown error occurred.',
    });
}
