// api/orpc/chat.ts - Type-safe Chat API Client
import { z } from 'zod';
import { emit } from '@/lib/telemetry/client';
import { parseJsonSseData, parseSseChunk } from '@/lib/sse/parser';
import { clearPendingStream, rememberPendingStream } from '@/components/chat/api/chat-resume';

// Schemas for runtime validation
const MessageMetadataSchema = z.object({
  source: z.literal('reasoning-plane'),
  citations: z.array(z.string()).optional(),
});

export const MessageSchema = z.object({
  id: z.string(),
  clientId: z.string().optional(),
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
  orgId: z.string().optional(),
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
  orgId: z.string().optional(),
});

const CreateMessageSchema = z.object({
  content: z.string().min(1),
  sessionId: z.string().optional(),
  clientId: z.string().optional(),
  lastEventId: z.string().optional(),
  userId: z.string().optional(),
  userName: z.string().optional(),
  userEmail: z.string().email().optional(),
  model: z.string().optional(),
  responseMode: z.enum(['auto', 'quick', 'deep']).optional(),
  browseWeb: z.boolean().optional(),
  attachmentUrls: z.array(z.string()).optional(),
});

const SendMessageResponseSchema = z.object({
  message: MessageSchema,
  session: SessionSchema,
});

const GetSessionsResponseSchema = z.object({
  sessions: z.array(SessionListItemSchema),
  totalCount: z.number(),
});

export const ErrorResponseSchema = z.object({
  error: z.string(),
  message: z.string(),
  statusCode: z.number(),
});

export const ChatActorSchema = z.object({
  userId: z.string(),
  userName: z.string(),
  userEmail: z.string().email(),
  orgId: z.string(),
  role: z.enum(['admin', 'member', 'viewer']),
  convexOrgId: z.string(),
  convexUserId: z.string(),
});

// Type exports
export type Message = z.infer<typeof MessageSchema>;
export type Session = z.infer<typeof SessionSchema>;
export type SessionListItem = z.infer<typeof SessionListItemSchema>;
type CreateMessage = z.infer<typeof CreateMessageSchema>;
type SendMessageResponse = z.infer<typeof SendMessageResponseSchema>;
type GetSessionsResponse = z.infer<typeof GetSessionsResponseSchema>;
export type ErrorResponse = z.infer<typeof ErrorResponseSchema>;
export type ChatActor = z.infer<typeof ChatActorSchema>;

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
class ChatApiClient {
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

  async getActor(): Promise<ChatActor> {
    return this.fetchWithValidation('/api/chat/actor', ChatActorSchema);
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
    const startedAt = performance.now();
    emit('sse.started', {
      props: {
        route: '/api/chat/stream',
        session_id: message.sessionId,
        client_id: message.clientId,
      },
    });

    const response = await fetch(this.buildUrl('/api/chat/stream'), {
      cache: 'no-store',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(message.lastEventId ? { 'Last-Event-ID': message.lastEventId } : {}),
      },
      body: JSON.stringify(message),
    });

    if (!response.ok) {
      let errorMessage = 'Stream request failed';

      try {
        const errorPayload = await response.json();
        const parsed = ErrorResponseSchema.safeParse(errorPayload);
        if (parsed.success) {
          errorMessage = parsed.data.message;
        }
      } catch {
        // Keep the generic message when the error body is not JSON.
      }

      throw new ApiError(errorMessage, response.status);
    }

    if (!response.body) {
      throw new Error('No response body');
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let sawFirstByte = false;
    let sawFirstToken = false;

    return new ReadableStream({
      start(controller) {
        function pump(): Promise<void> {
          return reader.read().then(({ done, value }) => {
            if (done) {
              // Turn completed normally — drop the resume marker.
              if (message.sessionId) clearPendingStream(message.sessionId);
              emit('sse.completed', {
                durationMs: Math.round(performance.now() - startedAt),
                props: {
                  route: '/api/chat/stream',
                  session_id: message.sessionId,
                  client_id: message.clientId,
                },
              });
              controller.close();
              return;
            }

            if (!sawFirstByte) {
              sawFirstByte = true;
              emit('sse.first_byte', {
                durationMs: Math.round(performance.now() - startedAt),
                props: {
                  route: '/api/chat/stream',
                  session_id: message.sessionId,
                  client_id: message.clientId,
                },
              });
            }

            const chunk = decoder.decode(value, { stream: true });
            const parsed = parseSseChunk(buffer, chunk);
            buffer = parsed.buffer;

            for (const event of parsed.events) {
              const data = parseJsonSseData<unknown>(event);
              if (!data) {
                continue;
              }

              // Gateway stream request_id → remember so a reload can resume
              // this in-flight turn via /api/chat/resume/{requestId}.
              if (event.event === 'meta') {
                const meta = data as { requestId?: string };
                if (message.sessionId && meta.requestId) {
                  rememberPendingStream(message.sessionId, meta.requestId);
                }
                continue;
              }

              const result = MessageSchema.safeParse(data);
              if (result.success) {
                if (!sawFirstToken && result.data.role === 'assistant' && result.data.content.length > 0) {
                  sawFirstToken = true;
                  emit('sse.first_token', {
                    durationMs: Math.round(performance.now() - startedAt),
                    props: {
                      route: '/api/chat/stream',
                      session_id: message.sessionId,
                      client_id: message.clientId,
                      event_id: event.id,
                    },
                  });
                }
                controller.enqueue(result.data);
              }
            }

            return pump();
          }).catch((error) => {
            emit('sse.aborted', {
              durationMs: Math.round(performance.now() - startedAt),
              props: {
                route: '/api/chat/stream',
                session_id: message.sessionId,
                client_id: message.clientId,
                reason: error instanceof Error ? error.message : 'unknown',
              },
            });
            controller.error(error);
          });
        }

        return pump();
      },
    });
  }
}

// Default client instance
export const chatApiClient = new ChatApiClient();
