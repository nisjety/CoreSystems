export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: Date;
  attachments?: FileAttachment[];
  metadata?: {
    source: 'reasoning-plane';
    citations?: string[];
  };
}

export interface Chat {
  id: string;
  title: string;
  messages: ChatMessage[];
  timestamp: Date;
  userId?: string;
}

export interface FileAttachment {
  id: string;
  name: string;
  mimeType: string;
  url?: string;
  extractedText?: string;
}

export interface SendMessageRequest {
  message: string;
  conversationId?: string;
  userId?: string;
  userName?: string;
  userEmail?: string;
  model?: string;
  attachments?: FileAttachment[];
  systemPrompt?: string;
}

export interface SendMessageResponse {
  success: boolean;
  conversationId: string;
  userMessage: ChatMessage;
  aiResponse: ChatMessage;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
    reasoningTokens?: number;
  };
  timestamp: string;
  model?: string;
  error?: string;
}

export interface RecentChatsResponse {
  success: boolean;
  chats: Chat[];
  count: number;
  error?: string;
}

export interface ImageGenerationRequest {
  prompt: string;
  size?: '1024x1024' | '1792x1024' | '1024x1792';
  quality?: 'standard' | 'hd';
  style?: 'vivid' | 'natural';
  n?: number;
  userTier?: 'free' | 'premium' | 'enterprise';
  enhance?: boolean;
}

export interface ImageGenerationResponse {
  success: boolean;
  images?: Array<{
    url: string;
    revisedPrompt?: string;
  }>;
  prompt: string;
  promptUsed?: string;
  durationMs?: number;
  model?: string;
  timestamp: string;
  error?: string;
}

const CHAT_API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || '';
const LEGACY_ORCHESTRATOR_BASE_URL = process.env.NEXT_PUBLIC_AI_ORCHESTRATOR_URL || 'http://localhost:3021';

class ChatServerAPI {
  private baseUrl: string;
  private legacyBaseUrl: string;

  constructor() {
    this.baseUrl = CHAT_API_BASE_URL;
    this.legacyBaseUrl = LEGACY_ORCHESTRATOR_BASE_URL;
  }

