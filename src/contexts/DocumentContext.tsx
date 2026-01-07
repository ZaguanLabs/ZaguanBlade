'use client';
import React, { createContext, useContext, useState, useCallback } from 'react';
import { DocumentTab } from '../types/document';
import { invoke } from '@tauri-apps/api/core';

interface DocumentContextType {
  tabs: DocumentTab[];
  activeTabId: string | null;
  openEphemeralDocument: (content: string, title: string, suggestedName: string) => Promise<void>;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
}

const DocumentContext = createContext<DocumentContextType | undefined>(undefined);

export const DocumentProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [tabs, setTabs] = useState<DocumentTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);

  const openEphemeralDocument = useCallback(async (content: string, title: string, suggestedName: string) => {
    try {
      // Create ephemeral document in Tauri backend
      const documentId = await invoke<string>('create_ephemeral_document', {
        content,
        suggestedName,
      });

      // Create tab
      const newTab: DocumentTab = {
        id: documentId,
        title,
        content,
        isEphemeral: true,
        isDirty: false,
        suggestedName,
      };

      setTabs(prev => [...prev, newTab]);
      setActiveTabId(documentId);
    } catch (error) {
      console.error('Failed to create ephemeral document:', error);
    }
  }, []);

  const closeTab = useCallback((tabId: string) => {
    setTabs(prev => prev.filter(tab => tab.id !== tabId));
    setActiveTabId(prev => {
      if (prev === tabId) {
        const remainingTabs = tabs.filter(tab => tab.id !== tabId);
        return remainingTabs.length > 0 ? remainingTabs[remainingTabs.length - 1].id : null;
      }
      return prev;
    });
  }, [tabs]);

  const setActiveTab = useCallback((tabId: string) => {
    setActiveTabId(tabId);
  }, []);

  return (
    <DocumentContext.Provider
      value={{
        tabs,
        activeTabId,
        openEphemeralDocument,
        closeTab,
        setActiveTab,
      }}
    >
      {children}
    </DocumentContext.Provider>
  );
};

export const useDocuments = () => {
  const context = useContext(DocumentContext);
  if (!context) {
    throw new Error('useDocuments must be used within DocumentProvider');
  }
  return context;
};
