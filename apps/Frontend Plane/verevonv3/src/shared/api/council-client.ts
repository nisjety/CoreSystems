import { parseCouncilInput, type CouncilRunInput, type CouncilRunRequest } from '@/shared/actions/ai-council'
import { ApiError } from './api-error'
import { requestJson } from './http'

// The model council (F-t3-092) through the same-origin gateway: POST /api/v1/ai/council, once for the estimate and once
// for the run. Each call checks the request against the action contract first, so a refused request sends nothing.

/** One call the run makes, as the estimate prices it: its tokens, and its cost when the catalogue prices the model. */
export type CouncilEstimateCall = {
  model: string
  provider: string
  estimatedInputTokens: number
  maxOutputTokens: number
  /** Null when the catalogue gives the model no price, and under zero retention, which is not priced. */
  estimatedCostUsd: number | null
}

/** A subscription model the request left out, and why. */
export type CouncilDroppedModel = { model: string; reason: string }

/** The list-price estimate of one run, made before any model is asked. Its total is null when a part is not priced. */
export type CouncilEstimate = {
  zdr: boolean
  members: CouncilEstimateCall[]
  synthesis: CouncilEstimateCall
  dropped: CouncilDroppedModel[]
  totalEstimatedCostUsd: number | null
}

/** The tokens one billed call used, and its cost. The cost is null under zero retention and where no price is given. */
export type CouncilUsage = {
  inputTokens: number
  outputTokens: number
  costUsd: number | null
}

/**
 * `answered` when the model gave an answer; `refused` when the gateway refused the call, with its code; `error` for any
 * other failure, and for an empty answer.
 */
export type CouncilMemberStatus = 'answered' | 'refused' | 'error'

/** One member's outcome. A refused or failed member has no answer, and the rest of the run is unaffected. */
export type CouncilMember = {
  model: string
  provider: string
  status: CouncilMemberStatus
  answer: string | null
  code: string | null
  usage: CouncilUsage | null
}

/**
 * `structured` when the synthesis reply read strictly; `unstructured` when it did not, and the raw text is kept;
 * `failed` when the synthesis call failed; `not_run` when fewer than two members answered.
 */
export type CouncilSynthesisStatus = 'structured' | 'unstructured' | 'failed' | 'not_run'

/** A claim the answers disagree on, with each model's stance on it, in the order the gateway sent them. */
export type CouncilDisagreement = {
  claim: string
  positions: { model: string; stance: string }[]
}

/** A claim only one model made. */
export type CouncilUniqueClaim = { model: string; claim: string }

/** The synthesis of the answers that came back: where they agree, where they disagree, and what only one model said. */
export type CouncilSynthesis = {
  status: CouncilSynthesisStatus
  model: string | null
  provider: string | null
  agreement: string[]
  disagreement: CouncilDisagreement[]
  unique: CouncilUniqueClaim[]
  /** The raw reply, kept only when it did not read as the structure. */
  text: string | null
  code: string | null
  usage: CouncilUsage | null
}

/** A run: every member's outcome, the synthesis, the models left out, and the run's total cost (null when not priced). */
export type CouncilRun = {
  members: CouncilMember[]
  dropped: CouncilDroppedModel[]
  synthesis: CouncilSynthesis
  totalCostUsd: number | null
}

const MEMBER_STATUSES = ['answered', 'refused', 'error'] as const
const SYNTHESIS_STATUSES = ['structured', 'unstructured', 'failed', 'not_run'] as const

/**
 * Asks the council for the list-price estimate of one run (`estimate_only`). No model is asked, so nothing is spent and
 * nothing is stored. The person sees the cost here before a run is offered. Refusals reject with the server's typed
 * `ApiError`. A request the contract refuses rejects with a `CouncilContractError`, and nothing is sent.
 */
export async function estimateCouncil(input: CouncilRunInput): Promise<CouncilEstimate> {
  const request = parseCouncilInput(input)
  const raw = await requestJson<unknown>('/api/v1/ai/council', {
    method: 'POST',
    body: JSON.stringify(councilWireBody(request, true)),
  })
  return estimateOf(objectValue(raw)?.estimate)
}

/**
 * Runs the council: every model answers the question with no tools, then one synthesis reads the answers. Call it only
 * for an estimate the person has seen. The answers go back to the caller only, and nothing is stored. A member the
 * gateway refuses is reported as it came back, and is never retried on another model. Refusals reject with the server's
 * typed `ApiError`, and a request the contract refuses rejects with a `CouncilContractError`, with nothing sent.
 */
export async function runCouncil(input: CouncilRunInput): Promise<CouncilRun> {
  const request = parseCouncilInput(input)
  const raw = await requestJson<unknown>('/api/v1/ai/council', {
    method: 'POST',
    body: JSON.stringify(councilWireBody(request, false)),
  })
  return runOf(raw)
}

