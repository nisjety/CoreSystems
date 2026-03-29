'use client';

import { useState, useCallback, useEffect } from 'react';
import { nanoid } from 'nanoid';
import { chatServerAPI, Chat, ChatMessage, SendMessageRequest } from '@/components/chat/lib/chat-server-api';

export interface UseChatOptions {
  userId?: string;
  userName?: string;
  userEmail?: string;
  initialConversationId?: string;
}

export function useChat(options: UseChatOptions = {}) {
  const [chats, setChats] = useState<Chat[]>([]);
  const [currentChat, setCurrentChat] = useState<Chat | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isTyping, setIsTyping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadRecentChats = useCallback(async () => {
    try {
      setIsLoading(true);
      const response = await chatServerAPI.getRecentChats(options.userId, 10);
      if (response.success) {
        setChats(response.chats);
      } else {
        setError(response.error || 'Failed to load recent chats');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load recent chats');
    } finally {
      setIsLoading(false);
    }
  }, [options.userId]);

  const loadConversation = useCallback(async (conversationId: string) => {
    try {
      setIsLoading(true);
      const response = await chatServerAPI.getConversationHistory(conversationId);
      if (response.success && response.conversation) {
        setCurrentChat(response.conversation);
      } else {
        setError(response.error || 'Failed to load conversation');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load conversation');
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Load recent chats on mount or user change
  useEffect(() => {
    loadRecentChats();
  }, [loadRecentChats]);

  // Load initial conversation if provided
  useEffect(() => {
    if (options.initialConversationId) {
      loadConversation(options.initialConversationId);
    }
  }, [options.initialConversationId, loadConversation]);

  const sendMessage = useCallback(async (message: string, selectedModel?: string) => {
    if (!message.trim()) return null;

    setError(null);
    setIsTyping(true);

    try {
      const conversationId = currentChat?.id || nanoid();

      const request: SendMessageRequest = {
        message: message.trim(),
        conversationId,
        userId: options.userId,
        userName: options.userName,
        userEmail: options.userEmail,
        model: selectedModel,
      };

      const response = await chatServerAPI.sendMessage(request);

      if (response.success) {
        // Update current chat
        const updatedChat: Chat = {
          id: response.conversationId,
          title: currentChat?.title || generateChatTitle(message),
          messages: [
            ...(currentChat?.messages || []),
            response.userMessage,
            response.aiResponse,
          ],
          timestamp: new Date(),
          userId: options.userId,
        };

        setCurrentChat(updatedChat);

        // Update chats list
        setChats(prevChats => {
          const existingChatIndex = prevChats.findIndex(chat => chat.id === response.conversationId);
          if (existingChatIndex >= 0) {
            // Update existing chat
            const newChats = [...prevChats];
            newChats[existingChatIndex] = updatedChat;
            return newChats;
          } else {
            // Add new chat to the beginning
            return [updatedChat, ...prevChats];
          }
        });

        return {
          conversationId: response.conversationId,
          userMessage: response.userMessage,
          aiResponse: response.aiResponse,
          usage: response.usage,
        };
      } else {
        setError(response.error || 'Failed to send message');
        return null;
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to send message';
      setError(errorMessage);
      
      // Create local error message
      if (currentChat) {
        const errorChatMessage: ChatMessage = {
          id: nanoid(),
          role: 'assistant',
          content: 'I apologize, but I encountered an error while processing your message. Please try again.',
          timestamp: new Date(),
        };

        setCurrentChat(prev => prev ? {
          ...prev,
          messages: [...prev.messages, errorChatMessage],
        } : null);
      }
      
      return null;
    } finally {
      setIsTyping(false);
    }
  }, [currentChat, options]);

  const selectChat = useCallback((chat: Chat) => {
    setCurrentChat(chat);
    setError(null);
  }, []);

  const startNewChat = useCallback(() => {
    setCurrentChat(null);
    setError(null);
  }, []);

  const clearCurrentConversation = useCallback(async () => {
    if (!currentChat) return false;

    try {
      setIsLoading(true);
      const response = await chatServerAPI.clearConversation(currentChat.id);
      
      if (response.success) {
        // Remove from chats list
        setChats(prevChats => prevChats.filter(chat => chat.id !== currentChat.id));
        setCurrentChat(null);
        return true;
      } else {
        setError(response.error || 'Failed to clear conversation');
        return false;
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to clear conversation');
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [currentChat]);

  const generateChatTitle = (firstMessage: string): string => {
    return firstMessage.length > 50 
      ? firstMessage.substring(0, 47) + '...'
      : firstMessage;
  };

  return {
    // State
    chats,
    currentChat,
    isLoading,
    isTyping,
    error,

    // Actions
    sendMessage,
    selectChat,
    startNewChat,
    clearCurrentConversation,
    loadRecentChats,
    loadConversation,

    // Utilities
    setError,
  };
}
