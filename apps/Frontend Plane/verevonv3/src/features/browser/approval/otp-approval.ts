import type { ApprovalDecisionRequest } from './approval-model'

/**
 * Pure view model for the OTP variant of the approval card (E3.1-i).
 *
 * A `fill_otp` approval lets the agent type a one-time code, read from a mail
 * connection, into one field on one page. Before the person decides, the card
 * names three things: the origin of the page, the origin of the frame that holds
 * the field, and the mail connection the code comes from (its provider and the
 * label the person gave it, never the mailbox address). The approval covers one
 * fill only.
 *
 * The code is not part of this model, so the card has no field that could show
 * it. Approve is withheld whenever the executor would refuse the fill (not https,
 * a frame on another origin, an origin or a connection that cannot be named), so
 * the person is never asked to approve a fill that cannot run. Reject is always
 * offered.
 *
 * Nothing here touches the DOM or the network. The preconditions mirror the E3.1-c
 * executor row in the finish queue and memo W3-E3.1-f section 4.
 */

/**
 * What the approval record states about one `fill_otp` approval. The field names
 * are the ask in MP-ASKS.md (W3-E3.1-i). Until the record carries them the caller
 * cannot build this card, and the approval stays on the generic card.
 */
export type OtpApprovalSource = {
  approvalId: string
  /** The page the code goes to: a URL or an origin. Only the origin is kept. */
  topOrigin: string
  /** The frame that holds the field: a URL or an origin. It must equal `topOrigin` to fill. */
  frameOrigin: string
  /** The mail provider key integration-core names, such as `google` or `microsoft`. */
  mailProviderKey: string
  /** The label the person gave the mail connection. Never the mailbox address. */
  mailAccountLabel: string
}

/** A mail provider the card can name. Any other connection withholds Approve. */
export type OtpMailProvider = 'gmail' | 'outlook'

/** Why Approve is withheld. Reject stays available for every reason. */
export type OtpApprovalBlock =
  | 'origin_unknown'
  | 'not_https'
  | 'frame_unknown'
  | 'frame_mismatch'
  | 'connection_unknown'

export type OtpApprovalCardModel = {
  approvalId: string
  /** `https://host[:port]` of the page, or '' when it cannot be read. */
  topOrigin: string
  /** `https://host[:port]` of the frame that holds the field, or '' when it cannot be read. */
  frameOrigin: string
  /** The connection's provider, or null when the card has no name for it. */
  mailProvider: OtpMailProvider | null
  /** The label the person gave the connection, or '' when none. Never an address. */
  mailAccountLabel: string
  /** null when Approve is offered; otherwise the reason it is withheld. */
  approveBlock: OtpApprovalBlock | null
}

/** The longest account label the card shows, in characters. */
export const OTP_ACCOUNT_LABEL_MAX = 64

/** The name the card shows for each provider. Brand names read the same in both languages. */
export const OTP_MAIL_PROVIDER_NAME: Readonly<Record<OtpMailProvider, string>> = {
  gmail: 'Gmail',
  outlook: 'Outlook',
}

const MAIL_PROVIDER_BY_KEY: Readonly<Record<string, OtpMailProvider>> = {
  google: 'gmail',
  gmail: 'gmail',
  microsoft: 'outlook',
  outlook: 'outlook',
}

/** The `actionType` values a one-time-code fill can arrive with: the sensitive action and the proposal's wire tag. */
const OTP_FILL_ACTION_TYPES: ReadonlySet<string> = new Set(['fill_otp', 'fill_otp_ref'])

/** Norwegian and English copy for each reason Approve is withheld. */
export const OTP_BLOCK_COPY: Readonly<Record<OtpApprovalBlock, { no: string; en: string }>> = {
  origin_unknown: {
    no: 'Nettstedet kan ikke fastslås. Godkjenning er ikke tilgjengelig.',
    en: 'The site cannot be identified. Approval is not available.',
  },
  not_https: {
    no: 'Koden kan bare fylles inn på en https-side. Godkjenning er ikke tilgjengelig.',
    en: 'A code can only be filled on an https page. Approval is not available.',
  },
  frame_unknown: {
    no: 'Nettstedet for feltet kan ikke fastslås. Godkjenning er ikke tilgjengelig.',
    en: 'The site of the field cannot be identified. Approval is not available.',
  },
  frame_mismatch: {
    no: 'Feltet ligger på et annet nettsted enn siden. Godkjenning er ikke tilgjengelig.',
    en: 'The field is on a different site from the page. Approval is not available.',
  },
  connection_unknown: {
    no: 'E-postkoblingen kan ikke fastslås. Godkjenning er ikke tilgjengelig.',
    en: 'The mail connection cannot be identified. Approval is not available.',
  },
}

