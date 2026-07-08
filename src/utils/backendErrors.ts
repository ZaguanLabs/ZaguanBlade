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

// Backend strings stay in English in Rust; the tables below map them to i18n
// keys. Exact entries match the whole trimmed message. Prefix entries capture
// a single trailing dynamic part into the named interpolation variable.
// Pattern entries capture multiple dynamic parts via anchored regexes.
const EXACT_BACKEND_MESSAGE_KEYS = new Map<string, string>([
    // Connection / WebSocket
    ['Not connected', 'errors.notConnected'],
    ['Failed to send authentication', 'errors.sendAuthFailed'],
    ['WebSocket authentication timed out', 'errors.wsAuthTimeout'],
    ['WebSocket disconnected before authentication', 'errors.wsDisconnectedBeforeAuth'],
    ['WebSocket closed before authentication', 'errors.wsClosedBeforeAuth'],
    ['WebSocket disconnected while waiting for history list response', 'errors.wsDisconnectedHistoryList'],
    ['WebSocket closed while waiting for history list response', 'errors.wsClosedHistoryList'],
    ['Timed out waiting for history list response', 'errors.historyListTimeout'],
    ['WebSocket disconnected while waiting for history detail response', 'errors.wsDisconnectedHistoryDetail'],
    ['WebSocket closed while waiting for history detail response', 'errors.wsClosedHistoryDetail'],
    ['Timed out waiting for history detail response', 'errors.historyDetailTimeout'],
    ['ZLP messages are not supported when a local model is active', 'errors.zlpLocalModelUnsupported'],

    // Chat system + research messages
    ['The response was too large to process. Please break your response into smaller parts or use more concise output.', 'chat.system.responseTooLargeBody'],
    ['Your previous response exceeded the message size limit. Please retry with a more concise approach: use smaller code blocks, avoid outputting entire files, and break large changes into multiple smaller tool calls.', 'chat.system.responseTooLargeRecoveryHint'],
    ['Please use smaller responses and break large changes into multiple tool calls.', 'chat.system.responseTooLargeHint'],
    ['Context limit reached during generation', 'chat.system.contextLimitDuringGeneration'],
    ['Server disconnected while generating. Reconnect to zcoderd and retry when it is available.', 'chat.system.disconnectedWhileGenerating'],
    ['Server disconnected before a response was received. Reconnect to zcoderd and retry when it is available.', 'chat.system.disconnectedBeforeResponse'],
    ['[no content]', 'chat.system.noContent'],
    ['Please wait a moment and try again.', 'errors.waitAndRetry'],
    ['Research Results', 'chat.researchResultsTitle'],
    ['Generated', 'chat.research.generated'],
    ['Thinking', 'chat.research.thinking'],
    ['Complete', 'chat.research.completeLabel'],
    ['Disconnected', 'chat.research.disconnected'],
    ['✅ **Research complete!**\n\n📄 Results opened in new editor tab above.', 'chat.research.complete'],
    ['Server disconnected before this tool could run.', 'toolCall.results.disconnectedBeforeRun'],

    // Conversations / documents / history
    ['New Conversation', 'chat.newConversation'],
    ['Conversation store failed to initialize', 'errors.conversationStoreInit'],
    ['Document not found', 'errors.documentNotFound'],
    ['Snapshot not found', 'errors.snapshotNotFound'],

    // Workspace / models
    ['path (parent) is outside workspace', 'errors.pathParentOutsideWorkspace'],
    ['Ollama authentication failed (401). Check your cloud API key in Settings.', 'errors.ollamaAuthFailed'],
    ['OpenAI-compatible', 'chat.modelPicker.openaiCompat'],

    // Git
    ['No staged changes to commit', 'git.errors.noStagedChanges'],
    ['Warning: HEAD is detached. Commits may be lost.', 'git.errors.detachedHeadCommit'],
    ['Cannot push without an active branch', 'git.errors.pushNoBranch'],
    ['Cannot push because no Git remote is configured', 'git.errors.pushNoRemote'],
    ['No changes to commit', 'git.errors.noChangesToCommit'],
    ['AI returned empty response', 'git.errors.aiEmptyResponse'],
    ['Timed out waiting for AI commit message', 'git.errors.commitMessageTimeout'],

    // Code intelligence / symbol index
    ['Code intelligence status unknown', 'statusBar.index.statusUnknown'],
    ['Code intelligence ready', 'statusBar.index.ready'],
    ['Checking symbol index', 'statusBar.index.checkingSymbolIndex'],
    ['Resolving symbol relationships...', 'statusBar.index.resolvingRelationships'],
    ['Resolving cross-file method calls...', 'statusBar.index.resolvingCrossFile'],

    // Remote control (Telegram)
    ['Bot token cannot be empty', 'settings.remote.errors.emptyToken'],
    ['Invalid bot token', 'settings.remote.errors.invalidToken'],

    // SSO
    ['A Zaguán sign-in is already in progress.', 'settings.account.sso.alreadyInProgress'],
    ['Zaguán sign-in expired. Start a new sign-in request.', 'settings.account.sso.expired'],
    ['Sign-in cancelled.', 'settings.account.ssoStatus.cancelled'],
    ['Zaguán sign-in cancelled.', 'settings.account.sso.cancelledError'],
    ['Finish subscription checkout in the browser, then approve this device.', 'settings.account.sso.pendingSubscriptionMessage'],
    ['Zaguán sign-in was denied in the browser.', 'settings.account.sso.denied'],
    ['This Zaguán sign-in was already used. Start a new request.', 'settings.account.sso.consumed'],
    ['Approved sign-in did not include an API key.', 'settings.account.sso.missingApiKey'],
    ['Approved sign-in did not include a user ID.', 'settings.account.sso.missingUserId'],
    ['Approved sign-in did not include an email address.', 'settings.account.sso.missingEmail'],
]);

