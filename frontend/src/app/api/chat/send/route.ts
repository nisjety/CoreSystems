import { NextRequest, NextResponse } from 'next/server'
import { ZodError } from 'zod'
import { randomUUID } from 'crypto'

import { parseChatRequest, requestReasoningPlaneAnswer } from '../_lib/reasoning-plane'
import {
  appendMessagesToSession,
  buildChatTitle,
  ensureChatSession,
  type StoredChatMessage,
} from '../_lib/session-store'

export async function POST(request: NextRequest) {
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

    const reasoningResult = await requestReasoningPlaneAnswer({
      content: parsedRequest.content,
      history: session.messages,
      sessionId: session.id,
      userId: parsedRequest.userId,
      userName: parsedRequest.userName,
      userEmail: parsedRequest.userEmail,
    })

    const assistantMessage: StoredChatMessage = {
      id: randomUUID(),
      content: reasoningResult.answer,
      role: 'assistant',
      timestamp: new Date().toISOString(),
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

    return NextResponse.json(assistantMessage)
  } catch (error) {
    if (error instanceof ZodError) {
      return NextResponse.json(
        {
          error: 'Validation failed',
          message: error.issues[0]?.message ?? 'Invalid request body',
          statusCode: 400,
        },
        { status: 400 },
      )
    }

    return NextResponse.json(
      {
        error: 'Reasoning plane unavailable',
        message: error instanceof Error ? error.message : 'Failed to get chat response',
        statusCode: 502,
      },
      { status: 502 },
    )
  }
}
