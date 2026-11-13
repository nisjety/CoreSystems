import { z } from 'zod'

// The client action contract for a reader's translation of one shown text (F-t3-098, W5-098-c). The browser checks the
// text and the target here before it sends anything, and the gateway's POST /api/v1/ai/translate reads the same two
// fields (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ai.rs). Model Gateway checks them again and refuses a
// zero-retention turn before any model call. The text is sent once and kept nowhere: the answer is the translation, shown
// on the reader's screen only. It never goes back to the model, a thread or the composer.

/** The most characters one translation takes, once the ends are trimmed. A longer text is refused, not cut. */
export const REASONING_TRANSLATE_MAX_CHARS = 12_000

/** The languages a translation can be read in, as the route names them: `nb` is Norwegian Bokmål, `en` is English. */
export const REASONING_TRANSLATE_LOCALES = ['nb', 'en'] as const

/** One of the route's target languages. */
export type ReasoningTranslateLocale = (typeof REASONING_TRANSLATE_LOCALES)[number]

/** The characters a text counts as: its ends are trimmed first, as Model Gateway trims them. */
export function reasoningTranslateChars(text: string): number {
  return Array.from(text.trim()).length
}

/** The route's target for the app's locale: Norwegian (`no`) is read as Bokmål (`nb`), English stays `en`. */
export function reasoningTranslateLocaleOf(locale: 'no' | 'en'): ReasoningTranslateLocale {
  return locale === 'no' ? 'nb' : 'en'
}

/**
 * The translation request: the text, and the language it is to be read in. Any other field is refused, so a caller cannot
 * name a posture, a model or a switch. The zero-retention flag comes from the request's header and the organisation's
 * posture, never from the body.
 */
export const reasoningTranslateInput = z
  .object({
    text: z
      .string()
      .refine((text) => text.trim().length > 0, 'The text is empty.')
      .refine(
        (text) => reasoningTranslateChars(text) <= REASONING_TRANSLATE_MAX_CHARS,
        'The text is over 12000 characters.',
      ),
    targetLocale: z.enum(REASONING_TRANSLATE_LOCALES),
  })
  .strict()

/** What a caller passes to a translation. */
export type ReasoningTranslateInput = z.input<typeof reasoningTranslateInput>

/** The request as it is sent. */
export type ReasoningTranslateRequest = z.infer<typeof reasoningTranslateInput>

/**
 * A translation request the contract refuses. Nothing is sent when it is thrown, and the error names no part of the
 * request, so it can be logged without the person's text.
 */
export class ReasoningTranslateContractError extends Error {
  readonly code = 'reasoning_translate_contract_refused'

  constructor() {
    super('The reasoning translation request is refused by its action contract.')
    this.name = 'ReasoningTranslateContractError'
  }
}

/** Parses one translation request through the contract and returns the request to send, or throws a typed refusal. */
export function parseReasoningTranslateInput(input: ReasoningTranslateInput): ReasoningTranslateRequest {
  const parsed = reasoningTranslateInput.safeParse(input)
  if (!parsed.success) throw new ReasoningTranslateContractError()
  return parsed.data
}