const PREFIX_BACKEND_MESSAGES: Array<[prefix: string, key: string, param: string]> = [
    // AI workflow / tool results
    ['Create directory: ', 'approval.createDirectory', 'path'],
    ['Change applied to ', 'toolCall.results.changeApplied', 'path'],
    ['File deleted: ', 'toolCall.results.fileDeleted', 'path'],
    ['Action proposed: ', 'toolCall.results.actionProposed', 'description'],
    ['Failed to apply change: ', 'errors.applyChangeFailed', 'error'],
    ['Failed to read file: ', 'errors.readFileFailed', 'error'],
    ['Failed to write file: ', 'errors.writeFileFailed', 'error'],
    ['Failed to create file: ', 'errors.createFileFailed', 'error'],
    ['Failed to delete file: ', 'errors.deleteFileFailed', 'error'],

    // WebSocket / chat transport
    ['Read error: ', 'errors.readError', 'error'],
    ['Failed to send message: ', 'errors.sendMessageFailed', 'error'],
    ['Failed to send approval response: ', 'errors.sendApprovalFailed', 'error'],
    ['Failed to send context pack response: ', 'errors.sendContextPackFailed', 'error'],
    ['Failed to send conversation context: ', 'errors.sendConversationContextFailed', 'error'],
    ['Authentication failed: ', 'errors.authenticationFailedDetail', 'error'],

    // Local model providers
    ['Failed to connect to Ollama: ', 'errors.ollamaConnectFailed', 'error'],
    ['Ollama request failed: ', 'errors.ollamaRequestFailed', 'error'],
    ['Ollama retry without tools failed: ', 'errors.ollamaRetryFailed', 'error'],
    ['Ollama stream error: ', 'errors.ollamaStreamError', 'error'],
    ['Ollama response decode error: ', 'errors.ollamaDecodeError', 'error'],
    ['Ollama error: ', 'errors.ollamaError', 'error'],
    ['Failed to connect to OpenAI-compatible server: ', 'errors.openaiCompatConnectFailed', 'error'],
    ['Retry without tools failed for OpenAI-compatible server: ', 'errors.openaiCompatRetryFailed', 'error'],
    ['OpenAI-compatible request failed: ', 'errors.openaiCompatRequestFailed', 'error'],
    ['Stream error: ', 'errors.streamError', 'error'],
    ['AI generation failed: ', 'errors.aiGenerationFailed', 'error'],

    // Conversations / history / uncommitted changes
    ['Failed to read conversation: ', 'errors.readConversationFailed', 'error'],
    ['No history entries found for group ID: ', 'errors.noHistoryForGroup', 'id'],
    ['Change not found: ', 'errors.changeNotFound', 'id'],
    ['No uncommitted change for file: ', 'errors.noUncommittedChange', 'path'],

    // Git (specific prefixes must run before the generic git patterns below)
    ['Failed to discover git repository: ', 'git.errors.discoverFailed', 'error'],
    ['failed to run git status: ', 'git.errors.statusRunFailed', 'error'],
    ['git status failed: ', 'git.errors.statusFailed', 'error'],
    ['failed to run git diff: ', 'git.errors.diffRunFailed', 'error'],
    ['git diff failed: ', 'git.errors.diffFailed', 'error'],
    ['Failed to walk commits: ', 'gitGraph.errors.walkFailed', 'error'],

    // Code intelligence
    ['Code intelligence refresh failed: ', 'statusBar.index.refreshFailed', 'error'],

    // Project storage setup
    ['Failed to create .gitignore: ', 'storageSetup.errors.createGitignoreFailed', 'error'],
    ['Failed to serialize settings: ', 'storageSetup.errors.serializeFailed', 'error'],
    ['Failed to write settings: ', 'storageSetup.errors.writeFailed', 'error'],
    ['Failed to create parent directories: ', 'errors.createParentDirsFailed', 'error'],

    // Remote control (Telegram)
    ['Failed to reach Telegram: ', 'settings.remote.errors.telegramUnreachable', 'error'],
    ['Failed to parse Telegram response: ', 'settings.remote.errors.telegramParseFailed', 'error'],

    // SSO
    ['Failed to initialize sign-in client: ', 'settings.account.sso.clientInitFailed', 'error'],
    ['Failed to start sign-in: ', 'settings.account.sso.startFailed', 'error'],
    ['Failed to read sign-in response: ', 'settings.account.sso.readResponseFailed', 'error'],
    ['Failed to poll sign-in: ', 'settings.account.sso.pollFailed', 'error'],
    ['Failed to read sign-in poll response: ', 'settings.account.sso.pollReadFailed', 'error'],
    ['Unexpected Zaguán sign-in status: ', 'settings.account.sso.unexpectedStatus', 'status'],
];

