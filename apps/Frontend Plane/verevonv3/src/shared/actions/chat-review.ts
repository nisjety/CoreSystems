import { z } from 'zod'

// The client action contract for a review as a run (F-t3-086): one read-only reviewer over one text. The browser parses
// the request here before it sends anything, and the gateway's POST /api/v1/ai/review reads the same fields
// (apps/gateway/src/domains/ai.rs). Model Gateway checks the target and the limits again. The text is sent once and kept
// nowhere: the answer is the review card, and nothing else is stored.

/** The kinds of text a review takes. Model Gateway refuses any other target (`review_target_unknown`). */
export const REVIEW_TARGETS = ['artifact_revision', 'turn_diff', 'attachment'] as const

export type ReviewTarget = (typeof REVIEW_TARGETS)[number]

/** The most UTF-8 bytes of material one review takes. A larger text is refused with 413. */
export const REVIEW_MATERIAL_MAX_BYTES = 60_000

/** The most lines of material one review takes, as Model Gateway counts them. */
export const REVIEW_MATERIAL_MAX_LINES = 2_000

/** The longest label, in characters, once whitespace runs are set to one space. */
export const REVIEW_LABEL_MAX_CHARS = 120

/** The longest brief, in characters, once whitespace runs are set to one space. */
export const REVIEW_BRIEF_MAX_CHARS = 1_000

/** The UTF-8 length of a text in bytes, as the material limit counts it. */
export function reviewMaterialBytes(text: string): number {
  return new TextEncoder().encode(text).length
}

/** The lines Model Gateway counts in a text: a final line break does not start another line. */
function reviewLineCount(text: string): number {
  if (text === '') return 0
  const body = text.endsWith('\n') ? text.slice(0, -1) : text
  return body.split('\n').length
}

/** The characters Model Gateway counts in a label or a brief: the ends are trimmed and each whitespace run is one space. */
function reviewLineChars(text: string): number {
  return Array.from(text.trim().split(/\s+/).filter(Boolean).join(' ')).length
}

// The material is kept byte for byte, since the card hashes it and every quote is checked against its lines, so the
// contract never trims or rewrites it. A label or brief is one line to the gateway, so its limit counts it as one.
const reviewRunFields = {
  target: z.enum(REVIEW_TARGETS),
  label: z
    .string()
    .refine((text) => reviewLineChars(text) <= REVIEW_LABEL_MAX_CHARS, 'The label is over 120 characters.')
    .default(''),
  material: z
    .string()
    .refine((text) => text.trim().length > 0, 'The material is empty.')
    .refine((text) => reviewMaterialBytes(text) <= REVIEW_MATERIAL_MAX_BYTES, 'The material is over 60000 bytes.')
    .refine((text) => reviewLineCount(text) <= REVIEW_MATERIAL_MAX_LINES, 'The material is over 2000 lines.'),
  brief: z
    .string()
    .refine((text) => reviewLineChars(text) <= REVIEW_BRIEF_MAX_CHARS, 'The brief is over 1000 characters.')
    .default(''),
  // The app's two languages. `no` is Norwegian Bokmål; Model Gateway reads any value other than `en` as Bokmål.
  locale: z.enum(['no', 'en']).default('no'),
}

/** `review.run`: the target, the text, and the label, brief and locale that frame it. Any other field is refused. */
export const reviewRunInput = z.object(reviewRunFields).strict()

/** What a caller passes to a review. The label, brief and locale may be left out. */
export type ReviewRunInput = z.input<typeof reviewRunInput>

/** The request as it is sent, with every field present and the defaults filled in. */
export type ReviewRunRequest = z.infer<typeof reviewRunInput>

/**
 * A review request the contract refuses. Nothing is sent when it is thrown, and the error names no part of the request,
 * so it can be logged without the person's text.
 */
export class ReviewRunContractError extends Error {
  readonly code = 'review_run_contract_refused'

  constructor() {
    super('The review.run request is refused by its action contract.')
    this.name = 'ReviewRunContractError'
  }
}

/** Parses one review request through the contract and returns the request to send, or throws a typed refusal. */
export function parseReviewRunInput(input: ReviewRunInput): ReviewRunRequest {
  const parsed = reviewRunInput.safeParse(input)
  if (!parsed.success) throw new ReviewRunContractError()
  return parsed.data
}
