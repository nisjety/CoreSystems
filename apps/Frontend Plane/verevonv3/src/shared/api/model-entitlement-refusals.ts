// Norwegian and English text for every typed model-entitlement refusal.
//
// An organisation decides which models a member may use. The Model Plane refuses a
// request for a model the caller may not use with one stable code, an action and,
// for `model_not_entitled`, a reason. Those are the golden
// `./contracts/model-entitlement-refusals.json`: a byte-identical copy of
// apps/Model Plane/api/contracts/model-entitlement-refusals.json, pinned by sha256
// beside it. The English text below is that golden's text, verbatim; the Norwegian
// is this SPA's own. The test beside this file fails if either drifts from the golden.
//
// The stream's `error` frame carries the code, not the reason or the model, so the
// chat shows the code's own sentence. The reason and model id (a unary 403 body
// carries both) refine it where a caller has them.

export type ModelEntitlementCode = 'model_not_entitled' | 'model_entitlement_unavailable'

export type ModelEntitlementReason =
  | 'not_in_allow_list'
  | 'explicitly_denied'
  | 'no_entitled_route'
  | 'member_not_found'
  | 'deployment_allowlist'

/** What the reader can do, as the golden names it. */
export type ModelEntitlementAction = 'retry' | 'choose_another_model'

/** The reader's-language chooser, the same shape the rest of the SPA passes around. */
export type Translate = (noText: string, enText: string) => string

type Copy = { readonly no: string; readonly en: string }

/** The sha256 of `./contracts/model-entitlement-refusals.json`. Changing the golden changes this too. */
export const MODEL_ENTITLEMENT_REFUSALS_CONTRACT_SHA256 =
  'd8b48550661ed4daefcff792707c9e1d241b7ddd9ac9f05e8aa64bbcd85a647a'

export type ModelEntitlementRefusalText = Copy & {
  readonly action: ModelEntitlementAction
  readonly retryable: boolean
  readonly takesReason: boolean
}

export type ModelEntitlementReasonText = Copy & {
  /** Whether both sentences hold `{model}`. */
  readonly namesModel: boolean
}

export const REFUSAL_TEXT: Record<ModelEntitlementCode, ModelEntitlementRefusalText> = {
  model_not_entitled: {
    no: 'Denne modellen er ikke tilgjengelig for deg. Forespørselen ble ikke sendt til en modell.',
    en: 'This model is not available to you. The request was not sent to a model.',
    action: 'choose_another_model',
    retryable: false,
    takesReason: true,
  },
  model_entitlement_unavailable: {
    no: 'Det kan ikke bekreftes akkurat nå hvilke modeller du kan bruke. Forespørselen ble ikke sendt til en modell. Prøv igjen om litt.',
    en: 'Which models you may use cannot be confirmed right now. The request was not sent to a model. Try again shortly.',
    action: 'retry',
    retryable: true,
    takesReason: false,
  },
}

export const REASON_TEXT: Record<ModelEntitlementReason, ModelEntitlementReasonText> = {
  not_in_allow_list: {
    no: 'Modellen {model} står ikke på listen over modeller organisasjonen din tillater for deg. Forespørselen ble ikke sendt til en modell.',
    en: 'The model {model} is not on the list your organisation allows for you. The request was not sent to a model.',
    namesModel: true,
  },
  explicitly_denied: {
    no: 'Organisasjonen din har slått av modellen {model}. Forespørselen ble ikke sendt til en modell.',
    en: 'Your organisation has turned off the model {model}. The request was not sent to a model.',
    namesModel: true,
  },
  no_entitled_route: {
    no: 'Ingen modell organisasjonen din tillater kan besvare denne forespørselen. Forespørselen ble ikke sendt til en modell.',
    en: 'No model your organisation allows can serve this request. The request was not sent to a model.',
    namesModel: false,
  },
  member_not_found: {
    no: 'Du er ikke medlem av denne organisasjonen, så ingen modell er tilgjengelig for deg. Forespørselen ble ikke sendt til en modell.',
    en: 'You are not a member of this organisation, so no model is available to you. The request was not sent to a model.',
    namesModel: false,
  },
  deployment_allowlist: {
    no: 'Modellen {model} er ikke slått på i denne installasjonen. Forespørselen ble ikke sendt til en modell.',
    en: 'The model {model} is not enabled on this deployment. The request was not sent to a model.',
    namesModel: true,
  },
}

/** The next step for an action that needs words. Retry needs none: the button says it. */
const NEXT_STEP: Partial<Record<ModelEntitlementAction, Copy>> = {
  choose_another_model: { no: 'Velg en annen modell og send på nytt.', en: 'Choose another model and send again.' },
}

/** A code of the golden, which this SPA renders by its own text. */
export function isModelEntitlementCode(code: unknown): code is ModelEntitlementCode {
  return typeof code === 'string' && Object.hasOwn(REFUSAL_TEXT, code)
}

function isReason(reason: unknown): reason is ModelEntitlementReason {
  return typeof reason === 'string' && Object.hasOwn(REASON_TEXT, reason)
}

export type ModelEntitlementDetails = {
  /** The reason of a `model_not_entitled` body. */
  readonly reason?: string | null
  /** The model id of that body. It is shown only when it is a plain id. */
  readonly model?: string | null
}

/** A model id as the Model Plane sends it: letters, digits and `. _ : / @ + -`, at most 128 bytes. */
const MODEL_ID = /^[A-Za-z0-9._:/@+-]{1,128}$/

export type ModelEntitlementPresentation = {
  /** The code as received. */
  readonly code: ModelEntitlementCode
  /** What happened, in the reader's language. */
  readonly message: string
  /** The next step, in the reader's language, or null when the action needs no words. */
  readonly next: string | null
  readonly action: ModelEntitlementAction
  readonly retryable: boolean
}

/**
 * The reader's words for a model-entitlement refusal. A reason the SPA knows refines
 * the sentence; a sentence that names the model needs a plain model id, and without
 * one the code's own sentence stands. The server's own text is never shown.
 */
export function modelEntitlementPresentation(
  code: ModelEntitlementCode,
  tr: Translate,
  details: ModelEntitlementDetails = {},
): ModelEntitlementPresentation {
  const entry = REFUSAL_TEXT[code]
  const reason = entry.takesReason && isReason(details.reason) ? REASON_TEXT[details.reason] : undefined
  const model = typeof details.model === 'string' && MODEL_ID.test(details.model) ? details.model : undefined
  const copy: Copy = reason && (!reason.namesModel || model !== undefined) ? reason : entry
  const fill = (text: string) => (model === undefined ? text : text.replace('{model}', () => model))
  const step = NEXT_STEP[entry.action]
  return {
    code,
    message: tr(fill(copy.no), fill(copy.en)),
    next: step ? tr(step.no, step.en) : null,
    action: entry.action,
    retryable: entry.retryable,
  }
}
