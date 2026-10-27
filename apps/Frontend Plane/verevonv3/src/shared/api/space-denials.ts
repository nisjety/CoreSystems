// Norwegian and English words for every typed Space denial.
//
// When Control refuses a Space request it now says why with one stable code (the
// reason), who can fix it (`resolvable_by`) and a few allowlisted details. Those
// are the golden `./contracts/space-denial-codes.json`: a byte-identical copy of
// apps/Control Plane/user-core/internal/spaces/testdata/space-denial-codes.golden.json,
// pinned by sha256 beside it. The gateway carries the reason in
// `error.details.reason` (a body that also names the operation, such as
// `space_thread_not_authorized`, in `error.code`), and on an `unavailable[]` gap
// in `details`. This file turns that into a sentence and a next step.
//
// The server's own text is never shown: a reason this SPA does not know (a newer
// server) gets the generic line, and so does anything malformed. The test beside
// this file fails if a golden code has no Norwegian or English copy.

export type SpaceDenialReason =
  | 'space_membership_required'
  | 'space_not_active'
  | 'space_kind_unsupported'
  | 'space_policy_not_provisioned'
  | 'space_entitlement_off'
  | 'space_role_below_floor'
  | 'space_zdr_forbids_effect'
  | 'space_audience_not_current'
  | 'space_authority_incomplete'
  | 'space_authority_unavailable'

/** Who can resolve the denial, as the golden names it. */
export type SpaceDenialResolver = 'none' | 'org_admin' | 'space_manager' | 'retry' | 'operator'

/** The reader's-language chooser, the same shape the rest of the SPA passes around. */
export type Translate = (noText: string, enText: string) => string

/** The sha256 of `./contracts/space-denial-codes.json`. Changing the golden changes this too. */
export const SPACE_DENIAL_CODES_CONTRACT_SHA256 =
  '8d1f1b0ab93bfbde4cc60d2cd560fc5df03d85b7bf7a9e318c3414d9533883fe'

/** Where an organisation administrator changes what Spaces may do. */
export const SPACE_POLICY_SETTINGS_PATH = '/settings/space-policy'

export type SpaceDenialText = {
  readonly no: string
  readonly en: string
  /** Who the golden says can fix it; used when the server's own value is missing or unknown. */
  readonly resolvableBy: SpaceDenialResolver
  readonly retryable: boolean
}

export const SPACE_DENIAL_TEXT: Record<SpaceDenialReason, SpaceDenialText> = {
  space_membership_required: {
    no: 'Du har ikke tilgang til dette rommet.',
    en: 'You do not have access to this Space.',
    resolvableBy: 'none',
    retryable: false,
  },
  space_not_active: {
    no: 'Rommet er ikke aktivt akkurat nå, så dette kan ikke gjøres.',
    en: 'This Space is not active right now, so this cannot be done.',
    resolvableBy: 'space_manager',
    retryable: false,
  },
  space_kind_unsupported: {
    no: 'Dette er ikke tilgjengelig for denne typen rom.',
    en: 'This is not available for this kind of Space.',
    resolvableBy: 'none',
    retryable: false,
  },
  space_policy_not_provisioned: {
    no: 'Organisasjonen din har ikke satt opp rom ennå, så dette er avslått.',
    en: 'Your organisation has not set up Spaces yet, so this is declined.',
    resolvableBy: 'org_admin',
    retryable: false,
  },
  space_entitlement_off: {
    no: 'Organisasjonen din har slått av dette for rom.',
    en: 'Your organisation has turned this off for Spaces.',
    resolvableBy: 'org_admin',
    retryable: false,
  },
  space_role_below_floor: {
    no: 'Rollen din i dette rommet er ikke høy nok til dette.',
    en: 'Your role in this Space is not high enough for this.',
    resolvableBy: 'space_manager',
    retryable: false,
  },
  space_zdr_forbids_effect: {
    no: 'Zero Data Retention i organisasjonen din tillater ikke dette i rom.',
    en: 'Your organisation’s Zero Data Retention does not allow this in Spaces.',
    resolvableBy: 'none',
    retryable: false,
  },
  space_audience_not_current: {
    no: 'Tilgangen din til rommet er under oppdatering. Prøv igjen om litt.',
    en: 'Your access to this Space is being updated. Try again shortly.',
    resolvableBy: 'retry',
    retryable: true,
  },
  space_authority_incomplete: {
    no: 'Rommets tilgangsoppsett er ikke komplett, så dette er avslått.',
    en: 'This Space’s access setup is incomplete, so this is declined.',
    resolvableBy: 'operator',
    retryable: false,
  },
  space_authority_unavailable: {
    no: 'Tilgangen din til rommet kan ikke bekreftes akkurat nå. Prøv igjen om litt.',
    en: 'Your access to this Space cannot be confirmed right now. Try again shortly.',
    resolvableBy: 'retry',
    retryable: true,
  },
}

