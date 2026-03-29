import { NextRequest } from 'next/server'
import { ZodError } from 'zod'
import { randomUUID } from 'crypto'

import {
  appendMessagesToSession,
  buildChatTitle,
  ensureChatSession,
  type StoredChatMessage,
} from '../_lib/session-store'
import {
  createAnswerChunks,
  parseChatRequest,
  requestReasoningPlaneAnswer,
} from '../_lib/reasoning-plane'

function createSseResponseChunk(payload: object, encoder: TextEncoder) {
  return encoder.encode(`data: ${JSON.stringify(payload)}\n\n`)
}

function createErrorMessage(message: string, sessionId: string, error: string): StoredChatMessage {
  return {
    id: randomUUID(),
    content: message,
    role: 'assistant',
    timestamp: new Date().toISOString(),
    sessionId,
    isThinking: false,
    error,
  }
}

export async function POST(request: NextRequest) {
  const encoder = new TextEncoder()

  try {
    const body = await request.json()
    const parsedRequest = parseChatRequest(body)

    const session = ensureChatSession({
      sessionId: parsedRequest.sessionId,
      title: buildChatTitle(parsedRequest.content),
      userId: parsedRequest.userId,
    })

    const userMessage: StoredChatMessage = {
      id: randomUUID(),
      content: parsedRequest.content,
      role: 'user',
      timestamp: new Date().toISOString(),
      sessionId: session.id,
    }

    const stream = new ReadableStream({
      async start(controller) {
        const enqueue = (payload: object) => controller.enqueue(createSseResponseChunk(payload, encoder))

        try {
          const reasoningResult = await requestReasoningPlaneAnswer({
            content: parsedRequest.content,
            history: session.messages,
            sessionId: session.id,
            userId: parsedRequest.userId,
            userName: parsedRequest.userName,
            userEmail: parsedRequest.userEmail,
          })

          const assistantMessageId = randomUUID()
          const assistantTimestamp = new Date().toISOString()
          const answerChunks = createAnswerChunks(reasoningResult.answer)
          let accumulated = ''

          for (const chunk of answerChunks) {
            accumulated += chunk

            enqueue({
              id: assistantMessageId,
              content: accumulated,
              role: 'assistant',
              timestamp: assistantTimestamp,
              sessionId: session.id,
              isThinking: true,
            })

            await new Promise((resolve) => setTimeout(resolve, 12))
          }

          const assistantMessage: StoredChatMessage = {
            id: assistantMessageId,
            content: reasoningResult.answer,
            role: 'assistant',
            timestamp: assistantTimestamp,
            sessionId: session.id,
            isThinking: false,
            metadata: reasoningResult.metadata,
          }

          appendMessagesToSession({
            sessionId: session.id,
            title: session.title,
            userId: parsedRequest.userId,
            messages: [userMessage, assistantMessage],
          })

          enqueue(assistantMessage)
        } catch (error) {
          const assistantMessage = createErrorMessage(
            'Kunne ikke hente svar fra Reasoning Plane. Prøv igjen.',
            session.id,
            error instanceof Error ? error.message : 'unknown',
          )

          appendMessagesToSession({
            sessionId: session.id,
            title: session.title,
            userId: parsedRequest.userId,
            messages: [userMessage, assistantMessage],
          })

          enqueue(assistantMessage)
        } finally {
          controller.close()
        }
      },
    })

    return new Response(stream, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      },
    })
  } catch (error) {
    if (error instanceof ZodError) {
      return new Response(
        JSON.stringify({
          error: 'Validation failed',
          message: error.issues[0]?.message ?? 'Invalid request body',
          statusCode: 400,
        }),
        {
          status: 400,
          headers: {
            'Content-Type': 'application/json',
          },
        },
      )
    }

    return new Response(
      JSON.stringify({
        error: 'Reasoning plane unavailable',
        message: error instanceof Error ? error.message : 'Failed to initialize stream',
        statusCode: 502,
      }),
      {
        status: 502,
        headers: {
          'Content-Type': 'application/json',
        },
      },
    )
  }
}
