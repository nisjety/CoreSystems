import { requestJson } from '@/shared/api/http'

/**
 * Inbox AI assist — real model-plane calls.
 *
 * The gateway already fronts the model plane at POST /api/v1/chat/invoke
 * (mints the delegated inference/execution/cost/session tokens and proxies to
 * model-gateway /v1/invoke). We assemble the conversation transcript into a
 * mode-specific prompt and return the generated text + any grounded sources.
 * This replaces the previous no-op Velion actions — every call is a genuine
 * model invocation.
 */

export type AssistMode = 'draft' | 'summarize' | 'intent' | 'ask'

export interface AssistSource {
  title?: string
  uri?: string
  excerpt?: string
}

export interface AssistResult {
  text: string
  sources: AssistSource[]
  model?: string
}

export interface AssistMessage {
  agent: boolean
  from?: string
  body: string
}

export interface AssistOptions {
  instruction?: string
  question?: string
  customer?: string
}

function transcriptText(messages: AssistMessage[]): string {
  return messages
    .filter((m) => m.body.trim())
    .map((m) => `${m.agent ? 'Agent (us)' : m.from || 'Customer'}: ${m.body.trim()}`)
    .join('\n\n')
}

function buildPrompt(mode: AssistMode, messages: AssistMessage[], opts: AssistOptions): string {
  const convo = transcriptText(messages)
  const who = opts.customer ? `the customer (${opts.customer})` : 'the customer'
  const header =
    `You are Velion, an AI customer-support agent working a shared inbox. Below is the ` +
    `conversation transcript with ${who}, oldest first.\n\n` +
    `----- TRANSCRIPT -----\n${convo || '(no messages yet)'}\n----- END TRANSCRIPT -----\n\n`

  switch (mode) {
    case 'draft':
      return (
        header +
        `Write the next reply the support agent should send to the customer. Be concise, warm, ` +
        `and professional; acknowledge their issue and give a clear next step. ` +
        `${opts.instruction ? `Extra instruction: ${opts.instruction}. ` : ''}` +
        `Return ONLY the message body — no subject line, no "Hi team", no preamble or sign-off placeholders like [Name].`
      )
    case 'summarize':
      return (
        header +
        `Summarize this conversation for an agent picking it up. Use 3–5 short bullet points covering: ` +
        `the customer's core issue, what has happened so far, and the current status / next action. Be terse.`
      )
    case 'intent':
      return (
        header +
        `State the customer's primary intent in one sentence. Then, on a new line prefixed "Next:", ` +
        `recommend the single best next action for the agent (e.g. reply, escalate, refund, ask for info).`
      )
    case 'ask':
      return (
        header +
        `The support agent asks: "${opts.question || 'What is the status of this conversation?'}". ` +
        `Answer specifically using only the transcript. If the transcript lacks the answer, say so.`
      )
  }
}

export async function runAssist(
  orgId: string,
  mode: AssistMode,
  messages: AssistMessage[],
  opts: AssistOptions = {},
): Promise<AssistResult> {
  const content = buildPrompt(mode, messages, opts)
  const payload = await requestJson<Record<string, unknown>>('/api/v1/chat/invoke', {
    method: 'POST',
    headers: { 'x-velion-org-id': orgId },
    body: JSON.stringify({
      content,
      profile: 'chat',
      features: [],
      tools: [],
      attachments: [],
      zdr: false,
    }),
  })
  const data = (payload?.data as Record<string, unknown> | undefined) ?? payload
  const text = String(data?.content ?? data?.text ?? '').trim()
  const rawSources = Array.isArray(data?.sources) ? (data.sources as Array<Record<string, unknown>>) : []
  const sources: AssistSource[] = rawSources.map((s) => ({
    title: typeof s.title === 'string' ? s.title : undefined,
    uri: typeof s.uri === 'string' ? s.uri : undefined,
    excerpt: typeof s.excerpt === 'string' ? s.excerpt : undefined,
  }))
  return { text, sources, model: typeof data?.model_used === 'string' ? data.model_used : undefined }
}
