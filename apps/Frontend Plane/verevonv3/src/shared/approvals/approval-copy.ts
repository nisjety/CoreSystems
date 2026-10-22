/**
 * The words every approval surface uses (build plan WS13 S2).
 *
 * An approval card used to headline the raw `APPROVAL_KIND_*` token, and the browser gate kept
 * its own Bokmal-only risk labels, so the same pause read differently on each surface and in
 * English not at all. The server now says why a pause was raised as a closed cause code; this
 * module holds the one sentence per code, in both languages, and every surface asks it.
 *
 * The code set is closed on purpose and mirrors `apps/Model Plane/api/contracts/approval-causes.json`
 * (the test reads that file, so a code added there fails here until it has words). A code this
 * build does not know reads as `unclassified_gate`, and an approval with no cause at all reads as
 * the generic waiting line, never as a token from the wire.
 *
 * The copy calls `i18n.tr(no, en)` at each site, so the two languages stay side by side.
 */

/** What `useI18n()` hands out; the copy only needs its `tr`. */
export type ApprovalCopyI18n = { tr: (no: string, en: string) => string }

/** Every cause code the server can state, in the order the contract lists them. */
export const APPROVAL_CAUSE_CODES = [
  'owner_effect_reserved',
  'consent_to_disclose',
  'risky_effect_under_ask',
  'provider_write',
  'capability_high_risk',
  'capability_policy_ask',
  'hook_ask',
  'browser_action_risk',
  'auto_posture_provider_write',
  'unclassified_gate',
] as const

export type CauseCode = (typeof APPROVAL_CAUSE_CODES)[number]

/** The allowlisted cause the HTTP approval carries. No argument text, digest or action key. */
export type ApprovalCause = {
  code: CauseCode
  tool?: string
  effectClass?: string
  riskCategory?: string
  ruleId?: string
  scopeOptions: string[]
  stepUpRequired: boolean
}

/** A code from the wire as one of the closed set; anything else is `unclassified_gate`. */
export function causeCodeOf(raw: unknown): CauseCode {
  return APPROVAL_CAUSE_CODES.find((code) => code === raw) ?? 'unclassified_gate'
}

/** The part of an approval the copy reads. A full `Approval` satisfies it. */
export type ApprovalCopySource = {
  cause?: ApprovalCause
  detail?: string
}

// Browser risk categories are a free-form string on the wire (the planner states one, and
// execution-core's backstop classifier derives another), so this map is not total and the label
// below falls back to a generic one.
const BROWSER_RISK_LABELS: Record<string, (i18n: ApprovalCopyI18n) => string> = {
  checkout: (i18n) => i18n.tr('Utsjekk/betaling', 'Checkout or payment'),
  cross_domain_navigation: (i18n) =>
    i18n.tr('Navigering til nytt domene', 'Navigation to a new domain'),
  destructive: (i18n) => i18n.tr('Ødeleggende handling', 'A destructive action'),
  login: (i18n) => i18n.tr('Innlogging', 'Signing in'),
  persistent_cookie_use: (i18n) =>
    i18n.tr('Bruker vedvarende profil/informasjonskapsler', 'Uses a persistent profile or cookies'),
  posting_form: (i18n) => i18n.tr('Skjemainnsending', 'Submitting a form'),
}

/** The label of a browser-action risk category, generic for one this build has no words for. */
export function browserRiskLabel(
  riskCategory: string | undefined,
  i18n: ApprovalCopyI18n,
): string {
  const label =
    riskCategory && Object.hasOwn(BROWSER_RISK_LABELS, riskCategory)
      ? BROWSER_RISK_LABELS[riskCategory]
      : undefined
  return label ? label(i18n) : i18n.tr('Risikofylt handling', 'A risky action')
}

/** The state of a browser-action approval as the evidence drawer holds it. */
export type BrowserApprovalStatus = 'pending' | 'granted' | 'denied' | 'timed_out'

export function browserApprovalStatusLabel(
  status: BrowserApprovalStatus,
  i18n: ApprovalCopyI18n,
): string {
  switch (status) {
    case 'granted':
      return i18n.tr('Godkjent', 'Approved')
    case 'denied':
      return i18n.tr('Avslått', 'Denied')
    case 'timed_out':
      return i18n.tr('Tidsavbrutt', 'Timed out')
    case 'pending':
      return i18n.tr('Venter på godkjenning', 'Waiting for approval')
  }
}

type CauseCopy = { headline: string; reason: string }