/** Said when a denial names a reason this build does not know, or nothing usable. */
const GENERIC_DENIAL = {
  no: 'Dette er ikke tillatt i rommet akkurat nå.',
  en: 'This is not allowed in this Space right now.',
}

/** The entitlement keys of the golden vocabulary, by the name an administrator sees. */
const ENTITLEMENT_LABEL: Record<string, { no: string; en: string }> = {
  thread_create: { no: 'Skrive i rom og starte samtaler', en: 'Write in rooms and start conversations' },
  thread_read: { no: 'Lese hele romsamtalen', en: 'Read the whole room conversation' },
  retrieval_read: { no: 'Bruke kunnskap i rom', en: 'Use knowledge in rooms' },
  import_write: { no: 'Importere filer og kilder', en: 'Import files and sources' },
  agent_action: { no: 'La agenter utføre handlinger', en: 'Let agents perform actions' },
  schedule_fire: { no: 'Planlagte kjøringer og overvåking', en: 'Scheduled runs and watches' },
  sandbox_capability: { no: 'Agent-datamaskin (sandkasse)', en: 'Agent computer (sandbox)' },
  process_registry: { no: 'Bakgrunnsprosesser', en: 'Background processes' },
}

const RESOLVERS: readonly SpaceDenialResolver[] = ['none', 'org_admin', 'space_manager', 'retry', 'operator']

/** The only shape a reason or detail value may have; anything else is dropped. */
const TOKEN = /^[a-z0-9_-]{1,64}$/

export type SpaceDenialInfo = {
  /** The reason code as the server sent it; a key of {@link SPACE_DENIAL_TEXT} when `known`. */
  readonly reason: string
  readonly known: boolean
  readonly resolvableBy: SpaceDenialResolver
  readonly entitlement?: string
  readonly state?: string
  readonly kind?: string
  readonly effect?: string
}

/** What a reader can do next. `href` is set only for a link. */
export type SpaceDenialCta = {
  readonly action: 'open_space_policy' | 'ask_org_admin' | 'ask_space_manager' | 'retry' | 'contact_support'
  readonly label: string
  readonly href?: string
}

export type SpaceDenialPresentation = {
  readonly message: string
  readonly cta: SpaceDenialCta | null
}

export function isSpaceDenialReason(value: unknown): value is SpaceDenialReason {
  return typeof value === 'string' && Object.prototype.hasOwnProperty.call(SPACE_DENIAL_TEXT, value)
}

function token(value: unknown): string | undefined {
  return typeof value === 'string' && TOKEN.test(value) ? value : undefined
}

/**
 * The typed denial a thrown `ApiError`, a chat error event or an `unavailable[]`
 * gap carries, or null when it carries none. Looks at `details.reason` first and
 * at `code` second (a pass-through body keeps Control's own code), and keeps only
 * the allowlisted detail keys as short tokens.
 */
