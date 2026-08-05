type ReasoningRequest = {
  query: string
  strategy?: string
  depth?: string
  context?: {
    session_id?: string
    [key: string]: unknown
  }
  require_citations?: boolean
  enable_verification?: boolean
  model?: string
  enable_web_search?: boolean
  /**
   * §15 (ui-ux-verevon-gap.md): agent tool-use loop. When present and
   * non-empty, forwarded to the gateway's `/v1/invoke` as `tools` so
   * `tool_loop::run_tool_loop` drives the conversation with the
   * registered tool registry.
   */
  tools?: readonly string[]
  /**
   * Harness profile forwarded to the gateway so it can set the approval
   * posture (HARNESS_PHASE1 §1). "deployed_agent" → ask/gate; "chat" → auto.
   */
  profile?: 'chat' | 'deployed_agent'
}

function isEnabled(value: string | undefined) {
  if (!value) {
    return false
  }

  const normalized = value.trim().toLowerCase()
  return normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'on'
}

export function isRustModelPlaneEnabled() {
  return isEnabled(process.env.MODEL_PLANE_RUST_ENABLED)
}

function legacyReasoningUrl() {
  return process.env.REASONING_CORE_URL || 'http://localhost:8101'
}

function modelGatewayUrl() {
  return process.env.MODEL_GATEWAY_URL || 'http://localhost:8080'
}

function buildRustInvokePayload(body: ReasoningRequest) {
  // U2-9 (response_mode) + U2-7/U2-16 (browse_web) + U2-17 (system_prompt):
  // forward all four fields the Rust gateway's `/v1/invoke` now accepts.
  // U2-17 fix: the system_prompt buried inside `context` used to be
  // silently dropped — sentiment / quick-replies / summarize / inbox-draft
  // routes all carry careful instructions ("Return ONLY a JSON object…")
  // and the model never saw them, so downstream JSON parsing failed.
  const responseMode = (() => {
    // Phase 3 (Model Plane token optimisations): the gateway accepts a new
    // `terse` response_mode for telegraphic replies (max_tokens=256). Verevon
    // callers can opt in by setting `body.depth === 'terse'`; legacy values
    // continue to map as before.
    if (body.depth === 'terse') return 'terse'
    if (body.depth === 'fast') return 'quick'
    if (body.depth === 'deep') return 'deep'
    return 'auto'
  })()

  // Accept system_prompt from `context.system_prompt` (the legacy shape
  // every existing caller uses). Empty/whitespace strings are dropped so
  // the gateway treats them as absent.
  const systemPromptRaw = body.context?.['system_prompt']
  const systemPrompt =
    typeof systemPromptRaw === 'string' && systemPromptRaw.trim()
      ? systemPromptRaw
      : undefined

  return {
    content: body.query,
    model: body.model,
    session_key: body.context?.session_id,
    response_mode: responseMode,
    browse_web: body.enable_web_search ?? false,
    system_prompt: systemPrompt,
    // §15: pass agent tools through to the gateway. When omitted /
    // empty, the gateway falls back to its legacy single-turn path
    // (no tool loop) so non-agent chat keeps working unchanged.
    tools: body.tools && body.tools.length > 0 ? Array.from(body.tools) : undefined,
  }
}

