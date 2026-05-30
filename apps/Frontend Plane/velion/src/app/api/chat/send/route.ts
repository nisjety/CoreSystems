import { NextRequest, NextResponse } from 'next/server'
import { ZodError } from 'zod'
import { randomUUID } from 'crypto'

import { parseChatRequest, requestReasoningPlaneAnswer } from '../_lib/reasoning-plane'
import {
  appendMessagesToSession,
  buildChatTitle,
  ChatStoreError,
  ensureChatSession,
  resolveChatActor,
  type StoredChatMessage,
} from '../_lib/session-store'

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsedRequest = parseChatRequest(body)

    const session = await ensureChatSession(actor, {
      sessionId: parsedRequest.sessionId,
      title: buildChatTitle(parsedRequest.content),
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
      userId: actor.userId,
      userName: actor.userName,
      userEmail: actor.userEmail,
      // U2-5: forward the session cookie so reasoning.ts can mint a real
      // Model Plane JWT against auth-core for this user.
      cookieHeader: request.headers.get('cookie') ?? '',
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

    await appendMessagesToSession(actor, {
      sessionId: session.id,
      title: session.title,
      messages: [userMessage, assistantMessage],
    })

    return NextResponse.json(assistantMessage)
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json(
        {
          error: 'Chat storage unavailable',
          message: error.message,
          statusCode: error.statusCode,
        },
        { status: error.statusCode },
      )
    }

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
