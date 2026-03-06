import React, { useEffect, useRef, useCallback, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { Check, X, Settings, Key, Loader2 } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../types/chat';

import type { StructuredAction, TodoItem } from '../types/events';
import type { RemoteAiConfig } from '../types/settings';
import { ChatMessage } from './ChatMessage';
import { ChatTabBar } from './ChatTabBar';
import { CommandCenter } from './CommandCenter';
import { HistoryTab } from './HistoryTab';
import { ProgressIndicator } from './ProgressIndicator';
import { GlobalChangeActions } from './editor/GlobalChangeActions';
import { TaskPanel } from './TaskPanel';
import { QueuePanel } from './QueuePanel';
import type { UncommittedChange } from '../types/uncommitted';

interface ResearchProgress {
    message: string;
    stage: string;
    percent: number;
    isActive: boolean;
}

interface ChatPanelProps {
    messages: ChatMessageType[];
    loading: boolean;
    error: string | null;
    sendMessage: (text: string, attachments?: ImageAttachment[]) => void;
    stopGeneration: () => void;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    pendingActions: StructuredAction[] | null;
    approveToolDecision: (decision: string) => void;
    skipSingleCommand: (callId: string) => void;
    projectId: string;
    onLoadConversation: (messages: ChatMessageType[]) => void;
    researchProgress?: ResearchProgress | null;
    onNewConversation: () => void;
    onUndoTool: (toolCallId: string) => void;
    uncommittedChanges: UncommittedChange[];
    onAcceptAllChanges: () => void;
    onRejectAllChanges: () => void;
    toolActivity?: ToolActivityState | null;
    activeTodos: TodoItem[];
    setActiveTodos: React.Dispatch<React.SetStateAction<TodoItem[]>>;
    queuedRequests: QueuedRequest[];
    deleteQueuedRequest: (index: number) => void;
}

const pendingResponseWords = [
    'Working...',
    'Thinking...',
    'Waiting...',
    'Ruminating...',
] as const;

const PendingResponseIndicator: React.FC = () => {
    const [wordIndex, setWordIndex] = useState(0);

    useEffect(() => {
        const intervalId = window.setInterval(() => {
            setWordIndex((prev) => (prev + 1) % pendingResponseWords.length);
        }, 6500);

        return () => window.clearInterval(intervalId);
    }, []);

    return (
        <div className="px-4 py-2">
            <div className="inline-flex items-center gap-2 rounded-md border border-(--border-subtle) bg-(--bg-surface)/70 px-3 py-1.5 text-[11px] font-medium text-(--fg-secondary)">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-(--fg-tertiary)" />
                <span key={pendingResponseWords[wordIndex]} className="pending-response-word">
                    {pendingResponseWords[wordIndex]}
                </span>
            </div>
        </div>
    );
};

const ChatPanelComponent: React.FC<ChatPanelProps> = ({
    messages,
    loading,
    error,
    sendMessage,
    stopGeneration,
    models,
    selectedModelId,
    setSelectedModelId,
    pendingActions,
    approveToolDecision,
    skipSingleCommand,
    projectId,
    onLoadConversation,
    researchProgress,
    onNewConversation,
    onUndoTool,
    uncommittedChanges,
    onAcceptAllChanges,
    onRejectAllChanges,
    toolActivity,
    activeTodos,
    setActiveTodos,
    queuedRequests,
    deleteQueuedRequest,
}) => {
    const { t } = useTranslation();
    const { stopCommandExecution } = useCommandExecution();
    const [taskPanelCollapsed, setTaskPanelCollapsed] = useState(false);
    const { loadConversation } = useHistory();
    const isUserAtBottomRef = useRef(true);
    const prevMessageCountRef = useRef(0);
    const [activeTab, setActiveTab] = useState<'chat' | 'history'>('chat');
    const [hasApiKey, setHasApiKey] = useState<boolean>(true);
    const [composerPrefill, setComposerPrefill] = useState<QueuedRequest | null>(null);

    // Check API Key
    const checkApiKey = useCallback(async () => {
        try {
            const config = await invoke<RemoteAiConfig>('get_remote_ai_settings');
            setHasApiKey(!!config.api_key && config.api_key.length > 0);
        } catch (e) {
            console.error('Failed to check API key:', e);
        }
    }, []);

    useEffect(() => {
        checkApiKey();
        const unlistenPromise = listen('global-settings-changed', checkApiKey);
        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [checkApiKey]);

    // Track visible range for virtualization
    const [visibleRange, setVisibleRange] = useState({ start: 0, end: 50 });
    const scrollContainerRef = useRef<HTMLDivElement>(null);

    const lastMessage = messages[messages.length - 1];
    const shouldShowPendingResponseIndicator = loading && lastMessage?.role !== 'Assistant';
    const streamingSignature = useMemo(() => {
        if (!lastMessage) return '';
        return [
            lastMessage.id ?? messages.length,
            lastMessage.content ?? '',
            lastMessage.reasoning ?? '',
            lastMessage.blocks?.length ?? 0,
        ].join('|');
    }, [lastMessage, messages.length]);

    const scrollToBottom = useCallback(() => {
        const container = scrollContainerRef.current;
        if (!container) return;
        container.scrollTop = container.scrollHeight;
    }, []);

    // Scroll to bottom when a different conversation is loaded (or on initial mount).
    // Detect conversation change by tracking the first message's ID.
    const prevFirstMsgIdRef = useRef<string | undefined>(undefined);
    useEffect(() => {
        const firstId = messages[0]?.id;
        if (messages.length > 0 && firstId !== prevFirstMsgIdRef.current) {
            prevFirstMsgIdRef.current = firstId;
            // Reset "at bottom" tracking so streaming auto-scroll works for the new conversation
            isUserAtBottomRef.current = true;
            prevMessageCountRef.current = messages.length;
            // Delay to ensure DOM has rendered the loaded messages
            const timer = setTimeout(scrollToBottom, 50);
            return () => clearTimeout(timer);
        }
    }, [messages, scrollToBottom]);

    // Scroll when a new message is appended (or user sends a message)
    useEffect(() => {
        const currentCount = messages.length;

        if (currentCount === prevMessageCountRef.current) {
            return;
        }

        prevMessageCountRef.current = currentCount;
        const lastMsg = messages[currentCount - 1];
        const justSent = lastMsg?.role === 'User';

        if (justSent || isUserAtBottomRef.current) {
            const rafId = requestAnimationFrame(scrollToBottom);
            return () => cancelAnimationFrame(rafId);
        }

        return;
    }, [messages.length, messages, scrollToBottom]);

    // Scroll during streaming updates only while user stays at bottom
    useEffect(() => {
        if (!loading || !isUserAtBottomRef.current) return;
        const rafId = requestAnimationFrame(scrollToBottom);
        return () => cancelAnimationFrame(rafId);
    }, [loading, streamingSignature, scrollToBottom]);

    // Prevent default context menu on empty areas
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        // Always prevent default to avoid native Tauri menu
        e.preventDefault();
    }, []);

    // Scroll handler - memoized to prevent recreation on every render
    const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
        const target = e.target as HTMLDivElement;
        // Use a larger threshold (100px) to be more resilient to large appends (e.g. code blocks)
        const isBottom = Math.abs(target.scrollHeight - target.scrollTop - target.clientHeight) < 100;
        isUserAtBottomRef.current = isBottom;
        
        // Update visible range for virtualization (throttled)
        // Estimate ~150px per message on average
        const estimatedMessageHeight = 150;
        const scrollTop = target.scrollTop;
        const viewportHeight = target.clientHeight;
        const buffer = 5; // Render 5 extra messages above/below viewport
        
        const startIdx = Math.max(0, Math.floor(scrollTop / estimatedMessageHeight) - buffer);
        const endIdx = Math.ceil((scrollTop + viewportHeight) / estimatedMessageHeight) + buffer;
        
        setVisibleRange(prev => {
            // Only update if significantly different to avoid excessive re-renders
            if (Math.abs(prev.start - startIdx) > 2 || Math.abs(prev.end - endIdx) > 2) {
                return { start: startIdx, end: endIdx };
            }
            return prev;
        });
    }, []);
    
    // Compute which messages to render (virtualization)
    // Always render last 10 messages + messages in visible range
    const messagesToRender = useMemo(() => {
        const totalMessages = messages.length;
        if (totalMessages <= 20) {
            // Small conversation - render all
            return messages.map((msg, idx) => ({ msg, idx, isPlaceholder: false }));
        }
        
        const result: { msg: ChatMessageType | null; idx: number; isPlaceholder: boolean }[] = [];
        const lastMessagesStart = Math.max(0, totalMessages - 10);
        
        for (let i = 0; i < totalMessages; i++) {
            const inVisibleRange = i >= visibleRange.start && i <= visibleRange.end;
            const inLastMessages = i >= lastMessagesStart;
            
            if (inVisibleRange || inLastMessages) {
                result.push({ msg: messages[i], idx: i, isPlaceholder: false });
            } else {
                // Placeholder for virtualized message
                result.push({ msg: null, idx: i, isPlaceholder: true });
            }
        }
        
        return result;
    }, [messages, visibleRange]);

    const handleNewConversation = () => {
        onNewConversation();
        setActiveTab('chat');
    };

    const handleSelectConversation = useCallback(async (sessionId: string) => {
        try {
            const conversationMessages = await loadConversation(sessionId);
            onLoadConversation(conversationMessages);
            setActiveTab('chat');
        } catch (e) {
            console.error('Failed to load conversation:', e);
        }
    }, [loadConversation, onLoadConversation]);

    // Stable callback references for ChatMessage - prevents re-renders
    const handleApproveCommand = useCallback(() => {
        approveToolDecision('approve_once');
    }, [approveToolDecision]);

    const handleSkipCommand = useCallback(() => {
        approveToolDecision('reject');
    }, [approveToolDecision]);

    // Individual command approval/skip handlers
    const handleApproveSingleCommand = useCallback((callId: string) => {
        invoke('approve_single_command', { callId, approved: true });
    }, []);

    const handleSkipSingleCommand = useCallback((callId: string) => {
        skipSingleCommand(callId);
    }, [skipSingleCommand]);

    const handleStopCommand = useCallback((callId: string) => {
        stopCommandExecution(callId);
    }, [stopCommandExecution]);

    const handleEditQueuedRequest = useCallback((index: number) => {
        const request = queuedRequests[index];
        if (!request) return;
        setComposerPrefill(request);
        deleteQueuedRequest(index);
    }, [queuedRequests, deleteQueuedRequest]);

    const handleComposerPrefillConsumed = useCallback(() => {
        setComposerPrefill(null);
    }, []);

    return (
        <div className="flex flex-col h-full bg-[var(--bg-app)] text-[var(--fg-primary)] font-sans tracking-tight" onContextMenu={handleContextMenu}>
            {/* Tab Bar */}
            <ChatTabBar
                activeTab={activeTab}
                onTabChange={setActiveTab}
                onNewConversation={handleNewConversation}
            />

            {/* Content Area - conditionally render based on active tab */}
            {activeTab === 'chat' ? (
                <div
                    ref={scrollContainerRef}
                    className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent"
                    onScroll={handleScroll}
                >
                    <div className="max-w-4xl mx-auto py-6">
                        {messages.length === 0 && (
                            <div className="flex flex-col items-center justify-center p-12 text-[var(--fg-tertiary)] text-center space-y-4 select-none">
                                <div className="text-4xl opacity-20 filter grayscale">
                                    🗡️
                                </div>
                                <h2 className="text-sm font-medium text-[var(--fg-secondary)] tracking-wide uppercase">System Ready</h2>
                                <p className="max-w-xs text-xs font-mono opacity-50">
                                    Awaiting input. Surgical precision engaged.
                                </p>
                            </div>
                        )}

                        {messagesToRender.map(({ msg, idx, isPlaceholder }) => {
                            // Render placeholder for virtualized messages
                            if (isPlaceholder || !msg) {
                                return (
                                    <div 
                                        key={`placeholder-${idx}`} 
                                        className="h-[100px]"
                                        aria-hidden="true"
                                    />
                                );
                            }
                            
                            // Show pending actions on the last assistant message
                            const isLast = idx === messages.length - 1;
                            const isLastAssistant = isLast && msg.role === 'Assistant';
                            const showPendingActions = isLastAssistant && pendingActions && pendingActions.length > 0;

                            // Calculate visual grouping props
                            const prevMsg = idx > 0 ? messages[idx - 1] : null;

                            // Treat "Tool" messages (if any visible) as part of Assistant flow if previous was Assistant
                            // Currently we hide Tool messages in ChatMessage, but if they were shown or if we have
                            // consecutive Assistant messages (split reasoning/tool calls), we group them.
                            const isAssistant = msg.role === 'Assistant';
                            const prevWasAssistant = prevMsg?.role === 'Assistant';

                            // Simple grouping: If current is assistant and previous was assistant
                            const isContinued = isAssistant && prevWasAssistant;

                            // Determine if this message is actively streaming/reasoning
                            // We assume the last message is active if global loading state is true
                            const isActive = isLast && loading;

                            return (
                                <ChatMessage
                                    key={msg.id || idx}
                                    message={msg}
                                    pendingActions={showPendingActions ? pendingActions : undefined}
                                    onApproveCommand={showPendingActions ? handleApproveCommand : undefined}
                                    onSkipCommand={showPendingActions ? handleSkipCommand : undefined}
                                    onApproveSingleCommand={showPendingActions ? handleApproveSingleCommand : undefined}
                                    onSkipSingleCommand={showPendingActions ? handleSkipSingleCommand : undefined}
                                    isContinued={isContinued}
                                    isActive={isActive}
                                    onUndoTool={onUndoTool}
                                    onStopCommand={handleStopCommand}
                                />
                            );
                        })}

                        {shouldShowPendingResponseIndicator && <PendingResponseIndicator />}

                        {/* Research progress indicator */}
                        {loading && researchProgress?.isActive && (
                            <div className="px-4">
                                <ProgressIndicator progress={researchProgress} />
                            </div>
                        )}

                        {/* Tool activity indicator - shows streaming tool progress, styled like ToolCallDisplay */}
                        {toolActivity && (() => {
                            const prettyToolNames: Record<string, string> = {
                                'write_file': 'Writing File',
                                'read_file': 'Reading File',
                                'apply_patch': 'Applying Code Changes',
                                'create_file': 'Creating File',
                                'edit_file': 'Editing File',
                                'delete_file': 'Deleting File',
                                'execute_command': 'Running Command',
                                'run_command': 'Running Command',
                                'search_files': 'Searching Code',
                                'list_files': 'Listing Files',
                                'grep_search': 'Searching Code',
                                'find_by_name': 'Finding Files',
                                'multi_edit': 'Multi-Edit File',
                                'list_dir': 'Listing Directory',
                                'list_directory': 'Listing Directory',
                                'codebase_search': 'Searching Codebase',
                                'get_workspace_structure': 'Analyzing Workspace',
                                'view_file': 'Viewing File',
                                'replace_file_content': 'Replacing Content',
                                'multi_replace_file_content': 'Multi-Edit File',
                                'write_to_file': 'Writing to File',
                            };
                            const writingTools = new Set([
                                'write_file',
                                'apply_patch',
                                'create_file',
                                'edit_file',
                                'delete_file',
                                'multi_edit',
                                'replace_file_content',
                                'multi_replace_file_content',
                                'write_to_file',
                            ]);
                            const isWriteTool = writingTools.has(toolActivity.toolName);
                            const isStreaming = toolActivity.action === 'streaming';
                            if (!isWriteTool || !isStreaming) {
                                return null;
                            }

                            const prettyName = prettyToolNames[toolActivity.toolName] || toolActivity.toolName;
                            const displayPath = toolActivity.filePath.split('/').pop() || toolActivity.filePath;
                            return (
                                <div className="px-4">
                                    <div className="flex items-center gap-2 py-1 text-[11px] text-zinc-500">
                                        <Loader2 className="w-3.5 h-3.5 text-emerald-400 animate-spin" />
                                        <span className="font-medium text-zinc-400">
                                            {prettyName}
                                        </span>
                                        {displayPath && (
                                            <span
                                                className="text-[10px] text-zinc-500 truncate max-w-[260px]"
                                                title={toolActivity.filePath}
                                            >
                                                {displayPath}
                                            </span>
                                        )}
                                        {toolActivity.chunkCount > 0 && (
                                            <span className="text-[10px] font-medium text-emerald-400">
                                                {toolActivity.chunkCount} chunks
                                            </span>
                                        )}
                                    </div>
                                </div>
                            );
                        })()}

                        {error && (
                            <div className="p-3 mx-4 mb-4 bg-red-500/5 border border-red-500/20 text-red-400 rounded-sm text-xs font-mono">
                                ERR: {error}
                            </div>
                        )}

                        <div className="h-4" />
                    </div>
                </div>
            ) : (
                <HistoryTab
                    projectId={projectId}
                    onSelectConversation={handleSelectConversation}
                />
            )}

            {/* Global Accept/Reject All Changes - show immediately when changes exist */}
            <GlobalChangeActions
                changes={uncommittedChanges}
                onAcceptAll={onAcceptAllChanges}
                onRejectAll={onRejectAllChanges}
            />

            {/* Task Panel - persistent TODO above Command Center */}
            <TaskPanel
                todos={activeTodos}
                isCollapsed={taskPanelCollapsed}
                onToggleCollapse={() => setTaskPanelCollapsed(prev => !prev)}
            />

            <QueuePanel
                requests={queuedRequests}
                onEditRequest={handleEditQueuedRequest}
                onDeleteRequest={deleteQueuedRequest}
            />

            <CommandCenter
                onSend={sendMessage}
                onStop={stopGeneration}
                loading={loading}
                models={models}
                selectedModelId={selectedModelId}
                setSelectedModelId={setSelectedModelId}
                disabled={!hasApiKey}
                prefillRequest={composerPrefill}
                onPrefillConsumed={handleComposerPrefillConsumed}
            />

            {/* API Key Missing Overlay */}
            {!hasApiKey && (
                <div className="absolute inset-x-0 bottom-[140px] top-0 bg-black/60 backdrop-blur-sm flex flex-col items-center justify-center p-6 z-20 text-center animate-in fade-in duration-300">
                    <div className="bg-[var(--bg-surface)] border border-[var(--border-subtle)] p-6 rounded-xl shadow-2xl max-w-sm w-full">
                        <div className="w-12 h-12 bg-amber-500/10 rounded-full flex items-center justify-center mx-auto mb-4">
                            <Key className="w-6 h-6 text-amber-500" />
                        </div>
                        <h3 className="text-lg font-semibold text-[var(--fg-primary)] mb-2">Setup Required</h3>
                        <p className="text-sm text-[var(--fg-secondary)] mb-6">
                            To use the AI Assistant, you need to configure your Zaguán API Key.
                        </p>
                        <button
                            onClick={() => {
                                // Dispatch event to open settings
                                // Since we don't have direct access to setIsSettingsOpen, we can dispatch a custom event
                                // or rely on the user clicking the gear icon.
                                // But for better UX, let's try to emit an event Layout listens to?
                                // Layout listens for 'open-settings' maybe?
                                // For now, we'll suggest using the gear icon if we can't trigger it.
                                document.dispatchEvent(new CustomEvent('open-settings'));
                            }}
                            className="w-full py-2.5 px-4 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg font-medium transition-colors flex items-center justify-center gap-2"
                        >
                            <Settings className="w-4 h-4" />
                            Open Settings
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
};

export const ChatPanel = React.memo(ChatPanelComponent);
