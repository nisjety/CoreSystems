// providers/ChatProvider.tsx - Global Chat State Management
'use client';

import React, { createContext, useContext, useReducer, useCallback, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { 
  Message, 
  Session, 
  SessionListItem,
  CreateMessage, 
  chatApiClient,
  type ErrorResponse 
} from '@/components/chat/api/orpc/chat';
import { useI18n } from '@/components/chat/hooks/i18n';
import {
  chatQueryKeys,
  removeChatHistorySession,
  sessionToListItem,
  setChatSessionCache,
  writeChatHistoryCache,
} from '@/components/chat/lib/query-state';

// Chat State Types
interface ChatState {
  sessions: SessionListItem[];
  currentSession: Session | null;
  messages: Message[];
  isLoading: boolean;
  isTyping: boolean;
  error: string | null;
  connected: boolean;
}

// Chat Actions
type ChatAction =
  | { type: 'SET_SESSIONS'; payload: SessionListItem[] }
  | { type: 'SET_CURRENT_SESSION'; payload: Session | null }
  | { type: 'SET_MESSAGES'; payload: Message[] }
  | { type: 'ADD_MESSAGE'; payload: Message }
  | { type: 'UPDATE_MESSAGE'; payload: { id: string; updates: Partial<Message> } }
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_TYPING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_CONNECTED'; payload: boolean }
  | { type: 'CREATE_SESSION'; payload: Session }
  | { type: 'DELETE_SESSION'; payload: string }
  | { type: 'UPDATE_SESSION'; payload: Session }
  | { type: 'START_NEW_CHAT' }
  | { type: 'RESET_STATE' };

// Initial State
const initialState: ChatState = {
  sessions: [],
  currentSession: null,
  messages: [],
  isLoading: false,
  isTyping: false,
  error: null,
  connected: false,
};

// State Reducer
function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case 'SET_SESSIONS':
      return { ...state, sessions: action.payload };
    
    case 'SET_CURRENT_SESSION':
      return { 
        ...state, 
        currentSession: action.payload,
        messages: action.payload?.messages || [],
      };
    
    case 'SET_MESSAGES':
      return { ...state, messages: action.payload };
    
    case 'ADD_MESSAGE':
      return { 
        ...state, 
        messages: [...state.messages, action.payload] 
      };
    
    case 'UPDATE_MESSAGE':
      return {
        ...state,
        messages: state.messages.map(msg =>
          msg.id === action.payload.id 
            ? { ...msg, ...action.payload.updates }
            : msg
        ),
      };
    
    case 'SET_LOADING':
      return { ...state, isLoading: action.payload };
    
    case 'SET_TYPING':
      return { ...state, isTyping: action.payload };
    
    case 'SET_ERROR':
      return { ...state, error: action.payload };
    
    case 'SET_CONNECTED':
      return { ...state, connected: action.payload };
    
    case 'CREATE_SESSION':
      return { 
        ...state, 
        sessions: [sessionToListItem(action.payload), ...state.sessions],
        currentSession: action.payload,
        messages: action.payload.messages,
      };
    
    case 'DELETE_SESSION':
      const filteredSessions = state.sessions.filter(s => s.id !== action.payload);
      const wasCurrentDeleted = state.currentSession?.id === action.payload;
      return {
        ...state,
        sessions: filteredSessions,
        currentSession: wasCurrentDeleted ? null : state.currentSession,
        messages: wasCurrentDeleted ? [] : state.messages,
      };
    
    case 'UPDATE_SESSION':
      {
        const isCurrentSession = state.currentSession?.id === action.payload.id;

        return {
        ...state,
        sessions: state.sessions.map(s =>
          s.id === action.payload.id ? sessionToListItem(action.payload) : s
        ),
        currentSession: isCurrentSession
          ? action.payload 
          : state.currentSession,
        messages: isCurrentSession ? action.payload.messages : state.messages,
      };
      }

    case 'START_NEW_CHAT':
      return {
        ...state,
        currentSession: null,
        messages: [],
        isTyping: false,
        error: null,
      };
    
    case 'RESET_STATE':
      return initialState;
    
    default:
      return state;
  }
}

// Context Interface
interface ChatContextType {
  // State
  state: ChatState;
  
  // Session Management
  loadSessions: () => Promise<void>;
  createNewSession: (title?: string) => Promise<Session | null>;
  selectSession: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  updateSessionTitle: (sessionId: string, title: string) => Promise<void>;
  
  // Message Management
  sendMessage: (content: string, sessionId?: string) => Promise<void>;
  streamMessage: (content: string, sessionId?: string) => Promise<void>;
  regenerateLastMessage: () => Promise<void>;
  
