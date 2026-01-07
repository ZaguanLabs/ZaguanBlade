import { useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';

interface Message {
  role: string;
  content: string;
}

export const useResearchTab = (messages: Message[]) => {
  const processedMessagesRef = useRef(new Set<string>());
  const processingRef = useRef(false);
  const suppressedIndicesRef = useRef(new Set<number>());

  useEffect(() => {
    // Check last assistant message for research results
    const lastMessage = messages[messages.length - 1];
    const lastIndex = messages.length - 1;
    
    if (!lastMessage || lastMessage.role !== 'Assistant') return;
    
    // Create unique key for this message (content hash to avoid duplicates)
    const messageKey = `${lastIndex}-${lastMessage.content.substring(0, 100)}`;
    if (processedMessagesRef.current.has(messageKey)) return;
    if (processingRef.current) return;

    // Check if this is a research result (contains markdown with headers and substantial content)
    const content = lastMessage.content;
    if (!content || content.length < 200) return;

    // Heuristic: Research results typically have headers and are long
    const hasHeaders = /^#{1,3}\s+/m.test(content);
    const isLongForm = content.length > 500;
    
    if (hasHeaders && isLongForm) {
      processedMessagesRef.current.add(messageKey);
      processingRef.current = true;
      suppressedIndicesRef.current.add(lastIndex);
      
      // Create ephemeral document
      const timestamp = new Date().toISOString().slice(0, 19).replace(/:/g, '-');
      const suggestedName = `research-${timestamp}.md`;
      
      invoke<string>('create_ephemeral_document', {
        content,
        suggestedName,
      }).then((documentId) => {
        // Small delay to ensure no race conditions
        setTimeout(() => {
          // Emit event to open the document tab
          emit('open-ephemeral-document', {
            id: documentId,
            title: 'Research Results',
            content,
            suggestedName,
          });
        }, 100);
        processingRef.current = false;
      }).catch((error) => {
        console.error('Failed to create ephemeral document:', error);
        processingRef.current = false;
      });
    }
  }, [messages]);

  const shouldSuppressMessage = useCallback((index: number) => {
    return suppressedIndicesRef.current.has(index);
  }, []);

  const getCompactMessage = useCallback((index: number) => {
    if (suppressedIndicesRef.current.has(index)) {
      return '📄 **Research results opened in new tab**\n\n_Click the tab above to view the full research document._';
    }
    return null;
  }, []);

  return { shouldSuppressMessage, getCompactMessage };
};
