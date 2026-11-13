import { describe, expect, it, vi } from 'vitest'
import { createInlineAssistantController } from './inline-assistant'
import type { DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import type { ChatInvokeRequest, ChatStreamHandlers } from '@/shared/api/chat-client'

const payload = (overrides: Partial<DashboardComposerSubmitPayload> = {}): DashboardComposerSubmitPayload => ({
  text: 'Hello', actions: [], attachments: [], tools: [], ...overrides,
})

describe('inline assistant', () => {
  it('streams a real response with the selected model and privacy settings', async () => {
    const states: string[] = []
    let request: ChatInvokeRequest | undefined
    const stream = vi.fn(async (body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      request = body
      handlers.onConnected?.({ requestId: 'request', threadId: 'thread' })
      handlers.onMessage?.({ content: 'Hei' })
      handlers.onDone?.({})
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: (state) => states.push(state.status), dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    await controller.send(payload({ model: 'model', sourceScope: 'conversation', minPrivacyTier: 'sovereign', effort: 'quick' }))
    expect(request).toMatchObject({ content: 'Hello', model: 'model', sourceScope: 'conversation', minPrivacyTier: 'sovereign', effort: 'quick' })
    expect(controller.state().turns).toEqual([{ role: 'user', text: 'Hello' }, { role: 'assistant', text: 'Hei' }])
    expect(controller.state().threadId).toBe('thread')
    expect(states).toContain('streaming')
    expect(controller.state().status).toBe('idle')
  })

  it('preserves the selected subscription and surfaces a route failure without switching models', async () => {
    const stream = vi.fn(async (_body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      handlers.onError?.({ code: 'subscription_route_unavailable', message: 'Subscription route unavailable' })
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })

    await controller.send(payload({ model: 'selected-model', provider: 'openai-codex-subscription', subscriptionConnectionId: 'subscription-connection-123' }))

    await vi.waitFor(() => expect(controller.state().status).toBe('error'))
    expect(controller.state().error).toBe('Could not send the message.')
    expect(stream).toHaveBeenCalledTimes(1)
    expect(stream.mock.calls[0]?.[0]).toMatchObject({
      model: 'selected-model',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'subscription-connection-123',
    })
  })

  it('shows the copy for a known server code in the chosen language, never the server text', async () => {
    const stream = vi.fn(async (_body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      handlers.onError?.({ code: 'thread_run_active', message: 'An agent run is already active on this thread' })
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', tr: (noText) => noText, onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })

    await controller.send(payload())

    await vi.waitFor(() => expect(controller.state().status).toBe('error'))
    expect(controller.state().error).toBe('Agenten jobber allerede i denne samtalen. Meldingen er lagt ut. Prøv igjen når den er ferdig.')
  })

  it('shows the fallback for a code it has no copy for, never the English server text', async () => {
    const stream = vi.fn(async (_body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      handlers.onError?.({ code: 'model_gateway_exploded', message: 'Model gateway exploded: upstream returned no bytes' })
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', tr: (noText) => noText, onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })

    await controller.send(payload())

    await vi.waitFor(() => expect(controller.state().status).toBe('error'))
    expect(controller.state().error).toBe('Kunne ikke sende meldingen.')
    expect(controller.state().error).not.toContain('Model gateway')
  })

  it('keeps temporary chats in memory and does not send durable message IDs', async () => {
    let request: ChatInvokeRequest | undefined
    const stream = vi.fn(async (body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      request = body
      handlers.onConnected?.({ threadId: 'ignored' })
      handlers.onDone?.({})
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    await controller.send(payload({ zdr: true }))
    expect(request?.zdr).toBe(true)
    expect(request?.clientMessageId).toBeUndefined()
    expect(controller.state().threadId).toBeNull()
  })

  it('flags citations and questions for the full chat instead of hiding them', async () => {
    const stream = vi.fn(async (_body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req', threadId: 'thread' })
      handlers.onCitation?.({ title: 'Source' })
      handlers.onDone?.({})
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    await controller.send(payload())
    expect(controller.state().needsFullChat).toBe(true)
    expect(controller.state().threadId).toBe('thread')
  })

  it('clears the prior escalation when switching to a new temporary conversation', async () => {
    let count = 0
    const stream = vi.fn(async (_body: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req', threadId: 'thread' })
      if (count++ === 0) handlers.onCitation?.({ title: 'Source' })
      handlers.onDone?.({})
    })
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    await controller.send(payload())
    await vi.waitFor(() => expect(controller.state().status).toBe('idle'))
    expect(controller.state().needsFullChat).toBe(true)
    await controller.send(payload({ zdr: true }))
    expect(controller.state().needsFullChat).toBe(false)
    expect(controller.state().threadId).toBeNull()
  })

  it('refuses rich payloads before dispatch so the caller can open full Chat', async () => {
    const stream = vi.fn()
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    await expect(controller.send(payload({ tools: ['research'] }))).rejects.toThrow(/open chat/i)
    expect(stream).not.toHaveBeenCalled()
  })

  it('sends Stop after a late connected event and waits for cancellation confirmation', async () => {
    let handlers: ChatStreamHandlers | undefined
    let finish!: () => void
    const stream = vi.fn(async (_body: ChatInvokeRequest, callbacks: ChatStreamHandlers) => {
      handlers = callbacks
      await new Promise<void>((resolve) => { finish = resolve })
    })
    const cancelInvocation = vi.fn().mockResolvedValue(undefined)
    const cancelRun = vi.fn().mockResolvedValue({})
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation, cancelRun } })
    const sent = controller.send(payload())
    await vi.waitFor(() => expect(handlers).toBeDefined())
    controller.stop()
    expect(controller.state().status).toBe('stopping')
    handlers?.onConnected?.({ requestId: 'request', runId: 'run' })
    await vi.waitFor(() => expect(controller.state().status).toBe('stopped'))
    expect(cancelInvocation).toHaveBeenCalledWith('request')
    expect(cancelRun).toHaveBeenCalledWith('run')
    finish()
    await sent
  })

  it('refuses to send a message if the workspace changes during preparation', async () => {
    let currentScope = 'user:org-a'
    const stream = vi.fn()
    const controller = createInlineAssistantController({ scope: () => currentScope, onChange: () => {}, dependencies: { stream, cancelInvocation: vi.fn(), cancelRun: vi.fn() } })
    const sent = controller.send(payload())
    currentScope = 'user:org-b'
    await expect(sent).rejects.toThrow(/workspace changed/i)
    expect(stream).not.toHaveBeenCalled()
  })

  it('clears private state on reset and cancels a late connected run', async () => {
    let handlers: ChatStreamHandlers | undefined
    let finish!: () => void
    const stream = vi.fn(async (_body: ChatInvokeRequest, callbacks: ChatStreamHandlers) => {
      handlers = callbacks
      await new Promise<void>((resolve) => { finish = resolve })
    })
    const cancelInvocation = vi.fn().mockResolvedValue(undefined)
    const controller = createInlineAssistantController({ scope: () => 'user:org', onChange: () => {}, dependencies: { stream, cancelInvocation, cancelRun: vi.fn().mockResolvedValue({}) } })
    const sent = controller.send(payload())
    await vi.waitFor(() => expect(handlers).toBeDefined())
    controller.reset()
    handlers?.onConnected?.({ requestId: 'late' })
    expect(cancelInvocation).toHaveBeenCalledWith('late')
    expect(controller.state().turns).toEqual([])
    finish()
    await sent
  })
})