/** True when a browser approval is a one-time-code fill, so this card is the one for it. */
export function isOtpFillApproval(actionType: string): boolean {
  return OTP_FILL_ACTION_TYPES.has(actionType.trim())
}

/**
 * The origin of a URL or an origin as `scheme://host[:port]`, or '' when it is not
 * a web address. Path, query, fragment and user info are dropped, so a token in a
 * query string never reaches the card.
 */
export function otpOriginOf(raw: string): string {
  const text = raw.trim()
  if (!text) return ''
  try {
    const parsed = new URL(text)
    if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') return ''
    return parsed.origin
  } catch {
    return ''
  }
}

/** The provider a connection's key names, or null when the card has no name for it. */
export function otpMailProviderOf(providerKey: string): OtpMailProvider | null {
  const key = providerKey.trim().toLowerCase()
  if (!Object.hasOwn(MAIL_PROVIDER_BY_KEY, key)) return null
  return MAIL_PROVIDER_BY_KEY[key] ?? null
}

/**
 * The label the person gave the connection, trimmed and capped. A label that looks
 * like an address (it holds an @) is dropped, so the card never shows a mailbox.
 */
export function otpAccountLabel(raw: string): string {
  const text = raw.replace(/\s+/g, ' ').trim()
  if (!text || text.includes('@')) return ''
  return text.slice(0, OTP_ACCOUNT_LABEL_MAX).trim()
}

/** The reason Approve is withheld, or null when this fill may be approved. */
export function otpApproveBlock(
  topOrigin: string,
  frameOrigin: string,
  mailProvider: OtpMailProvider | null,
): OtpApprovalBlock | null {
  if (!topOrigin) return 'origin_unknown'
  if (!topOrigin.startsWith('https://')) return 'not_https'
  if (!frameOrigin) return 'frame_unknown'
  if (frameOrigin !== topOrigin) return 'frame_mismatch'
  if (!mailProvider) return 'connection_unknown'
  return null
}

/**
 * The card model for one `fill_otp` approval. Null only when the approval has no
 * id to decide. Everything else is shown, so the person can always reject.
 */
export function otpApprovalToCardModel(source: OtpApprovalSource): OtpApprovalCardModel | null {
  const approvalId = source.approvalId.trim()
  if (!approvalId) return null
  const topOrigin = otpOriginOf(source.topOrigin)
  const frameOrigin = otpOriginOf(source.frameOrigin)
  const mailProvider = otpMailProviderOf(source.mailProviderKey)
  return {
    approvalId,
    topOrigin,
    frameOrigin,
    mailProvider,
    mailAccountLabel: otpAccountLabel(source.mailAccountLabel),
    approveBlock: otpApproveBlock(topOrigin, frameOrigin, mailProvider),
  }
}

export type OtpDecisionResult =
  | { ok: true; request: ApprovalDecisionRequest }
  | { ok: false; reason: 'blocked' }

/**
 * The decision request for one card action. Approve covers this one fill and is
 * refused while the card withholds it. Reject carries no reason.
 */
export function buildOtpDecisionRequest(
  model: OtpApprovalCardModel,
  action: 'approve' | 'reject',
): OtpDecisionResult {
  if (action === 'approve' && model.approveBlock !== null) return { ok: false, reason: 'blocked' }
  return { ok: true, request: { approvalId: model.approvalId, decision: action, reason: '' } }
}

/**
 * Build flag for the OTP variant of the approval card. Off unless
 * `VITE_BROWSER_OTP_APPROVAL_CARD=true`. The caller checks it before it mounts the
 * OTP card; off, it mounts the generic card as it does today.
 */
export function browserOtpApprovalEnabled(): boolean {
  return (import.meta.env as Record<string, string | undefined>).VITE_BROWSER_OTP_APPROVAL_CARD === 'true'
}
