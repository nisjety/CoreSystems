import { requestJson } from './http'
import type {
  ConnectionZdrAcknowledgedVia,
  ConnectionZdrLocale,
  ConnectionZdrVariant,
} from '@/shared/actions/connection-zdr'

// The browser's client for a model connection's zero data retention (ZDR) acknowledgement (ZDR Z-19d). It calls the
// gateway's routes (ZDR-Z-19c), which pass Integration Core's own envelope through with the caller's bearer. Integration
// Core owns the wording, the digest and the ledger; this client only carries them. It composes no wording and stores
// nothing: no acknowledgement and no text is kept in the browser.

/**
 * Whether the acknowledgement UI is on. Only the literal `true` turns it on (`1`, `TRUE` and a blank do not). Read it
 * only through this helper, so the switch has one reader.
 */
export function isZdrWaiverUiEnabled(): boolean {
  return import.meta.env.VITE_ZDR_WAIVER_UI === 'true'
}

/** What the capability contract says about the connection's zero data retention. `cannot_verify` is also an unassessed one. */
export type ConnectionZdrCapability = 'supported' | 'not_supported' | 'cannot_verify'

/** The acknowledgement in force for a connection, as Integration Core summarises it. */
export type ConnectionZdrAcknowledgementSummary = {
  id: string
  state: 'active' | 'revoked'
  digest: string
  textVersion: string
  locale: string
  acknowledgedVia: string
  acknowledgedAt: string
}

/** The wording Integration Core composed for one locale and variant. The browser shows it as it is. */
export type ConnectionZdrText = {
  key: string
  variant: string
  locale: string
  title: string
  body: string
  continueLabel: string
  cancelLabel: string
  sha256: string
}

/**
 * The assessment of one connection, the acknowledgement in force and the wording to show. A connection with no record
 * is unassessed: `assessed` is false, `capability` is `cannot_verify`, and there is no acknowledgement and no text.
 */
export type ConnectionZdrAssessment = {
  connectionId: string
  providerKey: string
  assessed: boolean
  capability: ConnectionZdrCapability
  basis: string
  digest: string
  catalogVersion?: number
  requiresAcknowledgement: boolean
  wordingVersion: string
  wordingStatus: string
  acknowledgement: ConnectionZdrAcknowledgementSummary | null
  text: ConnectionZdrText | null
}

/** What a member sends to acknowledge: the digest and text version from the read, and the text they were shown. */
export type ConnectionZdrAcknowledgeBody = {
  digest: string
  textVersion: string
  locale: ConnectionZdrLocale
  /** The text the member was shown: `org_zdr_on` or `first_use`. */
  variant: Extract<ConnectionZdrVariant, 'org_zdr_on' | 'first_use'>
  acknowledgedVia: ConnectionZdrAcknowledgedVia
}

/** The answer to an acknowledgement: whether it created the record or replayed one already in force. */
export type ConnectionZdrAcknowledgeResult = {
  created: boolean
  acknowledgement: ConnectionZdrAcknowledgementSummary
}

/** The answer to a revoke: how many acknowledgements it ended. Revoking again answers 0. */
export type ConnectionZdrRevokeResult = {
  connectionId: string
  revoked: number
}

/** One row of the organisation's acknowledgement ledger. Ids, states and codes only, never the wording. */
export type OrgZdrAcknowledgementItem = {
  id: string
  connectionId: string
  userId: string
  providerKey: string
  capability: string
  textVersion: string
  locale: string
  acknowledgedVia: string
  acknowledgedAt: string
  revokedAt: string | null
  revokeReason: string | null
  status: 'active' | 'revoked'
}

/** The admin list: the rows, the counts of active and revoked acknowledgements, and whether the rows were cut short. */
export type OrgZdrAcknowledgements = {
  items: OrgZdrAcknowledgementItem[]
  counts: { active: number; revoked: number }
  truncated: boolean
}

/** The most rows the gateway asks Integration Core for in one admin list. */
export const ORG_ZDR_ACKNOWLEDGEMENTS_MAX_LIMIT = 200

const BASE_PATH = '/api/v1/model-connections'

function connectionPath(connectionId: string): string {
  return `${BASE_PATH}/${encodeURIComponent(connectionId)}`
}

/** Reads the connection's assessment, the acknowledgement in force and the wording for a locale and variant. */
export function getConnectionZdr(
  connectionId: string,
  locale: ConnectionZdrLocale = 'no',
  variant: ConnectionZdrVariant = 'org_zdr_on',
): Promise<ConnectionZdrAssessment> {
  const query = new URLSearchParams({ locale, variant })
  return requestJson<ConnectionZdrAssessment>(`${connectionPath(connectionId)}/zdr?${query.toString()}`)
}

/** Records the owner's acknowledgement against the exact digest and text version they were shown. */
export function acknowledgeConnectionZdr(
  connectionId: string,
  body: ConnectionZdrAcknowledgeBody,
): Promise<ConnectionZdrAcknowledgeResult> {
  return requestJson<ConnectionZdrAcknowledgeResult>(`${connectionPath(connectionId)}/zdr-acknowledgement`, {
    method: 'POST',
    body: JSON.stringify({
      digest: body.digest,
      textVersion: body.textVersion,
      locale: body.locale,
      variant: body.variant,
      acknowledgedVia: body.acknowledgedVia,
    }),
  })
}

/** Ends the owner's acknowledgement of the connection. Idempotent. */
export function revokeConnectionZdr(connectionId: string): Promise<ConnectionZdrRevokeResult> {
  return requestJson<ConnectionZdrRevokeResult>(`${connectionPath(connectionId)}/zdr-acknowledgement`, {
    method: 'DELETE',
  })
}

/** Lists the organisation's acknowledgements. Organisation owners and admins only; anyone else is refused. */
export function listOrgZdrAcknowledgements(limit?: number): Promise<OrgZdrAcknowledgements> {
  const query = new URLSearchParams()
  if (limit !== undefined && Number.isInteger(limit) && limit >= 1) {
    query.set('limit', String(Math.min(limit, ORG_ZDR_ACKNOWLEDGEMENTS_MAX_LIMIT)))
  }
  const queryText = query.toString()
  const suffix = queryText ? `?${queryText}` : ''
  return requestJson<OrgZdrAcknowledgements>(`${BASE_PATH}/zdr-acknowledgements${suffix}`)
}