  async sendMessage(request: SendMessageRequest): Promise<SendMessageResponse> {
    try {
      const conversationId = request.conversationId;

      const response = await fetch(`${this.baseUrl}/api/chat/send`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          content: request.message,
          sessionId: conversationId,
          userId: request.userId,
          userName: request.userName,
          userEmail: request.userEmail,
        }),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json() as {
        id: string;
        content: string;
        role: 'assistant';
        timestamp: string;
        sessionId?: string;
        error?: string;
        metadata?: ChatMessage['metadata'];
      };
      
      const now = new Date();
      const userMessage: ChatMessage = {
        id: `msg_${Date.now()}_user`,
        role: 'user',
        content: request.message,
        timestamp: now,
        attachments: request.attachments,
      };

      const aiMessage: ChatMessage = {
        id: data.id,
        role: 'assistant',
        content: data.content,
        timestamp: new Date(data.timestamp),
        metadata: data.metadata,
      };

      const result = {
        success: true,
        conversationId: data.sessionId || conversationId || '',
        userMessage,
        aiResponse: aiMessage,
        timestamp: data.timestamp,
        error: data.error,
      };

      return result;
    } catch (error) {
      console.error('Chat API Error:', error);
      return {
        success: false,
        conversationId: request.conversationId || '',
        userMessage: {
          id: `msg_${Date.now()}_user`,
          role: 'user',
          content: request.message,
          timestamp: new Date(),
        },
        aiResponse: {
          id: `msg_${Date.now()}_error`,
          role: 'assistant',
          content: 'I apologize, but I encountered an error while processing your message. Please try again.',
          timestamp: new Date(),
        },
        timestamp: new Date().toISOString(),
        error: error instanceof Error ? error.message : 'Unknown error occurred',
      };
    }
  }

  async getRecentChats(_userId?: string, limit?: number): Promise<RecentChatsResponse> {
    try {
      const response = await fetch(`${this.baseUrl}/api/chat/sessions`, {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json() as {
        sessions: Array<{
          id: string;
          title: string;
          createdAt: string;
          updatedAt: string;
          userId?: string;
        }>;
        totalCount: number;
      };

      const chats = data.sessions.map((session) => ({
        id: session.id,
        title: session.title,
        messages: [],
        timestamp: new Date(session.updatedAt),
        userId: session.userId,
      }));

      const processedChats = limit ? chats.slice(0, limit) : chats;

      return {
        success: true,
        chats: processedChats,
        count: data.totalCount,
      };
    } catch (error) {
      return {
        success: false,
        chats: [],
        count: 0,
        error: error instanceof Error ? error.message : 'Failed to load recent chats',
      };
    }
  }

  async getConversationHistory(conversationId: string): Promise<{ success: boolean; conversation?: Chat; error?: string }> {
    try {
      const response = await fetch(`${this.baseUrl}/api/chat/sessions/${conversationId}`, {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      
      if (data.error) {
        return {
          success: false,
          error: data.error,
        };
      }

      const conversation: Chat = {
        id: data.id,
        title: data.title,
        userId: data.userId,
        timestamp: new Date(data.updatedAt),
        messages: data.messages.map((msg: {
          id: string;
          role: string;
          content: string;
          timestamp: string;
          metadata?: ChatMessage['metadata'];
        }) => ({
          id: msg.id,
          role: msg.role as 'user' | 'assistant' | 'system',
          content: msg.content,
          timestamp: new Date(msg.timestamp),
          metadata: msg.metadata,
        })),
      };

      return {
        success: true,
        conversation,
      };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to load conversation',
      };
    }
  }

  async clearConversation(conversationId: string): Promise<{ success: boolean; cleared: boolean; error?: string }> {
    try {
      const response = await fetch(`${this.baseUrl}/api/chat/sessions/${conversationId}`, {
        method: 'DELETE',
        headers: {
          'Content-Type': 'application/json',
        },
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      return {
        success: true,
        cleared: true,
      };
    } catch (error) {
      return {
        success: false,
        cleared: false,
        error: error instanceof Error ? error.message : 'Failed to clear conversation',
      };
    }
  }

  async generateImage(request: ImageGenerationRequest): Promise<ImageGenerationResponse> {
    try {
      const response = await fetch(`${this.legacyBaseUrl}/image/generate`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          prompt: request.prompt,
          size: request.size,
          quality: request.quality,
          style: request.style,
          n: request.n,
          enhance: request.enhance,
        }),
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => null);
        throw new Error(errorData?.message || `Failed to generate image: ${response.statusText}`);
      }

      const data = await response.json();
      
      // Transform ai-orchestrator response to expected format
      return {
        success: true,
        images: data.images || [],
        prompt: request.prompt,
        promptUsed: data.promptUsed,
        durationMs: data.durationMs,
        model: data.model,
        timestamp: new Date().toISOString(),
      };
    } catch (error) {
      console.error('Failed to generate image:', error);
      throw error;
    }
  }

  async getHealth(): Promise<{ status: string; uptime?: number; version?: string; service?: string }> {
    try {
      const response = await fetch(`${this.baseUrl}/api/chat/sessions`, {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      await response.json();

      return {
        status: 'ok',
        service: 'reasoning-plane-proxy',
      };
    } catch (error) {
      return {
        status: 'error',
        service: 'reasoning-plane-proxy',
      };
    }
  }
}

export const chatServerAPI = new ChatServerAPI();

// Export standalone functions for backward compatibility
export const sendMessage = (request: SendMessageRequest) => chatServerAPI.sendMessage(request);
export const getConversationHistory = (conversationId: string) => chatServerAPI.getConversationHistory(conversationId);
export const clearConversation = (conversationId: string) => chatServerAPI.clearConversation(conversationId);
export const getRecentChats = (userId?: string) => chatServerAPI.getRecentChats(userId);
export const generateImage = (request: ImageGenerationRequest) => chatServerAPI.generateImage(request);
export const getHealth = () => chatServerAPI.getHealth();
