import { useChatV2 } from './useChatV2';

interface UseChatOptions {
    autoApproveRunCommands?: boolean;
}

export function useChat(options?: UseChatOptions) {
    return useChatV2(options);
}