async function normalizeRustResponse(response: Response, query: string) {
  const text = await response.text()
  if (!response.ok) {
    return new Response(text, {
      status: response.status,
      headers: { 'Content-Type': response.headers.get('Content-Type') || 'text/plain' },
    })
  }

  const payload = text ? JSON.parse(text) as {
    content?: string
    model_used?: string
    stop_reason?: string
    input_tokens?: number
    output_tokens?: number
    /**
     * Wave 11 §5 — model-gateway's `/v1/invoke` returns the orchestrator
     * run id so the App Shell can attach operator feedback to it via
     * `POST /api/agents/runs/{runId}/rate`. Snake-case from the Rust
     * service; we re-emit it as camelCase `runId` for the SSE consumer.
     */
    run_id?: string
    /**
     * Wave 9 §19: model-gateway's `/v1/invoke` returns a per-turn
     * tool-loop trace when the agent invoked tools. Each entry is one
     * tool execution within one round of the multi-round loop. We
     * forward it untouched in `metadata.tool_trace` so the chat-stream
     * route can re-emit it on the final SSE event for the playground
     * trace panel (`useAgentPlayground` reads `metadata.toolTrace`).
     */
    tool_trace?: Array<{
      round: number
      tool: string
      args_preview: string
      result_preview: string
      result_bytes: number
    }>
  } : {}

  return new Response(
    JSON.stringify({
      query,
      answer: payload.content ?? '',
      reasoning_trace: [],
      strategy_used: 'model-gateway',
      confidence: 0,
      reasoning_time_ms: 0,
      alternative_explanations: [],
      verification_result: null,
      metadata: {
        provider: 'model-gateway',
        model_used: payload.model_used ?? '',
        stop_reason: payload.stop_reason ?? '',
        input_tokens: payload.input_tokens ?? 0,
        output_tokens: payload.output_tokens ?? 0,
        // Wave 9 §19: surface the tool-loop trace untouched. Empty
        // when the model did not invoke any tools (legacy single-turn
        // path) — chat-stream route only emits it when non-empty.
        tool_trace: payload.tool_trace ?? [],
        // Wave 11 §5: forward run id (camelCase) so chat-stream can
        // re-emit it on the SSE envelope; the playground hook reads
        // it off `event.metadata.runId` to attach Fin G/A/P ratings.
        runId: payload.run_id ?? null,
      },
    }),
    {
      status: response.status,
      headers: { 'Content-Type': 'application/json' },
    },
  )
}