export function readSpaceDenial(source: unknown): SpaceDenialInfo | null {
  if (!source || typeof source !== 'object') return null
  const record = source as { code?: unknown; details?: unknown }
  const details = record.details && typeof record.details === 'object' && !Array.isArray(record.details)
    ? (record.details as Record<string, unknown>)
    : undefined
  const reason = token(details?.reason) ?? (isSpaceDenialReason(record.code) ? record.code : undefined)
  if (!reason) return null
  const known = isSpaceDenialReason(reason)
  const sent = RESOLVERS.find((resolver) => resolver === details?.resolvable_by)
  const entitlement = token(details?.entitlement)
  const state = token(details?.state)
  const kind = token(details?.kind)
  const effect = token(details?.effect)
  return {
    reason,
    known,
    resolvableBy: sent ?? (known ? SPACE_DENIAL_TEXT[reason].resolvableBy : 'none'),
    ...(entitlement ? { entitlement } : {}),
    ...(state ? { state } : {}),
    ...(kind ? { kind } : {}),
    ...(effect ? { effect } : {}),
  }
}

/** True when `source` carries a typed Space denial, so a caller defers to the presenter. */
export function isSpaceDenial(source: unknown): boolean {
  return readSpaceDenial(source) !== null
}

function messageFor(info: SpaceDenialInfo, tr: Translate): string {
  if (!info.known) return tr(GENERIC_DENIAL.no, GENERIC_DENIAL.en)
  const copy = SPACE_DENIAL_TEXT[info.reason as SpaceDenialReason]
  if (info.reason === 'space_entitlement_off' && info.entitlement) {
    const label = ENTITLEMENT_LABEL[info.entitlement]
    if (label) {
      return tr(
        `Organisasjonen din har slått av «${label.no}» for rom.`,
        `Your organisation has turned off “${label.en}” for Spaces.`,
      )
    }
  }
  return tr(copy.no, copy.en)
}

function ctaFor(info: SpaceDenialInfo, tr: Translate, isAdmin: boolean): SpaceDenialCta | null {
  switch (info.resolvableBy) {
    case 'org_admin':
      return isAdmin
        ? {
            action: 'open_space_policy',
            label: tr('Åpne romregler', 'Open Space policy'),
            href: SPACE_POLICY_SETTINGS_PATH,
          }
        : {
            action: 'ask_org_admin',
            label: tr('Be en administrator i organisasjonen om å endre dette.', 'Ask an administrator in your organisation to change this.'),
          }
    case 'space_manager':
      return { action: 'ask_space_manager', label: tr('Be en romleder om hjelp.', 'Ask a room manager for help.') }
    case 'retry':
      return { action: 'retry', label: tr('Prøv igjen.', 'Try again.') }
    case 'operator':
      return { action: 'contact_support', label: tr('Kontakt support.', 'Contact support.') }
    default:
      return null
  }
}

/**
 * The sentence and the next step for a typed Space denial. Anything that is not
 * one (or names a reason this build does not know) gets the generic line, never
 * the server's own text. `isAdmin` is a UI hint only: it decides whether the next
 * step is a link to the policy console or a request to an administrator; the
 * gateway and Control decide what an administrator may actually do.
 */
export function presentSpaceDenial(
  source: unknown,
  tr: Translate,
  options: { isAdmin?: boolean } = {},
): SpaceDenialPresentation {
  const info = readSpaceDenial(source)
  if (!info) return { message: tr(GENERIC_DENIAL.no, GENERIC_DENIAL.en), cta: null }
  return { message: messageFor(info, tr), cta: ctaFor(info, tr, options.isAdmin === true) }
}

/** The presentation as one plain string, for a line that cannot hold a link. */
export function presentSpaceDenialText(
  source: unknown,
  tr: Translate,
  options: { isAdmin?: boolean } = {},
): string {
  const { message, cta } = presentSpaceDenial(source, tr, options)
  return cta ? `${message} ${cta.label}` : message
}
