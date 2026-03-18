import type { ChatMessage, MessageBlock } from '../types/chat';

export function findLastActivityBlockIndex(blocks: MessageBlock[]): number {
    for (let index = blocks.length - 1; index >= 0; index -= 1) {
        const block = blocks[index];
        if (block.type === 'tool_call' || block.type === 'command_execution') {
            return index;
        }
    }

    return -1;
}

export function insertToolCallBlockPreservingOrder(blocks: MessageBlock[], toolCallId: string): MessageBlock[] {
    if (blocks.some((block) => block.type === 'tool_call' && block.id === toolCallId)) {
        return blocks;
    }

    const nextBlocks = [...blocks];
    const matchingExecutionIndex = nextBlocks.findIndex((block) => block.type === 'command_execution' && block.id === toolCallId);
    if (matchingExecutionIndex >= 0) {
        nextBlocks.splice(matchingExecutionIndex, 0, { type: 'tool_call', id: toolCallId });
        return nextBlocks;
    }

    nextBlocks.push({ type: 'tool_call', id: toolCallId });
    return nextBlocks;
}

export function upsertSplitTextBlocks(
    blocks: MessageBlock[],
    beforeToolsContent: string,
    afterToolsContent: string,
): MessageBlock[] {
    const firstActivityIndex = blocks.findIndex((block) => block.type === 'tool_call' || block.type === 'command_execution');
    if (firstActivityIndex === -1) {
        return blocks;
    }

    const lastActivityIndex = findLastActivityBlockIndex(blocks);
    const prefix = blocks.slice(0, firstActivityIndex);
    const activity = blocks.slice(firstActivityIndex, lastActivityIndex + 1);
    const suffix = blocks.slice(lastActivityIndex + 1);

    const prefixWithoutText = prefix.filter((block) => block.type !== 'text');
    const suffixWithoutText = suffix.filter((block) => block.type !== 'text');

    return [
        ...prefixWithoutText,
        ...(beforeToolsContent.length > 0
            ? [{ type: 'text' as const, content: beforeToolsContent, id: crypto.randomUUID() }]
            : []),
        ...activity,
        ...(afterToolsContent.length > 0
            ? [{ type: 'text' as const, content: afterToolsContent, id: crypto.randomUUID() }]
            : []),
        ...suffixWithoutText,
    ];
}

export function insertAssistantMessageAfterLastUser(messages: ChatMessage[], message: ChatMessage): ChatMessage[] {
    const lastUserIndex = messages.map((item) => item.role).lastIndexOf('User');
    if (lastUserIndex === -1) {
        return [...messages, message];
    }

    const nextMessages = [...messages];
    nextMessages.splice(lastUserIndex + 1, 0, message);
    return nextMessages;
}

/**
 * Reconstructs the blocks array for a message based on its content.
 * This is used when loading messages from history or the backend,
 * where blocks are not persisted.
 * 
 * The reconstruction follows a logical conversation flow order:
 * 1. Reasoning (if present)
 * 2. Content before tools (or main content if no tools)
 * 3. Tool calls (in order)
 * 4. Command executions (in order)
 * 5. TODOs (if present)
 * 6. Content after tools
 */
export function reconstructBlocks(message: ChatMessage): MessageBlock[] {
    // If blocks already exist and are populated, return them
    if (message.blocks && message.blocks.length > 0) {
        return message.blocks;
    }

    const blocks: MessageBlock[] = [];

    // 1. Add reasoning block if present
    if (message.reasoning && message.reasoning.trim()) {
        blocks.push({
            type: 'reasoning',
            content: message.reasoning,
            id: crypto.randomUUID()
        });
    }

    // 2. Add initial text content (content_before_tools or full content if no tool structure)
    const hasToolStructure = message.content_before_tools !== undefined || message.content_after_tools !== undefined;
    const initialContent = hasToolStructure
        ? (message.content_before_tools || '')
        : message.content;

    if (initialContent && initialContent.trim()) {
        blocks.push({
            type: 'text',
            content: initialContent,
            id: crypto.randomUUID()
        });
    }

    // 3. Add tool calls and their corresponding executions
    if (message.tool_calls && message.tool_calls.length > 0) {
        const addedExecIds = new Set<string>();

        for (const toolCall of message.tool_calls) {
            // Add the tool call block
            if (toolCall.function.name !== 'todo_write') {
                blocks.push({
                    type: 'tool_call',
                    id: toolCall.id
                });

                // Check if we have a matching command execution for this tool call
                const matchingExec = message.commandExecutions?.find(cmd => cmd.id === toolCall.id);
                if (matchingExec) {
                    blocks.push({
                        type: 'command_execution',
                        id: matchingExec.id
                    });
                    addedExecIds.add(matchingExec.id);
                }
            }
        }

        // 4. Add any remaining (orphaned) command execution blocks
        if (message.commandExecutions && message.commandExecutions.length > 0) {
            for (const cmd of message.commandExecutions) {
                if (!addedExecIds.has(cmd.id)) {
                    blocks.push({
                        type: 'command_execution',
                        id: cmd.id
                    });
                }
            }
        }
    } else if (message.commandExecutions && message.commandExecutions.length > 0) {
        // Fallback: No tool calls, but we have executions (unlikely in valid protocol flow)
        for (const cmd of message.commandExecutions) {
            blocks.push({
                type: 'command_execution',
                id: cmd.id
            });
        }
    }

    // 5. Add TODO block if todos are present (legacy backward compat)
    if (message.todos && message.todos.length > 0) {
        blocks.push({
            type: 'todo',
            id: crypto.randomUUID()
        });
    }

    // 5b. Add plan_summary block if planSummary data is present
    if (message.planSummary) {
        blocks.push({
            type: 'plan_summary',
            id: crypto.randomUUID()
        });
    }

    // 6. Add content after tools
    if (hasToolStructure && message.content_after_tools && message.content_after_tools.trim()) {
        blocks.push({
            type: 'text',
            content: message.content_after_tools,
            id: crypto.randomUUID()
        });
    }

    return blocks;
}

/**
 * Ensures all messages in an array have proper blocks reconstructed.
 * Use this when loading conversations from history or backend.
 */
export function ensureMessagesHaveBlocks(messages: ChatMessage[]): ChatMessage[] {
    return messages.map(msg => {
        // Only reconstruct blocks for Assistant messages (they're the ones with tool calls, etc.)
        if (msg.role === 'Assistant' && (!msg.blocks || msg.blocks.length === 0)) {
            return {
                ...msg,
                blocks: reconstructBlocks(msg)
            };
        }
        return msg;
    });
}
