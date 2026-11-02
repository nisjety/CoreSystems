/**
 * Pure frame builders of the live engine: the product's chat and run events as
 * data, plus the SSE wire form. No timers, no DOM, no state: every function
 * returns the same frames for the same input (frames.test.ts).
 *
 * Timing is expressed as `at`, milliseconds after the anchor the caller picks
 * (the chat POST, or a decision). The channel decides when to write them.
 */
import type { StreamFrame } from './scenario-types'

/** Gap between two streamed text deltas: about 44 words a second. */
export const DELTA_MS = 45

export function frame(at: number, event: string, data: Record<string, unknown>): StreamFrame {
  return { at, event, data }
}

/** The SSE wire form: an `event:` line (and `id:` when present), a `data:` line, a blank line. */
export function encodeFrame(item: Pick<StreamFrame, 'event' | 'data' | 'id'>): string {
  const lines = [`event: ${item.event}`]
  if (item.id) lines.push(`id: ${item.id}`)
  lines.push(`data: ${JSON.stringify(item.data)}`)
  return `${lines.join('\n')}\n\n`
}

/**
 * Splits text into `chunk` deltas of two words each (whitespace kept), one
 * every `stepMs` from `at`. The deltas concatenate back to the exact text, so a
 * streamed answer reads as the markdown it was written as.
 */
export function paced(at: number, text: string, requestId: string, stepMs = DELTA_MS): StreamFrame[] {
  const parts = text.match(/\S+\s*|\s+/g) ?? []
  const frames: StreamFrame[] = []
  for (let index = 0; index < parts.length; index += 2) {
    const delta = parts.slice(index, index + 2).join('')
    frames.push(frame(at + (index / 2) * stepMs, 'chunk', { delta, request_id: requestId }))
  }
  return frames
}

/** The text a chat script streams: every `chunk` delta, in order. */
export function chunkTextOf(frames: readonly StreamFrame[]): string {
  return frames
    .filter((item) => item.event === 'chunk')
    .map((item) => (typeof item.data.delta === 'string' ? item.data.delta : ''))
    .join('')
}

/** The latest `at` of a script, or `fallback` for an empty one. */
export function lastAt(frames: readonly StreamFrame[], fallback = 0): number {
  return frames.reduce((latest, item) => Math.max(latest, item.at), fallback)
}

export function connected(
  at: number,
  input: { requestId: string; threadId: string; model: string; runId?: string },
): StreamFrame {
  return frame(at, 'connected', {
    ok: true,
    request_id: input.requestId,
    thread_id: input.threadId,
    model_used: input.model,
    ...(input.runId ? { run_id: input.runId } : {}),
  })
}

export function titleFrame(at: number, input: { requestId: string; title: string }): StreamFrame {
  return frame(at, 'title', { title: input.title, request_id: input.requestId })
}

export function toolCall(at: number, input: { id: string; name: string; args: Record<string, unknown> }): StreamFrame {
  return frame(at, 'tool_call', { id: input.id, name: input.name, args: input.args })
}

export function toolResult(at: number, input: { id: string; output: string }): StreamFrame {
  return frame(at, 'tool_result', { id: input.id, status: 'completed', output: input.output })
}

export type CitationSource = { id: string; title: string; url: string; snippet: string }

/** A source card. The films put every source in one group so the list never grows headings. */
export function citation(at: number, source: CitationSource, group: { id: string; label: string }): StreamFrame {
  return frame(at, 'citation', {
    id: source.id,
    title: source.title,
    url: source.url,
    snippet: source.snippet,
    source_group_id: group.id,
    source_group_label: group.label,
    origin: 'knowledge',
  })
}

/**
 * A Resultat document revision. `version` is the revision key: a version sent
 * again replaces that revision's content, so each revision of one document needs
 * its own version to appear in its history (chat-artifacts.ts, mergeArtifactVersion).
 */
export function artifact(
  at: number,
  input: { id: string; title: string; content: string; version: number },
): StreamFrame {
  return frame(at, 'artifact', {
    id: input.id,
    kind: 'markdown',
    title: input.title,
    content: input.content,
    version: input.version,
  })
}

/** One Arbeid row. A status of `awaiting_approval` on the run's own id is what makes the turn read its approvals. */
export function step(
  at: number,
  input: { id: string; title: string; detail: string; status: 'running' | 'done' | 'awaiting_approval' },
): StreamFrame {
  return frame(at, 'step_update', { id: input.id, title: input.title, detail: input.detail, status: input.status })
}

export function done(at: number, input: { requestId: string; model: string }): StreamFrame {
  return frame(at, 'done', { request_id: input.requestId, model_used: input.model, stop_reason: 'end_turn' })
}

export function stopped(at: number, input: { requestId: string; reason: string }): StreamFrame {
  return frame(at, 'stopped', { request_id: input.requestId, reason: input.reason })
}

/** A mid-run message the run has received. The product matches it by its trimmed text. */
export function queuedInput(at: number, message: string): StreamFrame {
  return frame(at, 'queued_input', { messages: [message] })
}

export function subagentAttached(
  at: number,
  input: { runId: string; childRunId: string; role: string; goal: string; batchId: string },
): StreamFrame {
  return frame(at, 'subagent_attached', {
    parent_run_id: input.runId,
    child_run_id: input.childRunId,
    role: input.role,
    goal_summary: input.goal,
    batch_id: input.batchId,
  })
}

export function subagentStopped(at: number, input: { runId: string; childRunId: string }): StreamFrame {
  return frame(at, 'subagent_stopped', {
    parent_run_id: input.runId,
    child_run_id: input.childRunId,
    status: 'completed',
  })
}

/** The run pauses at its gate; the live run panel asks the turn to re-read the approvals. */
export function runPaused(at: number, input: { runId: string; approvalId: string }): StreamFrame {
  return frame(at, 'run_paused_for_approval', { run_id: input.runId, approval_id: input.approvalId })
}

export function runResumed(at: number, input: { runId: string; approvalId: string }): StreamFrame {
  return frame(at, 'run_resumed_after_approval', { run_id: input.runId, approval_id: input.approvalId })
}

export function approvalChanged(
  at: number,
  input: { approvalId: string; runId: string; kind: string; to: string; decidedBy: string },
): StreamFrame {
  return frame(at, 'approval_state_changed', {
    approval_id: input.approvalId,
    run_id: input.runId,
    approval_kind: input.kind,
    to: input.to,
    decided_by: input.decidedBy,
  })
}
