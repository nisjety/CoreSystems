import { z } from 'zod'

// The thread and model a side feature names to the gateway (W4-BYO, DEC-V-R8): a review, a reasoning translation and the
// model council. A thread whose turns were written under a customer gateway or a ChatGPT subscription is answered on that
// connection, and the model is the one that connection serves. Both fields are optional, so a request that names neither
// runs on the platform chain as it always did. The BFF and Model Gateway check them again. A malformed name is refused and
// never dropped: a dropped thread would be answered on the platform instead of the connection the thread is on.

/** The most characters a thread id or a model id takes, as the BFF and Model Gateway bound them. */
export const FEATURE_NAME_MAX_CHARS = 200

/** A thread id or a model id is one identifier: no space and no control character. */
function isIdentifier(value: string): boolean {
  return !/[\s\p{Cc}]/u.test(value)
}

function withinFeatureNameLimit(value: string): boolean {
  return Array.from(value).length <= FEATURE_NAME_MAX_CHARS
}

/** The thread a side feature names: a server id, trimmed, at most 200 characters. */
export const featureThreadId = z
  .string()
  .trim()
  .min(1)
  .refine(withinFeatureNameLimit, { message: 'A thread id is at most 200 characters.' })
  .refine(isIdentifier, { message: 'A thread id has no spaces or control characters.' })

/** The model a connection thread's side feature names: one model id, trimmed, at most 200 characters. */
export const featureModelId = z
  .string()
  .trim()
  .min(1)
  .refine(withinFeatureNameLimit, { message: 'A model id is at most 200 characters.' })
  .refine(isIdentifier, { message: 'A model id has no spaces or control characters.' })