export async function invokeReasoning(
  body: ReasoningRequest,
  init?: {
    headers?: HeadersInit
    signal?: AbortSignal
    /**
     * U2-5: forward this cookie header to auth-core when minting the
     * gateway JWT. Pass `request.headers.get('cookie') ?? ''` from a
     * Next.js route handler. Without a cookie, the call falls back to
     * service-to-service mint (internal API key) or the dev-bypass
     * token depending on `MODEL_PLANE_USE_DEV_BYPASS`.
     */
    cookieHeader?: string
    /**
     * U2-5: optional service-to-service identity. When set AND no
     * `cookieHeader` is provided, mints a token via the internal-key
     * path. Use for background workers or any caller without a user
     * session. Both `userId` and `orgId` are required.
     */
    internalClaims?: {
      userId: string
      orgId: string
      email?: string
      scopes?: readonly string[]
    }
  },
) {
  if (!isRustModelPlaneEnabled()) {
    return fetch(`${legacyReasoningUrl()}/api/v1/reason`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(init?.headers ?? {}),
      },
      body: JSON.stringify(body),
      signal: init?.signal,
    })
  }

  // U2-5 (verevon ui-ux-verevon-gap.md §10): model-gateway's auth middleware
  // (require_auth in model-gateway/src/auth.rs) validates an RS256 JWT
  // against auth-core's JWKS in production. We mint that token here:
  //   - When `init.cookieHeader` is provided → forward to auth-core
  //     `/api/model-plane/token` (browser session path).
  //   - When `init.internalClaims` is provided → mint via
  //     `/api/model-plane/internal-token` (service-to-service path).
  //   - Otherwise → fall back to env-var bearer + `dev-bypass` literal.
  //     This last branch only works against gateways that have
  //     MODEL_GATEWAY_AUTH_DEV_BYPASS=1. Prod compose unsets that flag.
  const { getModelPlaneTokenFromCookie, getModelPlaneTokenInternal } =
    await import('./auth-token')

  let bearerToken: string
  if (init?.cookieHeader) {
    bearerToken = await getModelPlaneTokenFromCookie(init.cookieHeader)
  } else if (init?.internalClaims) {
    bearerToken = await getModelPlaneTokenInternal(init.internalClaims)
  } else {
    bearerToken =
      process.env.MODEL_GATEWAY_BEARER ??
      process.env.INTERNAL_API_KEY ??
      process.env.INTERNAL_SERVICE_SECRET ??
      'dev-bypass'
  }

  const authHeaders = {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${bearerToken}`,
    ...(init?.headers ?? {}),
  }

  // U2-8 (verevon ui-ux-verevon-gap.md §12): Deep Search routes to the
  // gateway's `/v1/research` endpoint instead of `/v1/invoke`. That
  // endpoint runs a multi-step research loop (plan → fetch/search →
  // synthesize) which is what the composer's "Deep" mode advertises.
  // Other modes (quick / auto) keep the cheaper `/v1/invoke` path.
  if (body.depth === 'deep') {
    const response = await fetch(`${modelGatewayUrl()}/v1/research`, {
      method: 'POST',
      headers: authHeaders,
      body: JSON.stringify({
        goal: body.query,
        // Conservative ceilings so a runaway loop can't drain budget.
        max_iterations: 4,
        max_cost_usd: 1.0,
      }),
      signal: init?.signal,
    })
    return normalizeRustResearchResponse(response, body.query)
  }

  const response = await fetch(`${modelGatewayUrl()}/v1/invoke`, {
    method: 'POST',
    headers: authHeaders,
    body: JSON.stringify(buildRustInvokePayload(body)),
    signal: init?.signal,
  })

  return normalizeRustResponse(response, body.query)
}

// U2-8 helper: massage `/v1/research` output into the same envelope shape
// that `/api/chat/stream` consumes (`{ query, answer, ... }`). The
// research endpoint returns `synthesis` as the final user-facing text
// and a `plan` array we surface as a coarse reasoning trace.
async function normalizeRustResearchResponse(response: Response, query: string) {
  const text = await response.text()
  if (!response.ok) {
    return new Response(text, {
      status: response.status,
      headers: { 'Content-Type': response.headers.get('Content-Type') || 'text/plain' },
    })
  }

  const payload = text
    ? (JSON.parse(text) as {
        research_id?: string
        synthesis?: string
        status?: unknown
        iterations?: number
        plan?: Array<{ kind: string }>
        task_results?: unknown[]
        model?: string
        total_cost_usd?: number
      })
    : {}

  const synthesis = (payload.synthesis ?? '').trim()
  // When the loop returned no synthesis (e.g. no Quarry executor wired)
  // we surface a clear message rather than an empty answer.
  const answer =
    synthesis ||
    'Deep research completed but produced no synthesis — check that a Quarry executor URL is configured for /v1/research.'

  return new Response(
    JSON.stringify({
      query,
      answer,
      reasoning_trace: (payload.plan ?? []).map((step) => ({ step: step.kind })),
      strategy_used: 'deep-research',
      confidence: 0,
      reasoning_time_ms: 0,
      alternative_explanations: [],
      verification_result: null,
      metadata: {
        provider: 'model-gateway',
        research_id: payload.research_id ?? '',
        iterations: payload.iterations ?? 0,
        task_count: Array.isArray(payload.task_results) ? payload.task_results.length : 0,
        model_used: payload.model ?? '',
        total_cost_usd: payload.total_cost_usd ?? 0,
        status: payload.status ?? '',
      },
    }),
    {
      status: response.status,
      headers: { 'Content-Type': 'application/json' },
    },
  )
}

/**
 * Streaming counterpart to `invokeReasoning`. Posts to the gateway's
 * `/v1/invoke/stream` SSE endpoint (defined in
 * `model-gateway/src/sse.rs::invoke_stream_sse`) and yields each `delta`
 * chunk as it arrives. The gateway emits `event:` lines with JSON
 * `{ request_id, delta, done, model_used, ... }` plus a final
 * `event: done` sentinel.
 *
 * The async generator completes when the stream ends OR when the
 * caller's AbortSignal fires. Errors from the gateway (non-2xx
 * responses, malformed SSE) are surfaced as thrown errors so the
 * caller can mark the assistant message as failed.
 *
 * Only used for the streaming chat path — tools / deep-research / agent
 * flows continue to use `invokeReasoning` because those need the
 * orchestrator's tool-loop output that the SSE path bypasses.
 */
export async function* invokeReasoningStream(
  body: ReasoningRequest,
  init?: {
    signal?: AbortSignal
    cookieHeader?: string
    internalClaims?: {
      userId: string
      orgId: string
      email?: string
      scopes?: readonly string[]
    }
  },
): AsyncGenerator<
  | { type: 'meta'; requestId: string }
  | { type: 'delta'; delta: string }
  | { type: 'done'; modelUsed: string; inputTokens: number; outputTokens: number },
  void,
  void
> {
  if (!isRustModelPlaneEnabled()) {
    throw new Error('invokeReasoningStream requires MODEL_PLANE_RUST_ENABLED')
  }

  const { getModelPlaneTokenFromCookie, getModelPlaneTokenInternal } =
    await import('./auth-token')

  let bearerToken: string
  if (init?.cookieHeader) {
    bearerToken = await getModelPlaneTokenFromCookie(init.cookieHeader)
  } else if (init?.internalClaims) {
    bearerToken = await getModelPlaneTokenInternal(init.internalClaims)
  } else {
    bearerToken =
      process.env.MODEL_GATEWAY_BEARER ??
      process.env.INTERNAL_API_KEY ??
      process.env.INTERNAL_SERVICE_SECRET ??
      'dev-bypass'
  }

  // Reshape into the simpler `/v1/invoke/stream` payload. The SSE
  // endpoint takes `content` + optional `model`; conversation history
  // is encoded into `content` upstream (same approach the non-stream
  // path uses via `buildConversationContext`).
  const payload = {
    content: body.query,
    model: body.model,
    profile: body.profile,
  }

  const response = await fetch(`${modelGatewayUrl()}/v1/invoke/stream`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${bearerToken}`,
      Accept: 'text/event-stream',
    },
    body: JSON.stringify(payload),
    signal: init?.signal,
  })

  if (!response.ok || !response.body) {
    const detail = await response.text().catch(() => '')
    throw new Error(`invoke/stream ${response.status}: ${detail.slice(0, 200)}`)
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let emittedMeta = false

  try {
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })

      // SSE events are delimited by blank lines. Parse all complete
      // events we have so far and leave the partial tail in `buffer`.
      let sep = buffer.indexOf('\n\n')
      while (sep !== -1) {
        const rawEvent = buffer.slice(0, sep)
        buffer = buffer.slice(sep + 2)
        sep = buffer.indexOf('\n\n')

        let eventName = 'message'
        const dataLines: string[] = []
        for (const line of rawEvent.split('\n')) {
          if (line.startsWith('event:')) {
            eventName = line.slice(6).trim()
          } else if (line.startsWith('data:')) {
            dataLines.push(line.slice(5).trim())
          }
        }
        if (dataLines.length === 0) continue
        const data = dataLines.join('\n')

        let chunk: {
          request_id: string
          delta: string
          done: boolean
          model_used: string
          input_tokens: number
          output_tokens: number
        }
        try {
          chunk = JSON.parse(data)
        } catch {
          continue
        }

        // Surface the gateway stream request_id once, so the browser can
        // remember it and resume via /v1/invoke/resume/{request_id} on reload.
        if (!emittedMeta && chunk.request_id) {
          emittedMeta = true
          yield { type: 'meta', requestId: chunk.request_id }
        }

        if (eventName === 'done' || chunk.done) {
          yield {
            type: 'done',
            modelUsed: chunk.model_used,
            inputTokens: chunk.input_tokens,
            outputTokens: chunk.output_tokens,
          }
          return
        }

        if (chunk.delta) {
          yield { type: 'delta', delta: chunk.delta }
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

export function reasoningProxyTarget(apiPath: string) {
  if (isRustModelPlaneEnabled() && apiPath === 'reason') {
    return `${modelGatewayUrl()}/v1/invoke`
  }

  return `${legacyReasoningUrl()}/api/v1/${apiPath}`
}

export function translateReasoningProxyBody(apiPath: string, body: Record<string, unknown>) {
  if (isRustModelPlaneEnabled() && apiPath === 'reason') {
    return buildRustInvokePayload(body as ReasoningRequest)
  }

  return body
}

export async function translateReasoningProxyResponse(
  apiPath: string,
  response: Response,
  originalBody: Record<string, unknown>,
) {
  if (isRustModelPlaneEnabled() && apiPath === 'reason') {
    return normalizeRustResponse(response, String(originalBody.query ?? ''))
  }

  return response
}