/** The body as the gateway reads it: its own field names, and the synthesizer only when one is named. */
function councilWireBody(request: CouncilRunRequest, estimateOnly: boolean) {
  return {
    question: request.question,
    models: request.models,
    ...(request.synthesizerModel ? { synthesizer_model: request.synthesizerModel } : {}),
    allow_subscription: request.allowSubscription,
    estimate_only: estimateOnly,
  }
}

// Each reader below takes what the gateway sent and keeps only the fields it names, so a reply in a shape this build does
// not know is read as far as it goes, never as a guess.

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function textOf(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

function countOf(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0
}

function moneyOf(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function listOf(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

function oneOf<T extends string>(values: readonly T[], value: unknown): T | undefined {
  return typeof value === 'string' && (values as readonly string[]).includes(value) ? (value as T) : undefined
}

function usageOf(value: unknown): CouncilUsage | null {
  const record = objectValue(value)
  if (!record) return null
  return {
    inputTokens: countOf(record.input_tokens),
    outputTokens: countOf(record.output_tokens),
    costUsd: moneyOf(record.cost_usd),
  }
}

function estimateCallOf(value: unknown): CouncilEstimateCall | null {
  const record = objectValue(value)
  const model = textOf(record?.model)
  if (!record || !model) return null
  return {
    model,
    provider: textOf(record.provider) ?? '',
    estimatedInputTokens: countOf(record.estimated_input_tokens),
    maxOutputTokens: countOf(record.max_output_tokens),
    estimatedCostUsd: moneyOf(record.estimated_cost_usd),
  }
}

function droppedOf(value: unknown): CouncilDroppedModel | null {
  const record = objectValue(value)
  const model = textOf(record?.model)
  return record && model ? { model, reason: textOf(record.reason) ?? '' } : null
}

function memberOf(value: unknown): CouncilMember | null {
  const record = objectValue(value)
  const model = textOf(record?.model)
  const status = oneOf(MEMBER_STATUSES, record?.status)
  if (!record || !model || !status) return null
  return {
    model,
    provider: textOf(record.provider) ?? '',
    status,
    answer: textOf(record.answer) || null,
    code: textOf(record.code) || null,
    usage: usageOf(record.usage),
  }
}

function disagreementOf(value: unknown): CouncilDisagreement | null {
  const record = objectValue(value)
  const claim = textOf(record?.claim)
  if (!record || !claim) return null
  const positions = objectValue(record.positions) ?? {}
  return {
    claim,
    positions: Object.entries(positions).flatMap(([model, stance]) =>
      typeof stance === 'string' ? [{ model, stance }] : [],
    ),
  }
}

function uniqueOf(value: unknown): CouncilUniqueClaim | null {
  const record = objectValue(value)
  const model = textOf(record?.model)
  const claim = textOf(record?.claim)
  return record && model && claim ? { model, claim } : null
}

function synthesisOf(value: unknown): CouncilSynthesis {
  const record = objectValue(value)
  return {
    status: oneOf(SYNTHESIS_STATUSES, record?.status) ?? 'failed',
    model: textOf(record?.model) || null,
    provider: textOf(record?.provider) || null,
    agreement: listOf(record?.agreement).flatMap((item) => {
      const claim = textOf(item)
      return claim ? [claim] : []
    }),
    disagreement: listOf(record?.disagreement).flatMap((item) => disagreementOf(item) ?? []),
    unique: listOf(record?.unique).flatMap((item) => uniqueOf(item) ?? []),
    text: textOf(record?.text) || null,
    code: textOf(record?.code) || null,
    usage: usageOf(record?.usage),
  }
}

/** The estimate's reply, read defensively. A reply with no synthesis call to price is not an estimate, and is refused. */
function estimateOf(value: unknown): CouncilEstimate {
  const record = objectValue(value)
  const synthesis = estimateCallOf(record?.synthesis)
  if (!record || !synthesis) {
    throw new ApiError('The council did not return an estimate.', 502, 'council_no_estimate')
  }
  return {
    zdr: record.zdr === true,
    members: listOf(record.members).flatMap((item) => estimateCallOf(item) ?? []),
    synthesis,
    dropped: listOf(record.dropped).flatMap((item) => droppedOf(item) ?? []),
    totalEstimatedCostUsd: moneyOf(record.total_estimated_cost_usd),
  }
}

/** The run's reply, read defensively. A reply with no list of answers is not a run, and is refused. */
function runOf(value: unknown): CouncilRun {
  const record = objectValue(value)
  if (!record || !Array.isArray(record.members)) {
    throw new ApiError('The council did not return its answers.', 502, 'council_no_answers')
  }
  return {
    members: listOf(record.members).flatMap((item) => memberOf(item) ?? []),
    dropped: listOf(record.dropped).flatMap((item) => droppedOf(item) ?? []),
    synthesis: synthesisOf(record.synthesis),
    totalCostUsd: moneyOf(objectValue(record.usage)?.total_cost_usd),
  }
}
