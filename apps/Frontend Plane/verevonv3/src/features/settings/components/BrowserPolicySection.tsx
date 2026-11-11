import { Loader2, ShieldCheck } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton, ToggleRow } from '@/features/settings/components/settings-ui'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import { getSession } from '@/shared/session/session-store'

/**
 * Settings > Browser (E3.2-g2): the organisation's browser policy for its agents.
 *
 * org-core stores the policy (WS7.H). The gateway serves it through the action contract
 * (E3.2-g1). This section draws the document and sends what an admin chose; the server decides
 * what is allowed. Four rules shape it:
 *
 * - It renders only when the policy API reports itself available. A failed load hides it too.
 *   It never reads a build-time flag, the same rule as the model access section.
 * - A save is a proposal. Setting the policy needs approval, so the copy says "sent for
 *   approval" and nothing is shown as applied until the stored document changes.
 * - A save replaces the whole document. Everything this section does not edit (capability keys
 *   it does not manage, robots_max, proxy_max) is carried over unchanged, because dropping a
 *   ceiling would loosen the policy.
 * - The viewer's own consent per website is listed, and each decision can be forgotten here
 *   (E3.2-e3). The decisions are per person, so a person sees and forgets only their own. A
 *   forgotten decision is not restored: the next browse from that site asks again.
 *
 * Until the action contract exists the section is wired to `unavailableBrowserPolicyPort`: it
 * stays hidden and sends nothing.
 */

export type BrowserCapMode = 'allowed' | 'disabled'

/** The org policy document as org-core stores it (the claim body), in the wire's snake_case. */
export type BrowserPolicyDocument = {
  caps?: Record<string, string>
  robots_max?: string
  proxy_max?: string
  domains?: { allow?: string[]; deny?: string[] }
}

/** One of the viewer's own per-origin decisions (user-core browser_consent), as the port maps it. */
export type BrowserOriginConsent = {
  startHost: string
  /** Further hosts the decision covers, besides the start host. */
  hosts: string[]
  decision: 'allow' | 'deny'
  scope: 'turn' | 'conversation' | 'always'
  /** The conversation the decision is bound to, when it is bound to one. Part of the key a forget sends. */
  conversationId?: string
  /** The turn the decision is bound to, when it is bound to one. Part of the key a forget sends. */
  turnId?: string
  decidedAt: string
}

export type BrowserPolicyView = {
  /** The revision the next save carries as If-Match. */
  revision: number
  /** Owners and admins: the controls are live. */
  canManage: boolean
  /** The stored document. Null for members, who see the effective view only. */
  document: BrowserPolicyDocument | null
  /** The stored document folded with the plan: what enforcement sees. */
  effective: BrowserPolicyDocument
  /** Keyed by the computed entitlement names, for example "web.paid_search". */
  entitlements: Record<string, boolean>
  /** The viewer's own per-origin decisions. Each one can be forgotten from here. */
  originConsents: BrowserOriginConsent[]
}

/** `available: false` hides the section, and so does a failed load. */
export type BrowserPolicyLoad = { available: false } | ({ available: true } & BrowserPolicyView)

export type BrowserPolicyProposal = {
  /** The stored revision this change was made against. */
  ifMatch: number
  document: BrowserPolicyDocument
}

export type BrowserPolicyProposalResult =
  | { status: 'waiting_approval'; runId: string }
  | { status: 'applied'; revision: number }

/**
 * The seam between this section and the action contract. The production port calls
 * browser.get_org_policy and browser.set_org_policy (E3.2-g1), and browser.list_origin_consents and
 * browser.revoke_origin_consent (E3.2-e3), through executeAction.
 */
export interface BrowserPolicyPort {
  load(orgId: string): Promise<BrowserPolicyLoad>
  propose(orgId: string, proposal: BrowserPolicyProposal): Promise<BrowserPolicyProposalResult>
  /** Forgets one of the viewer's own consent decisions. It is not restored. The section reloads on success. */
  revokeOriginConsent(consent: BrowserOriginConsent): Promise<void>
}