// Total over the closed set: adding a code to `APPROVAL_CAUSE_CODES` without words here does not
// compile, and the test pins the set to the contract file.
const CAUSE_COPY: Record<CauseCode, (i18n: ApprovalCopyI18n, cause: ApprovalCause) => CauseCopy> = {
  owner_effect_reserved: (i18n) => ({
    headline: i18n.tr('Handling som endrer kontoen din', 'An action that changes your account'),
    reason: i18n.tr(
      'Dette verktøyet endrer data du eier, så det krever alltid en godkjenning fra deg.',
      'This tool changes data you own, so it always needs your approval.',
    ),
  }),
  consent_to_disclose: (i18n) => ({
    headline: i18n.tr('Deling av informasjon', 'Sharing information'),
    reason: i18n.tr(
      'Agenten vil bruke eller sende ut informasjon. Godkjenn bare hvis du er komfortabel med at den deles.',
      'The agent wants to use or send out information. Approve only if you are comfortable with it being shared.',
    ),
  }),
  risky_effect_under_ask: (i18n) => ({
    headline: i18n.tr('Handling med risiko', 'A risky action'),
    reason: i18n.tr(
      'Du har valgt at agenten skal spørre før risikable handlinger.',
      'You have set the agent to ask before risky actions.',
    ),
  }),
  provider_write: (i18n) => ({
    headline: i18n.tr('Skriving til en tilkoblet tjeneste', 'A write to a connected service'),
    reason: i18n.tr(
      'Agenten vil endre noe hos en tilkoblet tjeneste. Det kan være vanskelig å angre.',
      'The agent wants to change something in a connected service. It may be hard to undo.',
    ),
  }),
  capability_high_risk: (i18n) => ({
    headline: i18n.tr('Handling med høy risiko', 'A high-risk action'),
    reason: i18n.tr(
      'Funksjonen agenten vil bruke er merket som høyrisiko.',
      'The capability the agent wants to use is marked high-risk.',
    ),
  }),
  capability_policy_ask: (i18n) => ({
    headline: i18n.tr('En regel ber om godkjenning', 'A rule asks for approval'),
    reason: i18n.tr(
      'En regel for verktøy krever at du avgjør dette først.',
      'A tool rule requires you to decide this first.',
    ),
  }),
  hook_ask: (i18n) => ({
    headline: i18n.tr('En kontroll ber om godkjenning', 'A check asks for approval'),
    reason: i18n.tr(
      'En kontroll før verktøybruk ba om at en person ser på dette først.',
      'A check before tool use asked for a person to look at this first.',
    ),
  }),
  browser_action_risk: (i18n, cause) => ({
    headline: cause.riskCategory
      ? browserRiskLabel(cause.riskCategory, i18n)
      : i18n.tr('Handling i nettleseren', 'A browser action'),
    reason: i18n.tr(
      'Agenten foreslo en handling i nettleseren som regnes som risikofull. Nettleseren utfører den bare hvis du godkjenner.',
      'The agent proposed a browser action that counts as risky. The browser only carries it out if you approve.',
    ),
  }),
  auto_posture_provider_write: (i18n) => ({
    headline: i18n.tr('Skriving godkjent på dine vegne', 'A write approved on your behalf'),
    reason: i18n.tr(
      'Chat-modus godkjenner skriving til tilkoblede tjenester automatisk. Dette er en registrering, ikke et valg du må ta.',
      'Chat mode approves writes to connected services automatically. This is a record, not a choice you have to make.',
    ),
  }),
  unclassified_gate: (i18n) => ({
    headline: i18n.tr('Agenten venter på godkjenning', 'The agent is waiting for approval'),
    reason: i18n.tr(
      'Et steg krever at du avgjør før agenten fortsetter.',
      'A step needs your decision before the agent continues.',
    ),
  }),
}

/** What an approval with no stated cause says: that something waits, not what. */
export function approvalFallbackLine(i18n: ApprovalCopyI18n): string {
  return i18n.tr(
    'Agenten venter på godkjenning før den fortsetter.',
    'The agent is waiting for approval before it continues.',
  )
}

/** The title of an approval card. */
export function approvalHeadline(approval: ApprovalCopySource, i18n: ApprovalCopyI18n): string {
  if (approval.cause) return CAUSE_COPY[approval.cause.code](i18n, approval.cause).headline
  return i18n.tr(
    'Agenten venter på godkjenning før neste steg',
    'The agent is waiting for approval before the next step',
  )
}

/**
 * Why the person is being asked: the cause's sentence, else a detail the server wrote, else
 * nothing. A surface that must always show a line adds {@link approvalFallbackLine}.
 */
export function approvalReason(
  approval: ApprovalCopySource,
  i18n: ApprovalCopyI18n,
): string | undefined {
  if (approval.cause) return CAUSE_COPY[approval.cause.code](i18n, approval.cause).reason
  const detail = approval.detail?.trim()
  return detail ? detail : undefined
}

// The kinds the approvals proto names. A proof bundle or a run event states one as a bare
// string, in whichever casing its producer used, so the lookup strips the enum prefix.
const KIND_LABELS: Record<string, (i18n: ApprovalCopyI18n) => string> = {
  plan: (i18n) => i18n.tr('Plan', 'Plan'),
  tool: (i18n) => i18n.tr('Verktøykall', 'Tool call'),
  tool_call: (i18n) => i18n.tr('Verktøykall', 'Tool call'),
  permission: (i18n) => i18n.tr('Tillatelse', 'Permission'),
  destructive: (i18n) => i18n.tr('Ødeleggende handling', 'A destructive action'),
  cost: (i18n) => i18n.tr('Kostnad', 'Cost'),
}

/** A plain label for an approval kind a proof or an event states; never the wire token. */
export function approvalKindLabel(kind: string | undefined, i18n: ApprovalCopyI18n): string {
  const key = (kind ?? '')
    .trim()
    .toLowerCase()
    .replace(/^approval_kind_/, '')
    .replace(/[\s-]+/g, '_')
  const label = Object.hasOwn(KIND_LABELS, key) ? KIND_LABELS[key] : undefined
  return label ? label(i18n) : i18n.tr('Handling', 'Action')
}
