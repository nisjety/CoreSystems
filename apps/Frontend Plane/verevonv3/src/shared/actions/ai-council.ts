import { z } from 'zod'

// The client action contract for the model council (F-t3-092): two to four catalogue models answer one question with no
// tools, then one synthesis reads where they agree and disagree. The browser parses the request here before it sends
// anything, and the gateway's POST /api/v1/ai/council reads the same fields (apps/gateway/src/domains/ai.rs). Model
// Gateway checks the question, the models and the limits again. Nothing is stored: the answers go back to the caller
// only, and the cost is estimated before any model is asked.

/** The most characters one question takes, as Model Gateway counts them. A longer one is refused, not cut. */
export const COUNCIL_QUESTION_MAX_CHARS = 4_000

/** A council asks at least this many models and at most this many. The cap bounds what one question can cost. */
export const COUNCIL_MIN_MODELS = 2
export const COUNCIL_MAX_MODELS = 4

/** The longest model id, in bytes. Model ids are ASCII. */
const COUNCIL_MODEL_ID_MAX_BYTES = 128

/**
 * The providers Model Gateway prices as a subscription route (`SUBSCRIPTION_PROVIDERS` in its pricing module). A model
 * served through one is billed to the person's own plan, so the council asks it only when the person allows it.
 */
const COUNCIL_SUBSCRIPTION_PROVIDERS: readonly string[] = [
  'openai-codex-subscription',
  'codex-subscription',
  'chatgpt-codex',
]

/** Whether a catalogue provider is a subscription route, which the council runs only with an explicit allowance. */
export function isCouncilSubscriptionProvider(provider: string | undefined): boolean {
  return COUNCIL_SUBSCRIPTION_PROVIDERS.includes((provider ?? '').trim().toLowerCase())
}

/** Whether a string is a model id the council takes: ASCII letters, digits and `-_.:/`, at most 128 bytes. */
export function isCouncilModelId(id: string): boolean {
  return id.length <= COUNCIL_MODEL_ID_MAX_BYTES && /^[A-Za-z0-9_.:/-]+$/.test(id)
}

// The question is the person's own text: it is trimmed, and a blank one is refused. Its limit counts characters, as
// Model Gateway does.
const councilQuestion = z
  .string()
  .transform((text) => text.trim())
  .refine((text) => text.length > 0, 'The question is empty.')
  .refine(
    (text) => Array.from(text).length <= COUNCIL_QUESTION_MAX_CHARS,
    'The question is over 4000 characters.',
  )

const councilModelId = z
  .string()
  .trim()
  .refine(isCouncilModelId, 'A model id is blank, longer than 128 bytes, or has a character a model id does not.')

// Each model is named once: the council keys its members, and its synthesis names them, by id.
const councilModels = z
  .array(councilModelId)
  .min(COUNCIL_MIN_MODELS, 'Name at least two models.')
  .max(COUNCIL_MAX_MODELS, 'Name at most four models.')
  .refine((ids) => new Set(ids).size === ids.length, 'A model is named more than once.')

/**
 * `council.run`: one question, the models that answer it, the synthesizer if one is named, and the allowance for
 * subscription models. The same fields serve the estimate, which differs only in not asking any model. Any other field
 * is refused.
 */
export const councilRunInput = z
  .object({
    question: councilQuestion,
    models: councilModels,
    synthesizerModel: councilModelId.optional(),
    allowSubscription: z.boolean().default(false),
  })
  .strict()

/** What a caller passes to the council. The subscription allowance may be left out. */
export type CouncilRunInput = z.input<typeof councilRunInput>

/** The request as it is sent: the question trimmed, and every field present with its default filled in. */
export type CouncilRunRequest = z.infer<typeof councilRunInput>

/** Whether the contract takes a council request as it stands. Nothing is sent when it does not, and no part of it is kept. */
export function councilInputAccepted(input: CouncilRunInput): boolean {
  return councilRunInput.safeParse(input).success
}

/**
 * A council request the contract refuses. Nothing is sent when it is thrown, and the error names no part of the request,
 * so it can be logged without the person's text.
 */
export class CouncilContractError extends Error {
  readonly code = 'council_contract_refused'

  constructor() {
    super('The council request is refused by its action contract.')
    this.name = 'CouncilContractError'
  }
}

/** Parses one council request through the contract and returns the request to send, or throws a typed refusal. */
export function parseCouncilInput(input: CouncilRunInput): CouncilRunRequest {
  const parsed = councilRunInput.safeParse(input)
  if (!parsed.success) throw new CouncilContractError()
  return parsed.data
}