/** Thrown by a port when the If-Match revision is stale. The section reloads and keeps the draft. */
export class BrowserPolicyRevisionConflict extends Error {
  constructor() {
    super('browser policy revision conflict')
    this.name = 'BrowserPolicyRevisionConflict'
  }
}

/** Used until the action contract exists: the section stays hidden and nothing is sent. */
export const unavailableBrowserPolicyPort: BrowserPolicyPort = {
  load: async () => ({ available: false as const }),
  propose: async () => {
    throw new Error('Browser policy changes are not available yet.')
  },
  revokeOriginConsent: async () => {
    throw new Error('Browser policy changes are not available yet.')
  },
}

type Tr = (no: string, en: string) => string

type ManagedCap = {
  key: string
  /** The catalogue baseline. A mode equal to it is dropped from the document, which changes nothing. */
  baseline: BrowserCapMode
  /** Entitlement the switch needs before it can be turned on. */
  entitlement?: string
  /** The catalogue's zdr_implies_disabled: off for organizations with zero data retention. */
  zdrOff?: boolean
  title: (tr: Tr) => string
  help: (tr: Tr) => string
}

/**
 * The seven abilities a person switches here: the non-reserved abilities of the catalogue
 * (web-policy-catalogue.v1.json). browser.act.script is always off and has no switch, and
 * reserved keys have none either. Keys this list does not know are carried through on save and
 * never shown.
 */
const MANAGED_CAPS: ManagedCap[] = [
  {
    key: 'browser.act.input',
    baseline: 'allowed',
    title: (tr) => tr('Skrive i nettsider', 'Type into websites'),
    help: (tr) => tr('Agenten kan skrive i skjemafelt under autonome kjøringer.', 'The agent can type into form fields during autonomous runs.'),
  },
  {
    key: 'browser.act.interact',
    baseline: 'allowed',
    title: (tr) => tr('Klikke og bruke nettsider', 'Click and use websites'),
    help: (tr) => tr('Agenten kan klikke på knapper og lenker under autonome kjøringer.', 'The agent can click buttons and links during autonomous runs.'),
  },
  {
    key: 'browser.act.upload',
    baseline: 'allowed',
    title: (tr) => tr('Laste opp filer til nettsider', 'Upload files to websites'),
    help: (tr) => tr('Hver opplasting krever fortsatt godkjenning.', 'Each upload still needs approval.'),
  },
  {
    key: 'browser.act.download',
    baseline: 'allowed',
    title: (tr) => tr('Laste ned filer fra nettsider', 'Download files from websites'),
    help: (tr) => tr('Hver nedlasting krever fortsatt godkjenning.', 'Each download still needs approval.'),
  },
  {
    key: 'browser.profile.persistent',
    baseline: 'allowed',
    zdrOff: true,
    title: (tr) => tr('Beholde nettleserprofiler', 'Keep browser profiles'),
    help: (tr) => tr('Profiler beholder innlogginger og informasjonskapsler mellom økter.', 'Profiles keep sign-ins and cookies between sessions.'),
  },
  {
    key: 'browser.consent.reject',
    baseline: 'disabled',
    title: (tr) => tr('Avvise informasjonskapsel-bannere', 'Reject cookie banners'),
    help: (tr) => tr(
      'Den lokale nettleseren kan trykke på en knapp for å avvise informasjonskapsler. Den godtar aldri, og hver avvisning blir loggført.',
      'The desktop browser may press a reject-only button on cookie banners; it never accepts, and each dismissal is logged.',
    ),
  },
  {
    key: 'search.paid_provider',
    baseline: 'allowed',
    zdrOff: true,
    entitlement: 'web.paid_search',
    title: (tr) => tr('Betalt søk', 'Paid search'),
    help: (tr) => tr('Lar agenten bruke en betalt søketjeneste.', 'Lets the agent use a paid search service.'),
  },
]

/** Limits org-core enforces across both host lists together (quarry-core MAX_RULE_ENTRIES and MAX_RULE_BYTES). */
const MAX_DOMAIN_ENTRIES = 64
const MAX_DOMAIN_BYTES = 2048
const MAX_HOST_LENGTH = 253

