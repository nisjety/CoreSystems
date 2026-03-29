// api/orpc/chat.ts - Type-safe Chat API Client
import { z } from 'zod';

// Schemas for runtime validation
export const MessageMetadataSchema = z.object({
  source: z.literal('reasoning-plane'),
  citations: z.array(z.string()).optional(),
});

export const MessageSchema = z.object({
  id: z.string(),
  content: z.string(),
  role: z.enum(['user', 'assistant', 'system']),
  timestamp: z.string(),
  sessionId: z.string().optional(),
  isThinking: z.boolean().optional(),
  error: z.string().optional(),
  metadata: MessageMetadataSchema.optional(),
});

export const SessionSchema = z.object({
  id: z.string(),
  title: z.string(),
  messages: z.array(MessageSchema),
  createdAt: z.string(),
  updatedAt: z.string(),
  userId: z.string().optional(),
});

// Schema for session list items (without full messages)
export const SessionListItemSchema = z.object({
  id: z.string(),
  title: z.string(),
  createdAt: z.string(),
  updatedAt: z.string(),
  isArchived: z.boolean().optional(),
  isPinned: z.boolean().optional(),
  messageCount: z.number().optional(),
  userId: z.string().optional(),
});

export const CreateMessageSchema = z.object({
  content: z.string().min(1),
  sessionId: z.string().optional(),
  userId: z.string().optional(),
  userName: z.string().optional(),
  userEmail: z.string().email().optional(),
});

export const SendMessageResponseSchema = z.object({
  message: MessageSchema,
  session: SessionSchema,
});

export const GetSessionsResponseSchema = z.object({
  sessions: z.array(SessionListItemSchema),
  totalCount: z.number(),
});

export const ErrorResponseSchema = z.object({
  error: z.string(),
  message: z.string(),
  statusCode: z.number(),
});

// Type exports
export type Message = z.infer<typeof MessageSchema>;
export type Session = z.infer<typeof SessionSchema>;
export type SessionListItem = z.infer<typeof SessionListItemSchema>;
export type CreateMessage = z.infer<typeof CreateMessageSchema>;
export type SendMessageResponse = z.infer<typeof SendMessageResponseSchema>;
export type GetSessionsResponse = z.infer<typeof GetSessionsResponseSchema>;
export type ErrorResponse = z.infer<typeof ErrorResponseSchema>;

// API Client Configuration - Prefer origin-only base URL (relative by default)
const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || '/';

class ApiError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public response?: ErrorResponse
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

// Type-safe API client
export class ChatApiClient {
  private baseUrl: string;

  constructor(baseUrl: string = API_BASE_URL) {
    this.baseUrl = baseUrl;
  }

  // Safely join base URL and endpoint, avoiding duplicate /api segments
  private buildUrl(endpoint: string): string {
    const base = (this.baseUrl || '').replace(/\/+$/, '');
    const path = endpoint.startsWith('/') ? endpoint : `/${endpoint}`;

    // If base ends with /api and path starts with /api, drop one
    if (base.endsWith('/api') && path.startsWith('/api')) {
      return `${base.slice(0, -4)}${path}`;
    }

    // Collapse duplicate slashes and accidental duplicate /api
    return `${base}${path}`
      .replace(/([^:])\/\/+/, '$1/')
      .replace(/\/api\/api\//g, '/api/');
  }

  private async fetchWithValidation<T>(
    endpoint: string,
    schema: z.ZodSchema<T>,
    options?: RequestInit
  ): Promise<T> {
    try {
      const url = this.buildUrl(endpoint);
      const response = await fetch(url, {
        cache: 'no-store',
        headers: {
          'Content-Type': 'application/json',
          ...options?.headers,
        },
        ...options,
      });

      const data = await response.json();

      if (!response.ok) {
        const errorData = ErrorResponseSchema.safeParse(data);
        if (errorData.success) {
          throw new ApiError(errorData.data.message, response.status, errorData.data);
        }
        throw new ApiError('Request failed', response.status);
      }

      const result = schema.safeParse(data);
      if (!result.success) {
        console.error('Schema validation failed:', result.error);
        throw new Error('Invalid response format');
      }

      return result.data;
    } catch (error) {
      if (error instanceof ApiError) {
        throw error;
      }
      throw new ApiError('Network error', 0);
    }
  }

  // Send a message to a session
  async sendMessage(message: CreateMessage): Promise<Message> {
    return this.fetchWithValidation(
      '/api/chat/send',
      MessageSchema,
      {
        method: 'POST',
        body: JSON.stringify(message),
      }
    );
  }

  // Get all sessions for the current user
  async getSessions(): Promise<GetSessionsResponse> {
    return this.fetchWithValidation('/api/chat/sessions', GetSessionsResponseSchema);
  }

  // Get a specific session
  async getSession(sessionId: string): Promise<Session> {
    return this.fetchWithValidation(`/api/chat/sessions/${sessionId}`, SessionSchema);
  }

  // Create a new session
  async createSession(title?: string): Promise<Session> {
    return this.fetchWithValidation(
      '/api/chat/sessions',
      SessionSchema,
      {
        method: 'POST',
        body: JSON.stringify({ title: title || 'New Chat' }),
      }
    );
  }

  // Delete a session
  async deleteSession(sessionId: string): Promise<void> {
    await this.fetchWithValidation(
      `/api/chat/sessions/${sessionId}`,
      z.object({}),
      {
        method: 'DELETE',
      }
    );
  }

  // Update session title
  async updateSessionTitle(sessionId: string, title: string): Promise<Session> {
    return this.fetchWithValidation(
      `/api/chat/sessions/${sessionId}`,
      SessionSchema,
      {
        method: 'PATCH',
        body: JSON.stringify({ title }),
      }
    );
  }

  // Stream messages for real-time updates
  async streamMessage(message: CreateMessage): Promise<ReadableStream<Message>> {
    const response = await fetch(this.buildUrl('/api/chat/stream'), {
      cache: 'no-store',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(message),
    });

    if (!response.ok) {
      throw new ApiError('Stream request failed', response.status);
    }

    if (!response.body) {
      throw new Error('No response body');
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();

    return new ReadableStream({
      start(controller) {
        function pump(): Promise<void> {
          return reader.read().then(({ done, value }) => {
            if (done) {
              controller.close();
              return;
            }

            const chunk = decoder.decode(value);
            const lines = chunk.split('\n');

            for (const line of lines) {
              if (line.trim() && line.startsWith('data: ')) {
                try {
                  const data = JSON.parse(line.slice(6));
                  const result = MessageSchema.safeParse(data);
                  if (result.success) {
                    controller.enqueue(result.data);
                  }
                } catch (error) {
                  console.error('Failed to parse stream data:', error);
                }
              }
            }

            return pump();
          });
        }

        return pump();
      },
    });
  }
}

// Default client instance
export const chatApiClient = new ChatApiClient();
