import { NextRequest } from 'next/server'
import { randomUUID } from 'crypto'
import { ZodError } from 'zod'

import {
  appendMessagesToSession,
  buildChatTitle,
  ChatStoreError,
  createStreamingAssistantMessage,
  ensureChatSession,
  finalizeStreamingAssistantMessage,
  resolveChatActor,
  type StoredChatMessage,
  updateStreamingAssistantMessage,
} from '../_lib/session-store'
import {
  createAnswerChunks,
  parseChatRequest,
  requestReasoningPlaneAnswer,
  streamReasoningPlaneAnswer,
} from '../_lib/reasoning-plane'
import { convexQuery } from '@/app/api/_lib/convex-client'
import { detectComplexity, resolveModel } from '../_lib/models'
import { resolveAgentProfile, type PersistedAgent } from '@/components/agents/types'
import { createSseEventId, encodeSseChunk } from '@/lib/sse/server'

export async function POST(request: NextRequest) {
  const encoder = new TextEncoder()

  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsedRequest = parseChatRequest(body)
    const clientId = parsedRequest.clientId
    const lastEventId = parsedRequest.lastEventId ?? request.headers.get('last-event-id') ?? undefined

    const session = await ensureChatSession(actor, {
      sessionId: parsedRequest.sessionId,
      title: buildChatTitle(parsedRequest.content),
    })

    // Resolve agent config if agentId provided (non-blocking on failure)
    let agentConfig: PersistedAgent | null = null
    if (parsedRequest.agentId) {
      agentConfig = await convexQuery<PersistedAgent | null>('agents:getById', {
        agentId: parsedRequest.agentId,
        orgId: actor.convexOrgId,
      }).catch(() => null)
    }

    // Route to correct model based on agent config + complexity
    const complexity = detectComplexity(parsedRequest.content, session.messages.length)
    const resolvedModel = resolveModel({
      requestModel: parsedRequest.model,
      agentModel: agentConfig?.model,
      complexity,
      allowUpgrade: true,
    })

    const userMessage: StoredChatMessage = {
      id: randomUUID(),
      clientId,
      content: parsedRequest.content,
      role: 'user',
      timestamp: new Date().toISOString(),
      sessionId: session.id,
    }

    await appendMessagesToSession(actor, {
      sessionId: session.id,
      title: session.title,
      messages: [userMessage],
    })

    const stream = new ReadableStream({
      async start(controller) {
        // U2-4 (ui-ux-velion-gap.md §10): session lifetime is bound to AI
        // work completion, not to the browser connection. If the user
        // closes the tab / switches pane mid-stream, we keep writing chunks
        // to Convex so the conversation finishes correctly — a refreshed
        // tab or a second pane re-subscribes via `conversations:get` and
        // sees the real answer reactively, not a half-written error.
        //
        // Previously the catch handler treated *any* error inside the loop
        // (including `controller.enqueue` throwing on a closed SSE pipe)
        // as a reasoning failure and overwrote the assistant message in
        // Convex with `Kunne ikke hente svar fra Model Plane`. That made
        // a successful gateway response invisible to other panes whenever
        // the original pane disconnected before the loop drained.
        //
        // The fix splits SSE writes from Convex writes:
        //   - `safeEnqueue` swallows write failures (broken pipe is
        //     expected when the client is gone).
        //   - Convex writes keep going regardless.
        //   - The outer catch only fires when the *gateway* call itself
        //     threw — in which case we still finalize with an error so
        //     the placeholder doesn't sit in an in-flight limbo state.
        let sseAlive = true
        const safeEnqueue = (payload: object, event = 'message'): void => {
          if (!sseAlive) return
          try {
            controller.enqueue(encodeSseChunk(payload, encoder, {
              id: createSseEventId(clientId ?? session.id),
              event,
            }))
          } catch {
            // The SSE consumer is gone (closed tab, network drop, abort).
            // Mark the stream dead so we stop attempting writes — Convex
            // remains the source of truth for the remaining work.
            sseAlive = false
          }
        }

        const placeholderMessage = await createStreamingAssistantMessage(actor, session.id)
        if (lastEventId) {
          safeEnqueue({ type: 'resume_ack', lastEventId }, 'resume')
        }
        safeEnqueue(placeholderMessage)

        // Decide which path to use:
        //   - Real streaming (gateway /v1/invoke/stream) for plain chat —
        //     emits tokens as they arrive, ~50ms first-token instead of
        //     full-answer round-trip.
        //   - Orchestrator (gateway /v1/invoke or /v1/research) for tool-use
        //     agents or deep research; those need the orchestrator's
        //     multi-step output that the SSE endpoint bypasses.
        const useStreaming =
          !agentConfig?.tools?.length && parsedRequest.responseMode !== 'deep'

        try {
          if (useStreaming) {
            // Real token-by-token streaming. Each gateway delta is
            // appended to Convex via `messages:updateStreaming` so any
            // subscribed pane (or a reopened tab) sees the same growing
            // answer. SSE is still emitted for callers that want a
            // direct pipe without the Convex subscription.
            let assembled = ''
            let lastConvexWriteAt = Date.now()
            let pendingBuffer = ''
            const FLUSH_MS = 60
            const FLUSH_CHARS = 24
            let streamedAssistantMessage = placeholderMessage
            let modelUsed = resolvedModel

            const flush = async () => {
              if (pendingBuffer.length === 0) return
              const toWrite = pendingBuffer
              pendingBuffer = ''
              streamedAssistantMessage = await updateStreamingAssistantMessage(
                actor,
                streamedAssistantMessage.id,
                toWrite,
              )
              safeEnqueue(streamedAssistantMessage)
              lastConvexWriteAt = Date.now()
            }

            for await (const event of streamReasoningPlaneAnswer({
              content: parsedRequest.content,
              history: session.messages,
              sessionId: session.id,
              userId: actor.userId,
              userName: actor.userName,
              userEmail: actor.userEmail,
              model: resolvedModel,
              responseMode: parsedRequest.responseMode,
              profile: resolveAgentProfile(agentConfig),
              cookieHeader: request.headers.get('cookie') ?? '',
            })) {
              if (event.type === 'meta') {
                // Surface the gateway stream request_id so the browser can
                // persist it and resume the in-flight answer on reload via
                // /api/chat/resume/{requestId} (HARNESS_PHASE1 §3b).
                safeEnqueue({ type: 'stream_meta', requestId: event.requestId }, 'meta')
              } else if (event.type === 'delta') {
                assembled += event.delta
                pendingBuffer += event.delta
                if (
                  pendingBuffer.length >= FLUSH_CHARS ||
                  Date.now() - lastConvexWriteAt >= FLUSH_MS
                ) {
                  await flush()
                }
              } else if (event.type === 'done') {
                modelUsed = event.modelUsed || modelUsed
                await flush()
              }
            }

            // Final flush + finalize.
            await flush()
            const assistantMessage = await finalizeStreamingAssistantMessage(actor, {
              messageId: streamedAssistantMessage.id,
              content: assembled,
              metadata: {
                source: 'reasoning-plane',
                citations: undefined,
              },
            })
            safeEnqueue(assistantMessage)
            // No tool-trace / runId on the streaming path — those are
            // orchestrator outputs that the SSE endpoint bypasses.
            return
          }

          const reasoningResult = await requestReasoningPlaneAnswer({
            content: parsedRequest.content,
            history: session.messages,
            sessionId: session.id,
            userId: actor.userId,
            userName: actor.userName,
            userEmail: actor.userEmail,
            model: resolvedModel,
            responseMode: parsedRequest.responseMode,
            browseWeb: parsedRequest.browseWeb,
            // U2-5: forward the session cookie so reasoning.ts can mint a
            // real Model Plane JWT against auth-core for this user. Note:
            // we intentionally do NOT forward `request.signal` so a browser
            // disconnect does not abort the gateway call.
            cookieHeader: request.headers.get('cookie') ?? '',
            // §15 (ui-ux-velion-gap.md): when the call is on behalf of a
            // configured agent, forward its enabled tool ids so the
            // gateway runs the multi-round tool-use loop
            // (`tool_loop::run_tool_loop`). Non-agent chat (no
            // `agentId` on the request) leaves this undefined and the
            // gateway falls back to its single-turn legacy path.
            tools: agentConfig?.tools,
          })

          const answerChunks = createAnswerChunks(reasoningResult.answer)
          let streamedAssistantMessage = placeholderMessage

          for (const chunk of answerChunks) {
            // Convex first — that's the durable side. SSE is best-effort.
            streamedAssistantMessage = await updateStreamingAssistantMessage(
              actor,
              streamedAssistantMessage.id,
              chunk,
            )
            safeEnqueue(streamedAssistantMessage)
            // The 12ms cosmetic pacing is preserved so SSE consumers still
            // see a typing-like cadence. When SSE is dead we could skip the
            // sleep, but keeping it makes the Convex write rate identical
            // either way — important for any subscribed pane.
            await new Promise((resolve) => setTimeout(resolve, 12))
          }

          const assistantMessage = await finalizeStreamingAssistantMessage(actor, {
            messageId: streamedAssistantMessage.id,
            content: reasoningResult.answer,
            metadata: reasoningResult.metadata,
          })

          safeEnqueue(assistantMessage)

          // Wave 9 §19: surface the gateway's per-turn tool-loop trace as
          // a trailing SSE envelope. The `useAgentPlayground` hook keys
          // off `metadata.toolTrace` on any event to populate the
          // collapsible trace panel under the assistant turn. We send
          // this *after* the finalized message so consumers that only
          // care about content can ignore it without parsing tool data.
          //
          // We intentionally do NOT persist the trace to Convex: the
          // arg/result previews can contain user-supplied data and live
          // logs we don't want long-term in chat history. The playground
          // is ephemeral — the in-memory hook state is the right home.
          if (reasoningResult.toolTrace && reasoningResult.toolTrace.length > 0) {
            safeEnqueue({
              type: 'tool_trace',
              metadata: { toolTrace: reasoningResult.toolTrace },
            }, 'tool_trace')
          }

          // Wave 11 §5: forward the gateway's run id so the playground
          // can attach Fin G/A/P ratings to the underlying agentRuns row.
          // Same rationale as the tool-trace envelope above — kept on
          // the SSE side only, never persisted to Convex chat history.
          if (reasoningResult.runId) {
            safeEnqueue({
              type: 'run_id',
              metadata: { runId: reasoningResult.runId },
            }, 'run_id')
          }
        } catch (error) {
          console.error('[chat-stream] Model Plane answer failed', {
            sessionId: session.id,
            model: resolvedModel,
            message:
              error instanceof Error
                ? error.message.slice(0, 500)
                : String(error).slice(0, 500),
          })

          // Reasoning genuinely failed — record it so any subscribed pane
          // sees a deterministic terminal state instead of a placeholder
          // that never resolves.
          const assistantMessage = await finalizeStreamingAssistantMessage(actor, {
            messageId: placeholderMessage.id,
            content: 'Kunne ikke hente svar fra Model Plane. Prøv igjen.',
            error: error instanceof Error ? error.message : 'unknown',
          })

          safeEnqueue(assistantMessage)
        } finally {
          // controller.close() is a no-op when the consumer already
          // disconnected; wrap defensively so a late close doesn't unwind
          // the stack with a hard error.
          try {
            controller.close()
          } catch {
            /* SSE already torn down */
          }
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
    if (error instanceof ChatStoreError) {
      return new Response(
        JSON.stringify({
          error: 'Chat storage unavailable',
          message: error.message,
          statusCode: error.statusCode,
        }),
        {
          status: error.statusCode,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    }

    if (error instanceof ZodError) {
      return new Response(
        JSON.stringify({
          error: 'Validation failed',
          message: error.issues[0]?.message ?? 'Invalid request body',
          statusCode: 400,
        }),
        {
          status: 400,
          headers: { 'Content-Type': 'application/json' },
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
        headers: { 'Content-Type': 'application/json' },
      },
    )
  }
}