const PATTERN_BACKEND_MESSAGES: Array<[pattern: RegExp, key: string, params: string[]]> = [
    // AI workflow approvals / patches
    [/^Move ([\s\S]+) to ([\s\S]+)$/, 'approval.moveFile', ['source', 'destination']],
    [/^Copy ([\s\S]+) to ([\s\S]+)$/, 'approval.copyFile', ['source', 'destination']],
    [/^Patch (.+?) failed \(no changes made\): ([\s\S]*)$/, 'errors.patchFailed', ['index', 'error']],

    // WebSocket / chat transport
    [/^WebSocket connection failed after (\d+) retries: ([\s\S]*)$/, 'errors.wsConnectionFailedRetries', ['count', 'error']],
    [/^Failed to send tool result for (.+?): ([\s\S]*)$/, 'errors.sendToolResultFailed', ['tool', 'error']],

    // Conversations
    [/^Cannot truncate conversation to (\d+) messages; current length is (\d+)$/, 'errors.truncateConversation', ['count', 'length']],
    [/^Conversation (.+) not found$/, 'errors.conversationNotFound', ['id']],

    // Local model providers (HTTP variants before the generic ones)
    [/^Ollama returned HTTP (.+?): ([\s\S]*)$/, 'errors.ollamaHttpError', ['status', 'body']],
    [/^Ollama returned (.+?): ([\s\S]*)$/, 'errors.ollamaReturned', ['status', 'body']],
    [/^OpenAI-compatible server returned HTTP (.+?): ([\s\S]*)$/, 'errors.openaiCompatServerReturnedHttp', ['status', 'body']],
    [/^OpenAI-compatible server returned (.+?): ([\s\S]*)$/, 'errors.openaiCompatServerReturned', ['status', 'body']],
    [/^Server returned (.+?): ([\s\S]*)$/, 'errors.serverReturned', ['status', 'body']],
    [/^OpenAI-compatible \((.+)\)$/, 'chat.modelPicker.openaiCompatWith', ['ownedBy']],

    // SSO
    [/^Waiting (\d+) seconds before retrying\.$/, 'settings.account.sso.rateLimitWait', ['seconds']],
    [/^Sign-in start failed \((.+?)\): ([\s\S]*)$/, 'settings.account.sso.startHttpFailed', ['status', 'body']],
    [/^Sign-in poll failed \((.+?)\): ([\s\S]*)$/, 'settings.account.sso.pollHttpFailed', ['status', 'body']],

    // Git (specific prefix matches above run first)
    [/^failed to run git (.+?): ([\s\S]*)$/, 'git.errors.runFailed', ['args', 'error']],
    [/^git (.+?) failed: ([\s\S]*)$/, 'git.errors.commandFailed', ['args', 'error']],
    [/^AI returned empty commit message([\s\S]*)$/, 'git.errors.emptyCommitMessage', ['suffix']],
    [/^(\d+) seconds ago$/, 'gitGraph.relativeDate.secondsAgo', ['count']],
    [/^Update (\d+) files$/, 'git.fallbackCommitMessageMany', ['count']],
    [/^Update ([\s\S]+)$/, 'git.fallbackCommitMessage', ['files']],

    // Project storage setup
    [/^Failed to create directory ([\s\S]+?): ([\s\S]*)$/, 'storageSetup.errors.createDirFailed', ['path', 'error']],

    // Code intelligence / symbol index
    [/^Code intelligence partial: graph integrity issues detected \((\d+) missing sources, (\d+) missing targets, (\d+) files missing roots\)$/, 'statusBar.index.partialIntegrity', ['missingSources', 'missingTargets', 'filesMissingRoots']],
    [/^Code intelligence partial: (\d+) files pending$/, 'statusBar.index.partialPending', ['count']],
    [/^Refreshing code intelligence: (\d+) files pending$/, 'statusBar.index.refreshingPending', ['count']],
    [/^Building symbol index\.\.\. (\d+)\/(\d+) files$/, 'statusBar.index.building', ['done', 'total']],
    [/^Indexing ([\s\S]+)\.\.\. (\d+)\/(\d+) files \((\d+) workers\)$/, 'statusBar.index.indexingDetailed', ['file', 'done', 'total', 'workers']],
    [/^Retrying (\d+) deferred file\(s\)\.\.\.$/, 'statusBar.index.retryingDeferred', ['count']],
    [/^Rebuilding symbol index after graph integrity issues \((\d+) missing sources, (\d+) missing targets, (\d+) files missing roots\)$/, 'statusBar.index.rebuildingIntegrity', ['missingSources', 'missingTargets', 'filesMissingRoots']],
    [/^Code intelligence ready: (\d+)\/(\d+) symbol relationships resolved$/, 'statusBar.index.readyResolved', ['resolved', 'total']],

    // AI tool status lines (xml_parser)
    [/^📖 Reading `([\s\S]+)`\.\.\.$/, 'chat.status.reading', ['path']],
    [/^✍️ Writing to `([\s\S]+)`\.\.\.$/, 'chat.status.writing', ['path']],
    [/^✏️ Editing `([\s\S]+)`\.\.\.$/, 'chat.status.editing', ['path']],
    [/^📂 Listing `([\s\S]+)`\.\.\.$/, 'chat.status.listing', ['path']],
    [/^🔍 Searching for `([\s\S]+)`\.\.\.$/, 'chat.status.searching', ['pattern']],
    [/^⚙️ Running `([\s\S]+)`\.\.\.$/, 'chat.status.running', ['command']],
    [/^🔧 Using tool `([\s\S]+)`\.\.\.$/, 'chat.status.usingTool', ['tool']],
];

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

    // Existing keys whose value differs slightly from the backend literal.
    if (trimmed === 'Resolve merge conflicts before committing') {
        return i18n.t('git.resolveConflictsBeforeCommit', {
            defaultValue: 'Resolve merge conflicts before committing.',
        });
    }

    if (trimmed === 'Finalizing index...') {
        return i18n.t('statusBar.index.finalizing', {
            defaultValue: 'Finalizing index…',
        });
    }

    const exactKey = EXACT_BACKEND_MESSAGE_KEYS.get(trimmed);
    if (exactKey) {
        return i18n.t(exactKey, { defaultValue: trimmed });
    }

    for (const [prefix, key, param] of PREFIX_BACKEND_MESSAGES) {
        if (trimmed.startsWith(prefix)) {
            return i18n.t(key, {
                [param]: trimmed.slice(prefix.length).trim(),
                defaultValue: trimmed,
            });
        }
    }

    for (const [pattern, key, params] of PATTERN_BACKEND_MESSAGES) {
        const match = pattern.exec(trimmed);
        if (match) {
            const options: Record<string, string> = { defaultValue: trimmed };
            params.forEach((param, index) => {
                options[param] = match[index + 1] ?? '';
            });
            return i18n.t(key, options);
        }
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
