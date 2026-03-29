'use client';

import { useState, useCallback, useEffect } from 'react';
import { useQuery, useMutation, useAction } from 'convex/react';
import { api } from '@/lib/convex-api-stub';
import type { Chat as UIChat } from '@/components/chat/types/index';
import type { Chat as ApiChat, ChatMessage } from '@/components/chat/lib/chat-server-api';
// import { Id } from '@convex/_generated/dataModel'; // stubbed below
// Fake Id type alias to fix the build
export type Id<TableName extends string> = string & { __tableName: TableName };

export interface UseChatOptions {
    userId?: string;
    userName?: string;
    userEmail?: string;
    initialConversationId?: string;
}

type ChatLike = UIChat & ApiChat;

export function useConvexChat(options: UseChatOptions = {}) {
    const [currentConversationId, setCurrentConversationId] = useState<Id<"conversations"> | null>(null);
    const [isTyping, setIsTyping] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // 1. Get Convex User & Org Context
    // Ensure we pass a string; if options.userId is undefined, we skip the query.
    const convexUser = useQuery(
        api.users.getByExternalAuthId,
        options.userId ? { externalAuthId: options.userId } : "skip"
    );

    // 2. Load Convex Conversations (Recent Chats)
    // We use the convex user ID to fetch their conversations
    const convexConversations = useQuery(
        api.conversations.listByUser,
        convexUser ? { userId: convexUser._id } : "skip"
    ) || [];

    // Map Convex conversations to the expected Chat[] format for UI
    const chats: UIChat[] = convexConversations.map((conv: any) => ({
        id: conv._id,
        title: conv.title,
        messages: [], // Real messages loaded when selected
        timestamp: new Date(conv.updatedAt),
        userId: options.userId,
    }));

    // 3. Load Current Chat Messages
    const convexMessages = useQuery(
        api.messages.get,
        currentConversationId ? { conversationId: currentConversationId } : "skip"
    ) || [];

    // Reconstruct the currentChat object for the UI
    const currentChat: UIChat | null = currentConversationId ? {
        id: currentConversationId,
        title: chats.find(c => c.id === currentConversationId)?.title || "Active Chat",
        timestamp: new Date(),
        userId: options.userId,
        messages: convexMessages.map((msg: any) => ({
            id: msg._id,
            role: msg.role === "system" ? "assistant" : msg.role,
            content: msg.content,
            timestamp: new Date(msg.createdAt),
        })),
    } : null;

    // 4. Mutations & Actions
    const createConversation = useMutation(api.conversations.create);
    const addMessage = useMutation(api.conversations.sendMessage);
    const triggerAiResponse = useAction(api.ai.generateResponse);
    const removeConversation = useMutation(api.conversations.remove);

    const sendMessage = useCallback(async (messageText: string, selectedModel?: string) => {
        if (!messageText.trim() || !convexUser) return null;

        setError(null);
        setIsTyping(true);

        try {
            let activeConversationId = currentConversationId;

            // Create a conversation if there isn't one
            if (!activeConversationId) {
                const title = messageText.length > 50 ? messageText.substring(0, 47) + '...' : messageText;
                activeConversationId = await createConversation({
                    userId: convexUser._id,
                    orgId: convexUser.orgId,
                    title,
                    metadata: { model: selectedModel },
                });
                setCurrentConversationId(activeConversationId);
            }

            // Add the user message
            const userMessageId = await addMessage({
                conversationId: activeConversationId as Id<"conversations">,
                content: messageText,
                userId: convexUser._id,
            });

            // Trigger the AI backend to stream the response
            // We do NOT await this fully before showing UI updates, 
            // because Convex's reactive subscriptions will instantly show the placeholder!
            triggerAiResponse({
                conversationId: activeConversationId as Id<"conversations">,
                userMessageId: userMessageId,
            }).catch(err => {
                console.error("AI Generation failed:", err);
                setError("Failed to get response from AI Core");
            }).finally(() => {
                setIsTyping(false);
            });

            // Return immediately so the UI clears the input.
            // The `messages:get` subscription handles the rest seamlessly.
            return { conversationId: activeConversationId };

        } catch (err) {
            console.error("Failed to send message via Convex:", err);
            setError(err instanceof Error ? err.message : 'Failed to send message');
            setIsTyping(false);
            return null;
        }
    }, [currentConversationId, convexUser, createConversation, addMessage, triggerAiResponse]);

    const selectChat = useCallback((chat: ChatLike) => {
        setCurrentConversationId(chat.id as Id<"conversations">);
        setError(null);
    }, []);

    const startNewChat = useCallback(() => {
        setCurrentConversationId(null);
        setError(null);
    }, []);

    const clearCurrentConversation = useCallback(async () => {
        if (!currentConversationId) return false;
        try {
            await removeConversation({ conversationId: currentConversationId });
            setCurrentConversationId(null);
            return true;
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Failed to clear conversation');
            return false;
        }
    }, [currentConversationId, removeConversation]);

    // Compatibility stubs
    const loadRecentChats = useCallback(async () => { }, []);
    const loadConversation = useCallback(async (id: string) => {
        setCurrentConversationId(id as Id<"conversations">);
    }, []);

    useEffect(() => {
        if (options.initialConversationId) {
            setCurrentConversationId(options.initialConversationId as Id<"conversations">);
        }
    }, [options.initialConversationId]);

    return {
        chats,
        currentChat,
        isLoading: convexUser === undefined, // Simple loading state for auth resolution
        isTyping,
        error,
        sendMessage,
        selectChat,
        startNewChat,
        clearCurrentConversation,
        loadRecentChats,
        loadConversation,
        setError,
    };
}