export type BrowserHostProblem = 'empty' | 'wildcard' | 'host_only' | 'ip_literal' | 'invalid'

export type BrowserHostResult = { ok: true; host: string } | { ok: false; problem: BrowserHostProblem }

/**
 * The canonical form of one host name, or the first reason it is not one. The rules mirror
 * org-core's validWebPolicyHost, which mirrors quarry-core's valid_host: lower-case ASCII letters,
 * digits, hyphens and dots; no wildcard, port, path, IP literal or trailing dot; labels of 1 to 63
 * characters with no hyphen at either edge; and a last label that is not all digits.
 */
export function canonicalBrowserHost(raw: string): BrowserHostResult {
  const host = raw.trim().toLowerCase()
  if (host === '') return { ok: false, problem: 'empty' }
  if (host.includes('*')) return { ok: false, problem: 'wildcard' }
  if (host.includes('[') || host.includes(']')) return { ok: false, problem: 'ip_literal' }
  if (/[\s\/\\?#@:]/.test(host)) return { ok: false, problem: 'host_only' }
  if (host.length > MAX_HOST_LENGTH || !/^[a-z0-9.-]+$/.test(host)) return { ok: false, problem: 'invalid' }
  const labels = host.split('.')
  const badLabel = labels.some(
    (label) => label === '' || label.length > 63 || label.startsWith('-') || label.endsWith('-'),
  )
  if (badLabel) return { ok: false, problem: 'invalid' }
  if (/^[0-9]+$/.test(labels[labels.length - 1] ?? '')) return { ok: false, problem: 'ip_literal' }
  return { ok: true, host }
}

export type BrowserHostListResult = {
  /** Canonical, de-duplicated and sorted, as org-core stores them. */
  hosts: string[]
  /** The tokens that are not host names, in the order typed. */
  problems: Array<{ token: string; problem: BrowserHostProblem }>
}

/** One list as typed: split on whitespace or commas, then canonicalised, de-duplicated and sorted. */
export function canonicalBrowserHostList(text: string): BrowserHostListResult {
  const hosts = new Set<string>()
  const problems: BrowserHostListResult['problems'] = []
  for (const token of text.split(/[\s,]+/)) {
    if (token === '') continue
    const result = canonicalBrowserHost(token)
    if (result.ok) hosts.add(result.host)
    else problems.push({ token, problem: result.problem })
  }
  return { hosts: [...hosts].sort(), problems }
}

export type BrowserPolicyDraft = {
  /** A mode for every managed capability. */
  caps: Record<string, BrowserCapMode>
  allow: string[]
  deny: string[]
}

function capModeIn(doc: BrowserPolicyDocument | null, cap: ManagedCap): BrowserCapMode {
  const mode = doc?.caps?.[cap.key]
  return mode === 'allowed' || mode === 'disabled' ? mode : cap.baseline
}

/** The draft a stored document shows before any edit. */
export function draftFromDocument(doc: BrowserPolicyDocument | null): BrowserPolicyDraft {
  const caps: Record<string, BrowserCapMode> = {}
  for (const cap of MANAGED_CAPS) caps[cap.key] = capModeIn(doc, cap)
  return {
    caps,
    allow: [...(doc?.domains?.allow ?? [])],
    deny: [...(doc?.domains?.deny ?? [])],
  }
}

/**
 * The document a save sends: the stored document with the managed switches and both host lists
 * replaced. Unmanaged capability keys, robots_max and proxy_max are carried over unchanged. An
 * empty container is omitted, as org-core normalises it.
 */
export function buildBrowserPolicyDocument(
  stored: BrowserPolicyDocument | null,
  draft: BrowserPolicyDraft,
): BrowserPolicyDocument {
  const caps: Record<string, string> = { ...(stored?.caps ?? {}) }
  for (const cap of MANAGED_CAPS) {
    const mode = draft.caps[cap.key] ?? cap.baseline
    if (mode === cap.baseline) delete caps[cap.key]
    else caps[cap.key] = mode
  }
  const next: BrowserPolicyDocument = {}
  if (Object.keys(caps).length > 0) next.caps = caps
  if (stored?.robots_max !== undefined) next.robots_max = stored.robots_max
  if (stored?.proxy_max !== undefined) next.proxy_max = stored.proxy_max
  if (draft.allow.length > 0 || draft.deny.length > 0) {
    next.domains = { allow: [...draft.allow], deny: [...draft.deny] }
  }
  return next
}

/** JSON with sorted keys, so two documents compare equal when they say the same thing. */
function stableJson(value: unknown): string {
  return JSON.stringify(value, (_key, inner: unknown) => {
    if (inner === null || typeof inner !== 'object' || Array.isArray(inner)) return inner
    return Object.fromEntries(
      Object.entries(inner).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
    )
  })
}

function hostProblemText(token: string, problem: BrowserHostProblem, tr: Tr): string {
  const reasons: Record<BrowserHostProblem, string> = {
    empty: tr('Skriv et domenenavn.', 'Enter a host name.'),
    wildcard: tr('Jokertegn støttes ikke. Skriv det eksakte domenet.', 'Wildcards are not supported. Write the exact host.'),
    host_only: tr('Skriv bare domenenavnet, uten https://, sti eller port.', 'Write the host name only, without https://, a path or a port.'),
    ip_literal: tr('IP-adresser kan ikke brukes. Skriv et domenenavn.', 'IP addresses cannot be used. Write a host name.'),
    invalid: tr(
      'Ikke et gyldig domenenavn. Bruk bokstavene a–z, tall, bindestrek og punktum.',
      'Not a valid host name. Use the letters a–z, digits, hyphens and dots.',
    ),
  }
  return `«${token}»: ${reasons[problem]}`
}

function consentScopeText(scope: BrowserOriginConsent['scope'], tr: Tr): string {
  if (scope === 'turn') return tr('Denne gangen', 'This turn')
  if (scope === 'conversation') return tr('Denne samtalen', 'This conversation')
  return tr('Alltid', 'Always')
}

/** The decision, its scope and the further hosts it covers, on one line. */
function consentDetail(consent: BrowserOriginConsent, tr: Tr): string {
  const decision = consent.decision === 'allow' ? tr('Tillatt', 'Allowed') : tr('Nektet', 'Denied')
  const parts = [decision, consentScopeText(consent.scope, tr)]
  if (consent.hosts.length > 0) parts.push(consent.hosts.join(', '))
  return parts.join(' · ')
}

/** Identifies one decision on screen. It is the same key the forget sends, so a row maps to one stored decision. */
function consentKey(consent: BrowserOriginConsent): string {
  return JSON.stringify([consent.startHost, consent.scope, consent.conversationId ?? null, consent.turnId ?? null])
}

function HostListField(props: {
  disabled: boolean
  help: string
  id: string
  label: string
  onInput: (value: string) => void
  problems: string[]
  value: string
}) {
  return (
    <label for={props.id} class="verevon-settings-field">
      <span class="verevon-settings-label">{props.label}</span>
      <span class="verevon-settings-help">{props.help}</span>
      <VerevonTextarea
        id={props.id}
        value={props.value}
        disabled={props.disabled}
        rows={4}
        class="verevon-settings-input verevon-settings-textarea"
        onInput={(event) => props.onInput(event.currentTarget.value)}
      />
      <For each={props.problems}>
        {(problem) => (
          <span class="verevon-settings-help" role="alert">
            {problem}
          </span>
        )}
      </For>
    </label>
  )
}

export function BrowserPolicySection(props: { port?: BrowserPolicyPort }) {
  const i18n = useI18n()
  const tr: Tr = (no, en) => i18n.tr(no, en)
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const port = (): BrowserPolicyPort => props.port ?? unavailableBrowserPolicyPort

  // A failed load hides the section, exactly as a report of "not available" does.
  const [load, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => port().load(id).catch((): BrowserPolicyLoad => ({ available: false })),
  )
  const view = createMemo<BrowserPolicyView | undefined>(() => {
    const current = load()
    return current && current.available ? current : undefined
  })

  const [capDraft, setCapDraft] = createSignal<Record<string, BrowserCapMode> | null>(null)
  const [allowText, setAllowText] = createSignal<string | null>(null)
  const [denyText, setDenyText] = createSignal<string | null>(null)
  const [saving, setSaving] = createSignal(false)
  const [saveError, setSaveError] = createSignal<string | null>(null)
  const [savedNote, setSavedNote] = createSignal<string | null>(null)
  // Forgetting a consent takes two clicks: the first arms its row, the second sends it.
  const [forgetArmed, setForgetArmed] = createSignal<string | null>(null)
  const [forgetting, setForgetting] = createSignal<string | null>(null)
  const [forgetError, setForgetError] = createSignal<string | null>(null)

  const canManage = () => view()?.canManage === true
  const stored = (): BrowserPolicyDocument | null => view()?.document ?? null
  // Before any edit the controls show the stored document; members see the effective view.
  const shown = (): BrowserPolicyDocument => view()?.document ?? view()?.effective ?? {}
  const shownDraft = createMemo(() => draftFromDocument(shown()))

  const capMode = (cap: ManagedCap): BrowserCapMode =>
    capDraft()?.[cap.key] ?? shownDraft().caps[cap.key] ?? cap.baseline
  const currentCaps = (): Record<string, BrowserCapMode> => {
    const caps: Record<string, BrowserCapMode> = {}
    for (const cap of MANAGED_CAPS) caps[cap.key] = capMode(cap)
    return caps
  }
  const entitled = (cap: ManagedCap): boolean =>
    !cap.entitlement || view()?.entitlements[cap.entitlement] === true

  const allowShown = (): string => allowText() ?? (shown().domains?.allow ?? []).join('\n')
  const denyShown = (): string => denyText() ?? (shown().domains?.deny ?? []).join('\n')
  const allowList = createMemo(() => canonicalBrowserHostList(allowShown()))
  const denyList = createMemo(() => canonicalBrowserHostList(denyShown()))
  const listProblem = createMemo<string | null>(() => {
    const hosts = [...allowList().hosts, ...denyList().hosts]
    if (hosts.length > MAX_DOMAIN_ENTRIES) {
      return tr(
        `Høyst ${MAX_DOMAIN_ENTRIES} domener totalt, over begge listene.`,
        `At most ${MAX_DOMAIN_ENTRIES} domains in total, across both lists.`,
      )
    }
    if (hosts.reduce((sum, host) => sum + host.length, 0) > MAX_DOMAIN_BYTES) {
      return tr('Domenene fyller mer enn 2 KiB totalt.', 'The domains take more than 2 KiB in total.')
    }
    return null
  })
  const hostsValid = () =>
    allowList().problems.length === 0 && denyList().problems.length === 0 && listProblem() === null

  const nextDocument = createMemo<BrowserPolicyDocument | null>(() => {
    if (!hostsValid()) return null
    return buildBrowserPolicyDocument(stored(), {
      caps: currentCaps(),
      allow: allowList().hosts,
      deny: denyList().hosts,
    })
  })
  // Dirty means the save would change what the stored document says, not merely its bytes.
  const dirty = createMemo(() => {
    const next = nextDocument()
    if (!next) return false
    return stableJson(next) !== stableJson(buildBrowserPolicyDocument(stored(), draftFromDocument(stored())))
  })
  const edited = () => capDraft() !== null || allowText() !== null || denyText() !== null

  const clearNotes = () => {
    setSaveError(null)
    setSavedNote(null)
  }

  const toggleCap = (cap: ManagedCap, on: boolean) => {
    clearNotes()
    const caps = currentCaps()
    caps[cap.key] = on ? 'allowed' : 'disabled'
    setCapDraft(caps)
  }

  const onAllowInput = (value: string) => {
    clearNotes()
    setAllowText(value)
  }

  const onDenyInput = (value: string) => {
    clearNotes()
    setDenyText(value)
  }

  const discard = () => {
    clearNotes()
    setCapDraft(null)
    setAllowText(null)
    setDenyText(null)
  }

  const capLocked = (cap: ManagedCap, on: boolean): boolean =>
    !canManage() || saving() || (!on && !entitled(cap))

  const capNotes = (cap: ManagedCap): string | undefined => {
    const notes: string[] = []
    if (cap.zdrOff) {
      notes.push(tr(
        'Slås av automatisk for organisasjoner med nulloppbevaring.',
        'Turned off automatically for organizations with zero data retention.',
      ))
    }
    if (!entitled(cap)) notes.push(tr('Ikke med i denne planen.', 'Not part of this plan.'))
    return notes.length > 0 ? notes.join(' ') : undefined
  }

  const save = async () => {
    const current = view()
    const next = nextDocument()
    if (!current || !current.canManage || !next || !dirty() || saving()) return
    setSaving(true)
    clearNotes()
    try {
      const result = await port().propose(orgId(), { ifMatch: current.revision, document: next })
      setCapDraft(null)
      setAllowText(null)
      setDenyText(null)
      setSavedNote(
        result.status === 'waiting_approval'
          ? tr(
              'Sendt til godkjenning. Ingenting endres før endringen er godkjent.',
              'Sent for approval. Nothing changes until the change is approved.',
            )
          : tr('Lagret.', 'Saved.'),
      )
      await refetch()
    } catch (err) {
      if (err instanceof BrowserPolicyRevisionConflict) {
        // The draft stays on screen. The reload makes the next send carry the current revision.
        setSaveError(
          tr(
            'Innstillingene ble endret av noen andre. Den nyeste versjonen er lastet. Sjekk utkastet og send på nytt.',
            'The settings were changed by someone else. The latest version is loaded. Check your draft and send it again.',
          ),
        )
        await refetch()
      } else {
        setSaveError(
          translateApiError(err, tr, {
            no: 'Kunne ikke sende endringen.',
            en: 'Could not send the change.',
          }),
        )
      }
    } finally {
      setSaving(false)
    }
  }

  const forget = async (consent: BrowserOriginConsent) => {
    const key = consentKey(consent)
    if (forgetting() !== null) return
    if (forgetArmed() !== key) {
      setForgetArmed(key)
      setForgetError(null)
      return
    }
    setForgetting(key)
    setForgetError(null)
    setSavedNote(null)
    try {
      await port().revokeOriginConsent(consent)
      setSavedNote(tr('Samtykket er glemt.', 'The consent was forgotten.'))
    } catch (err) {
      setForgetError(
        translateApiError(err, tr, {
          no: 'Kunne ikke glemme samtykket. Prøv igjen.',
          en: 'Could not forget the consent. Try again.',
        }),
      )
    } finally {
      setForgetArmed(null)
      setForgetting(null)
      // The list reloads either way: a refused forget can still have changed what is stored.
      await refetch()
    }
  }

  return (
    <Show when={view()}>
      {(current) => (
        <>
          <SectionHeader
            title={tr('Nettleser', 'Browser')}
            description={tr(
              'Hva agenter i organisasjonen kan gjøre i nettleseren, og hvilke domener de kan besøke. Endringer gjelder først når de er godkjent.',
              'What agents in the organization can do in the browser, and which domains they can visit. Changes apply once they are approved.',
            )}
          />

          <Show when={!current().canManage}>
            <p class="verevon-settings-subnote" data-testid="browser-policy-read-only">
              <ShieldCheck size={14} aria-hidden="true" />{' '}
              {tr('Bare administratorer kan endre nettleserinnstillingene.', 'Only administrators can change browser settings.')}
            </p>
          </Show>

          <h3 class="verevon-settings-label">{tr('Handlinger', 'Actions')}</h3>
          <div class="verevon-settings-field-grid" data-testid="browser-policy-caps">
            <For each={MANAGED_CAPS}>
              {(cap) => (
                <ToggleRow
                  title={cap.title(tr)}
                  description={cap.help(tr)}
                  enabled={capMode(cap) !== 'disabled'}
                  disabled={capLocked(cap, capMode(cap) !== 'disabled')}
                  info={capNotes(cap)}
                  onChange={(checked) => toggleCap(cap, checked)}
                />
              )}
            </For>
          </div>
          <p class="verevon-settings-subnote">
            {tr(
              'Egne skript på nettsider er alltid av og kan ikke slås på.',
              'Custom scripts on websites are always off and cannot be switched on.',
            )}
          </p>

          <h3 class="verevon-settings-label">{tr('Domener', 'Domains')}</h3>
          <HostListField
            id="browser-policy-allow"
            label={tr('Tillatte domener', 'Allowed domains')}
            help={tr(
              'Ett domene per linje, for eksempel example.com. Underdomener er også tillatt.',
              'One domain per line, for example example.com. Subdomains are allowed too.',
            )}
            value={allowShown()}
            disabled={!canManage() || saving()}
            problems={allowList().problems.map((entry) => hostProblemText(entry.token, entry.problem, tr))}
            onInput={onAllowInput}
          />
          <HostListField
            id="browser-policy-deny"
            label={tr('Blokkerte domener', 'Blocked domains')}
            help={tr(
              'Blokkerte domener nektes alltid, også når noen har godkjent dem.',
              'Blocked domains are always refused, even when someone has approved them.',
            )}
            value={denyShown()}
            disabled={!canManage() || saving()}
            problems={denyList().problems.map((entry) => hostProblemText(entry.token, entry.problem, tr))}
            onInput={onDenyInput}
          />
          <Show when={listProblem()}>
            {(message) => (
              <p class="verevon-settings-help" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <h3 class="verevon-settings-label">{tr('Dine samtykker per nettsted', 'Your consent per website')}</h3>
          <Show
            when={current().originConsents.length > 0}
            fallback={
              <p class="verevon-settings-subnote" data-testid="browser-policy-consents-empty">
                {tr('Du har ingen lagrede samtykker per nettsted.', 'You have no saved consent per website.')}
              </p>
            }
          >
            <div class="verevon-settings-list-card" data-testid="browser-policy-consents">
              <For each={current().originConsents}>
                {(consent) => {
                  const key = consentKey(consent)
                  return (
                    <div class="verevon-settings-list-row">
                      <div>
                        <p>{consent.startHost}</p>
                        <span>{consentDetail(consent, tr)}</span>
                      </div>
                      <SettingsButton
                        settingsSize="sm"
                        danger={forgetArmed() === key}
                        disabled={saving() || forgetting() !== null}
                        onClick={() => void forget(consent)}
                      >
                        <Show when={forgetting() === key}>
                          <Loader2 size={14} aria-hidden="true" />
                        </Show>
                        {forgetArmed() === key ? tr('Bekreft glemsel?', 'Confirm forgetting?') : tr('Glem', 'Forget')}
                      </SettingsButton>
                    </div>
                  )
                }}
              </For>
            </div>
            <p class="verevon-settings-help">
              {tr(
                'Et glemt samtykke kan ikke gjenopprettes. Neste gang agenten vil åpne nettstedet, spørres du på nytt.',
                'A forgotten consent cannot be restored. The next time the agent wants to open that website, you are asked again.',
              )}
            </p>
          </Show>
          <Show when={forgetError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <Show when={canManage() && edited()}>
            <div class="verevon-settings-actions">
              <p>{tr('Endringen sendes til godkjenning før den gjelder.', 'The change is sent for approval before it applies.')}</p>
              <div>
                <SettingsButton settingsSize="sm" disabled={saving()} onClick={discard}>
                  {tr('Forkast', 'Discard')}
                </SettingsButton>{' '}
                <SettingsButton
                  variant="primary"
                  settingsSize="sm"
                  disabled={saving() || !dirty()}
                  onClick={() => void save()}
                >
                  <Show when={saving()} fallback={tr('Send til godkjenning', 'Send for approval')}>
                    <Loader2 size={14} aria-hidden="true" /> {tr('Sender…', 'Sending…')}
                  </Show>
                </SettingsButton>
              </div>
            </div>
          </Show>

          <Show when={saveError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>
          <Show when={savedNote()}>
            {(message) => (
              <p class="verevon-settings-status-message" role="status">
                {message()}
              </p>
            )}
          </Show>
        </>
      )}
    </Show>
  )
}
