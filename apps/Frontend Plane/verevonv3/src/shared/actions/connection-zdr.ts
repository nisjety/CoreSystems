import { z } from 'zod'

// The client action contracts for a model connection's zero data retention (ZDR) acknowledgement (ZDR Z-19d). A
// connection that cannot give ZDR (today the ChatGPT subscription) is used only after its owner has acknowledged,
// once, the exact assessment and wording they were shown. Integration Core owns the wording, the digest and the
// ledger; the browser reads the wording from the gateway's ZDR routes (ZDR-Z-19c) and never composes it. The three
// contracts here parse what the browser sends before it sends anything. Nothing is stored in the browser.
//
// The two writes are never model-executable: a model must not accept a waiver on a person's behalf, and the
// acknowledgement is the person's own act on the text they were shown. Their rows in model-access.ts are closed.

/** The longest connection id, in characters, as the gateway and Integration Core take it. */
export const CONNECTION_ZDR_CONNECTION_ID_MAX_CHARS = 128

/** The locales the wording is composed in. `no` is Norwegian Bokmal. */
export const CONNECTION_ZDR_LOCALES = ['no', 'en'] as const

/** The wording variants a person can be shown. Only `org_zdr_on` and `first_use` can be acknowledged. */
export const CONNECTION_ZDR_VARIANTS = ['org_zdr_on', 'org_zdr_off', 'first_use'] as const

/** The wording variants an acknowledgement is recorded against: the text the person was actually shown. */
export const CONNECTION_ZDR_ACKNOWLEDGED_VARIANTS = ['org_zdr_on', 'first_use'] as const

/** Where the person acknowledged: while connecting, on first use of the connection, or from settings. */
export const CONNECTION_ZDR_ACKNOWLEDGED_VIA = ['connect', 'first_use', 'settings'] as const

/** The longest text version or digest the gateway forwards. A digest is 64 hex characters; a version is `v<digits>`. */
export const CONNECTION_ZDR_FIELD_MAX_CHARS = 256

export type ConnectionZdrLocale = (typeof CONNECTION_ZDR_LOCALES)[number]
export type ConnectionZdrVariant = (typeof CONNECTION_ZDR_VARIANTS)[number]
export type ConnectionZdrAcknowledgedVia = (typeof CONNECTION_ZDR_ACKNOWLEDGED_VIA)[number]

/** Whether a string is a connection id the gateway puts in a path: 1 to 128 characters, no `/`, no whitespace. */
export function isConnectionZdrConnectionId(id: string): boolean {
  return id.length > 0 && id.length <= CONNECTION_ZDR_CONNECTION_ID_MAX_CHARS && !/[\s/]/.test(id)
}

const connectionId = z
  .string()
  .refine(isConnectionZdrConnectionId, 'A connection id is 1 to 128 characters with no slash and no whitespace.')

// An opaque digest the assessment carried: 64 lowercase hex characters, compared by Integration Core, never by the browser.
const assessmentDigest = z.string().regex(/^[0-9a-f]{64}$/, 'A digest is 64 lowercase hex characters.')

// The wording version the person was shown, such as `v1`. Integration Core checks it against the current wording.
const textVersion = z.string().min(1).max(CONNECTION_ZDR_FIELD_MAX_CHARS)

/**
 * `connection_zdr.read`: the connection's assessment, the acknowledgement in force and the wording for a locale and
 * variant. Read only. Any other field is refused.
 */
export const connectionZdrReadInput = z
  .object({
    connectionId,
    locale: z.enum(CONNECTION_ZDR_LOCALES).default('no'),
    variant: z.enum(CONNECTION_ZDR_VARIANTS).default('org_zdr_on'),
  })
  .strict()

/**
 * `connection_zdr.acknowledge`: the owner acknowledges the assessment and wording they were shown. The digest and
 * text version come from the read, unchanged; Integration Core refuses a stale pair with `zdr_assessment_changed`. Any
 * other field is refused.
 */
export const connectionZdrAcknowledgeInput = z
  .object({
    connectionId,
    digest: assessmentDigest,
    textVersion,
    locale: z.enum(CONNECTION_ZDR_LOCALES),
    variant: z.enum(CONNECTION_ZDR_ACKNOWLEDGED_VARIANTS),
    acknowledgedVia: z.enum(CONNECTION_ZDR_ACKNOWLEDGED_VIA),
  })
  .strict()

/** `connection_zdr.revoke`: the owner ends their acknowledgement of the connection. Idempotent. Any other field is refused. */
export const connectionZdrRevokeInput = z.object({ connectionId }).strict()

const connectionZdrAssessmentCapability = z.enum(['supported', 'not_supported', 'cannot_verify'])

// `superseded` is an active row below the minimum text version: Integration Core names it, and it is not in force.
const connectionZdrAcknowledgementSummary = z.object({
  id: z.string(),
  state: z.enum(['active', 'superseded', 'revoked']),
  digest: z.string(),
  textVersion: z.string(),
  locale: z.string(),
  acknowledgedVia: z.string(),
  acknowledgedAt: z.string(),
})

// The wording is Integration Core's own. The output names the fields the browser reads and passes any other through.
const connectionZdrText = z.object({
  key: z.string(),
  variant: z.string(),
  locale: z.string(),
  title: z.string(),
  body: z.string(),
  continueLabel: z.string(),
  cancelLabel: z.string(),
  sha256: z.string(),
})

/** What the read answers. A connection with no record is unassessed: `assessed` false, no acknowledgement and no text. */
export const connectionZdrReadOutput = z
  .object({
    connectionId: z.string(),
    providerKey: z.string(),
    assessed: z.boolean(),
    capability: connectionZdrAssessmentCapability,
    basis: z.string(),
    digest: z.string(),
    catalogVersion: z.number().optional(),
    requiresAcknowledgement: z.boolean(),
    wordingVersion: z.string(),
    wordingStatus: z.string(),
    acknowledgement: connectionZdrAcknowledgementSummary.nullable(),
    text: connectionZdrText.nullable(),
  })
  .passthrough()

/** What the acknowledge answers: whether it created the record (201) or replayed it (200), and the record. */
export const connectionZdrAcknowledgeOutput = z
  .object({
    created: z.boolean(),
    acknowledgement: connectionZdrAcknowledgementSummary,
  })
  .passthrough()

/** What the revoke answers: how many acknowledgements it ended. A second revoke answers 0. */
export const connectionZdrRevokeOutput = z
  .object({
    connectionId: z.string(),
    revoked: z.number().int().nonnegative(),
  })
  .passthrough()

export type ConnectionZdrReadInput = z.input<typeof connectionZdrReadInput>
export type ConnectionZdrAcknowledgeInput = z.input<typeof connectionZdrAcknowledgeInput>
export type ConnectionZdrRevokeInput = z.input<typeof connectionZdrRevokeInput>