  // Utility
  clearError: () => void;
  startNewChat: () => void;
  resetChat: () => void;
}

// Create Context
const ChatContext = createContext<ChatContextType | undefined>(undefined);

// Provider Component
interface ChatProviderProps {
  children: React.ReactNode;
}

export function ChatProvider({ children }: ChatProviderProps) {
  const [state, dispatch] = useReducer(chatReducer, initialState);
  const { t } = useI18n();
  const queryClient = useQueryClient();

  const invalidateChatCollections = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.history }),
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.legacyMessages }),
    ]);
  }, [queryClient]);

  // Error Handling Helper
  const handleError = useCallback((error: unknown) => {
    console.error('Chat error:', error);
    
    if (error instanceof Error) {
      dispatch({ type: 'SET_ERROR', payload: error.message });
    } else if (typeof error === 'object' && error !== null && 'message' in error) {
      const errorResponse = error as ErrorResponse;
      dispatch({ type: 'SET_ERROR', payload: errorResponse.message });
    } else {
      dispatch({ type: 'SET_ERROR', payload: t('error.generic') });
    }
  }, [t]);

  // Load all sessions
  const loadSessions = useCallback(async () => {
    try {
      dispatch({ type: 'SET_LOADING', payload: true });
      const response = await chatApiClient.getSessions();
      writeChatHistoryCache(queryClient, response);
      dispatch({ type: 'SET_SESSIONS', payload: response.sessions });
    } catch (error) {
      handleError(error);
    } finally {
      dispatch({ type: 'SET_LOADING', payload: false });
    }
  }, [handleError, queryClient]);

  // Create new session
  const createNewSession = useCallback(async (title?: string): Promise<Session | null> => {
    try {
      dispatch({ type: 'SET_LOADING', payload: true });
      const session = await chatApiClient.createSession(title);
      setChatSessionCache(queryClient, session);
      dispatch({ type: 'CREATE_SESSION', payload: session });
      await invalidateChatCollections();
      return session;
    } catch (error) {
      handleError(error);
      return null;
    } finally {
      dispatch({ type: 'SET_LOADING', payload: false });
    }
  }, [handleError, invalidateChatCollections, queryClient]);

  // Select existing session
  const selectSession = useCallback(async (sessionId: string) => {
    try {
      dispatch({ type: 'SET_LOADING', payload: true });
      const session = await chatApiClient.getSession(sessionId);
      setChatSessionCache(queryClient, session);
      dispatch({ type: 'SET_CURRENT_SESSION', payload: session });
    } catch (error) {
      handleError(error);
    } finally {
      dispatch({ type: 'SET_LOADING', payload: false });
    }
  }, [handleError, queryClient]);

  // Delete session
  const deleteSession = useCallback(async (sessionId: string) => {
    try {
      await chatApiClient.deleteSession(sessionId);
      removeChatHistorySession(queryClient, sessionId);
      dispatch({ type: 'DELETE_SESSION', payload: sessionId });
      await invalidateChatCollections();
    } catch (error) {
      handleError(error);
    }
  }, [handleError, invalidateChatCollections, queryClient]);

  // Update session title
  const updateSessionTitle = useCallback(async (sessionId: string, title: string) => {
    try {
      const updatedSession = await chatApiClient.updateSessionTitle(sessionId, title);
      setChatSessionCache(queryClient, updatedSession);
      dispatch({ type: 'UPDATE_SESSION', payload: updatedSession });
      await invalidateChatCollections();
    } catch (error) {
      handleError(error);
    }
  }, [handleError, invalidateChatCollections, queryClient]);

  const syncSession = useCallback(async (sessionId: string) => {
    const session = await chatApiClient.getSession(sessionId);
    setChatSessionCache(queryClient, session);
    dispatch({ type: 'UPDATE_SESSION', payload: session });
    await invalidateChatCollections();
  }, [invalidateChatCollections, queryClient]);

  const ensureSession = useCallback(async (content: string, sessionId?: string) => {
    const activeSessionId = sessionId || state.currentSession?.id;
    if (activeSessionId) {
      return activeSessionId;
    }

    const title = content.length > 60 ? `${content.slice(0, 57)}...` : content;
    const createdSession = await chatApiClient.createSession(title);
    setChatSessionCache(queryClient, createdSession);
    dispatch({ type: 'CREATE_SESSION', payload: createdSession });
    await invalidateChatCollections();

    return createdSession.id;
  }, [invalidateChatCollections, queryClient, state.currentSession?.id]);

  // Send message (non-streaming)
  const sendMessage = useCallback(async (content: string, sessionId?: string) => {
    if (!content.trim()) return;

    try {
      dispatch({ type: 'SET_LOADING', payload: true });
      dispatch({ type: 'SET_ERROR', payload: null });

      const activeSessionId = await ensureSession(content.trim(), sessionId);

      const messageData: CreateMessage = {
        content: content.trim(),
        sessionId: activeSessionId,
      };

      const userMessage: Message = {
        id: `temp-user-${Date.now()}`,
        content: content.trim(),
        role: 'user',
        timestamp: new Date().toISOString(),
        sessionId: activeSessionId,
      };
      dispatch({ type: 'ADD_MESSAGE', payload: userMessage });

      const response = await chatApiClient.sendMessage(messageData);
      
      dispatch({ type: 'ADD_MESSAGE', payload: response });
      await syncSession(activeSessionId);
      
    } catch (error) {
      handleError(error);
    } finally {
      dispatch({ type: 'SET_LOADING', payload: false });
    }
  }, [ensureSession, handleError, syncSession]);

  // Stream message (real-time)
  const streamMessage = useCallback(async (content: string, sessionId?: string) => {
    if (!content.trim()) return;

    try {
      dispatch({ type: 'SET_TYPING', payload: true });
      dispatch({ type: 'SET_ERROR', payload: null });

      const activeSessionId = await ensureSession(content.trim(), sessionId);

      const messageData: CreateMessage = {
        content: content.trim(),
        sessionId: activeSessionId,
      };

      // Add user message immediately
      const userMessage: Message = {
        id: `temp-${Date.now()}`,
        content: content.trim(),
        role: 'user',
        timestamp: new Date().toISOString(),
        sessionId: activeSessionId,
      };
      dispatch({ type: 'ADD_MESSAGE', payload: userMessage });

      // Stream AI response
      const stream = await chatApiClient.streamMessage(messageData);
      const reader = stream.getReader();

      let assistantMessage: Message | null = null;

      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          if (!assistantMessage) {
            // First chunk - create assistant message
            assistantMessage = {
              ...value,
              isThinking: true,
            };
            dispatch({ type: 'ADD_MESSAGE', payload: assistantMessage });
          } else {
            // Update existing message
            dispatch({ 
              type: 'UPDATE_MESSAGE', 
              payload: { 
                id: assistantMessage.id, 
                updates: { 
                  content: value.content,
                  isThinking: value.isThinking || false,
                } 
              } 
            });
          }
        }
      } finally {
        reader.releaseLock();
      }

      await syncSession(activeSessionId);

    } catch (error) {
      handleError(error);
    } finally {
      dispatch({ type: 'SET_TYPING', payload: false });
    }
  }, [ensureSession, handleError, syncSession]);

  // Regenerate last assistant message
  const regenerateLastMessage = useCallback(async () => {
    const messages = state.messages;
    const lastUserMessage = [...messages].reverse().find(m => m.role === 'user');
    
    if (lastUserMessage) {
      // Remove last assistant message if exists
      const lastAssistantIndex = messages.findLastIndex(m => m.role === 'assistant');
      if (lastAssistantIndex > -1) {
        const filteredMessages = messages.slice(0, lastAssistantIndex);
        dispatch({ type: 'SET_MESSAGES', payload: filteredMessages });
      }
      
      // Resend the last user message
      await streamMessage(lastUserMessage.content, lastUserMessage.sessionId);
    }
  }, [state.messages, streamMessage]);

  // Clear current error
  const clearError = useCallback(() => {
    dispatch({ type: 'SET_ERROR', payload: null });
  }, []);

  const startNewChat = useCallback(() => {
    dispatch({ type: 'START_NEW_CHAT' });
  }, []);

  // Reset entire chat state
  const resetChat = useCallback(() => {
    dispatch({ type: 'RESET_STATE' });
  }, []);

  // Connection status simulation (can be replaced with actual WebSocket)
  useEffect(() => {
    dispatch({ type: 'SET_CONNECTED', payload: true });
  }, []);

  // Context value
  const contextValue: ChatContextType = {
    state,
    loadSessions,
    createNewSession,
    selectSession,
    deleteSession,
    updateSessionTitle,
    sendMessage,
    streamMessage,
    regenerateLastMessage,
    clearError,
    startNewChat,
    resetChat,
  };

  return (
    <ChatContext.Provider value={contextValue}>
      {children}
    </ChatContext.Provider>
  );
}

// Custom hook for using chat context
export function useChat() {
  const context = useContext(ChatContext);
  if (context === undefined) {
    throw new Error('useChat must be used within a ChatProvider');
  }
  return context;
}
