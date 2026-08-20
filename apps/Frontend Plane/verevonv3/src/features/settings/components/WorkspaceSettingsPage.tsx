import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from 'solid-js'
import RouterPolicyPage from '@/features/router-policy/components/RouterPolicyPage'
import FinetuneJobsPage from '@/features/finetune/components/FinetuneJobsPage'
import { OrgDeletionDangerZone } from '@/features/settings/components/OrgDeletionDangerZone'
import { TrustCenterSection } from '@/features/settings/components/TrustCenterSection'
import { McpServersSection } from '@/features/settings/components/McpServersSection'
import { SkillsSection } from '@/features/settings/components/SkillsSection'
import { OrgInstructionsSection } from '@/features/settings/components/OrgInstructionsSection'
import { PluginsSection } from '@/features/settings/components/PluginsSection'
import { CronSchedulesSection } from '@/features/settings/components/CronSchedulesSection'
import { OrgQuotasSection } from '@/features/settings/components/OrgQuotasSection'
import { MemorySection } from '@/features/settings/components/MemorySection'
import { HyperswitchCheckout } from '@/features/billing/components/HyperswitchCheckout'
import { NexiCheckout } from '@/features/billing/components/NexiCheckout'
import { reserveDirectOauthWindow, runDirectOauthWindow } from '@/shared/integrations/provider-auth-window'
import { connectBundlesForProvider, instagramInboxConnectionRequest } from '@/features/settings/lib/integration-bundles'
import {
  confirmBillingCheckout,
  loadBillingAccount,
  startBillingCheckout,
  type BillingAccount,
  type CheckoutSession,
} from '@/features/billing/lib/api'
import { resolveCheckoutSurface } from '@/features/billing/lib/checkout'
import {
  billingPlans,
  isCheckoutActivatingStatus,
  paidBillingPlan,
  planLabel,
  type BillingPlanId,
} from '@/features/billing/lib/plans'
import { requestJson } from '@/shared/api/http'
import { listAuditEvents, type AuditEvent } from '@/shared/api/audit-client'
import {
  inviteMember,
  removeMember,
  updateMemberRole,
  type MembershipRole,
} from '@/shared/api/membership-client'
import type { MetaFacebookSdkConfig } from '@/shared/integrations/meta-facebook-sdk'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import {
  getOrganizationSupportAIMode,
  getOrganizationZdr,
  getOrganizationZdrEntitled,
  updateOrganizationSupportAIMode,
  updateOrganizationZdr,
  type SupportAIMode,
} from '@/shared/api/organization-client'
import { triggerSync } from '@/shared/api/integrations-client'
import { ApiError } from '@/shared/api/http'
import {
  getWorkspaceSettingsSection,
  isWorkspaceSettingsSection,
  workspaceSettingsSectionIds,
  workspaceSettingsSections,
  type WorkspaceSettingsSectionId,
} from '@/features/settings/lib/settings-sections'
import {
  DataRow,
  FeaturePanel,
  Metric,
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsHero,
  SettingsSaveActions,
  SettingsSelect,
  SettingsSurface,
  StatusGrid,
  ToggleRow,
  type StatusCard,
} from '@/features/settings/components/settings-ui'
import { useI18n } from '@/shared/i18n'

export type { WorkspaceSettingsSectionId }
export { getWorkspaceSettingsSection, isWorkspaceSettingsSection, workspaceSettingsSectionIds, workspaceSettingsSections }

type LiveMember = { userId: string; name?: string; email: string; role: string; status: string }
type MemberListResponse = { members?: unknown[]; count?: number }

type IntegrationCapability = {
  key: string
  label?: string
  description?: string
  direction: 'read' | 'write'
  scopes?: string[]
  sensitive?: boolean
}

type IntegrationSettingsProvider = {
  key: string
  label: string
  category: string
  configured: boolean
  status: string
  missingConfig: string[]
  directOAuthReady: boolean
  capabilities: IntegrationCapability[]
  metaSdk?: MetaFacebookSdkConfig
  /** Provider replacing this one (e.g. facebook/instagram/whatsapp/meta-ads →
   * "meta"). Superseded providers are hidden from new-connection lists but
   * still render for existing connections. */
  supersededBy?: string
}

type IntegrationSettingsConnection = {
  id: string
  providerKey: string
  providerLabel: string
  displayName: string
  providerAccountId?: string
  providerEmail?: string
  providerWorkspaceName?: string
  status: string
  capabilities: string[]
  scopeCount: number
  syncStatus: string
  deletedAt?: string
  latestSyncJob?: { status: string; updatedAt?: string }
}

type IntegrationSettingsSummary = {
  metrics: {
    connected: number
    failed: number
    readyProviders: number
    syncing: number
    totalProviders: number
  }
  providers: IntegrationSettingsProvider[]
  connections: IntegrationSettingsConnection[]
}

type IntegrationSettingsProvidersResponse = { providers?: unknown[] }
type IntegrationSettingsConnectionsResponse = { connections?: unknown[] }

type ConnectSessionResult = {
  authMode?: 'direct-oauth'
  connectUrl: string
  expiresAt?: string
  providerConfigKey?: string
  sessionToken?: string
}

type IntegrationSettingsRow = {
  action: 'admin' | 'connect' | 'loading' | 'missing' | 'connected'
  connection?: IntegrationSettingsConnection
  detail: string
  name: string
  provider: IntegrationSettingsProvider
  status: string
}

// Phase 4 PR-2 de-fake: org-security policy controls. No real org-security /
// MFA / domain-restriction source is wired behind the gateway today, so these
// render as honest, DISABLED "not configured" controls — a security control is
// never shown enabled from a literal. (No boolean-`true` posture remains, so
// the A8 no-fabricated-state guard needs no suppression here.)
function getSecurityControls(i18n: ReturnType<typeof useI18n>): { title: string; description: string }[] {
  return [
    {
      title: i18n.tr('Krev MFA for administratorer', 'Require MFA for admins'),
      description: i18n.tr(
        'Administratorer må bruke flerfaktorautentisering før de får tilgang til organisasjonsinnstillinger.',
        'Admins must use multi-factor authentication before accessing organization settings.',
      ),
    },
    {
      title: i18n.tr('Begrens innlogging til verifiserte domener', 'Restrict sign-in to verified domains'),
      description: i18n.tr(
        'Kun brukere med godkjente arbeidsområdedomener kan logge inn.',
        'Only users with approved workspace domains can sign in.',
      ),
    },
    {
      title: i18n.tr('Loggfør endringer i administratorkonfigurasjon', 'Log admin configuration changes'),
      description: i18n.tr(
        'Behold en revisjonslogg for endringer i fakturering, medlemmer, SSO og integrasjoner.',
        'Keep an audit trail for billing, member, SSO, and integration changes.',
      ),
    },
  ]
}

const sectionStatusCards: Record<WorkspaceSettingsSectionId, StatusCard[]> = {
  // Phase 4 PR-2 de-fake: the workspace + integrations status grids carried a
  // fabricated posture ('Verified' / 'coresystem.no is ready' / 'Connected apps
  // 2 / 4' / 'Healthy, synced 8 minutes ago'). No real producer exists, so the
  // grids are honestly empty; integration counts are shown truthfully by the
  // live <Metric> grid in IntegrationsSection.
  workspace: [],
  members: [],
  // Cross-org user directory renders its own live list; no shared status grid.
  'platform-users': [],
  billing: [],
  // SSO + org-security status are not wired to a real source; show no
  // fabricated "Verified / Required / 365 days" cards.
  sso: [],
  'org-security': [],
  integrations: [],
  // Trust Center renders its own live transparency cards, and router-policy /
  // fine-tune render dedicated pages — so these carry no shared status grid.
  trust: [],
  'router-policy': [],
  finetune: [],
  // MCP servers renders its own live list + form, so it carries no shared status grid.
  mcp: [],
  // Skills renders its own live list + form, so it carries no shared status grid.
  skills: [],
  // Org instructions renders its own live field + form, so no shared status grid.
  'org-instructions': [],
  // Plugins renders its own live list + form, so it carries no shared status grid.
  plugins: [],
  // Cron schedules render their own live list + form, so no shared status grid.
  cron: [],
  // Quotas render their own live fields with the persisted limit and usage, so
  // no shared status grid — and no fabricated "within limits" posture.
  quotas: [],
  // Memory renders its own live list, so it carries no shared status grid.
  memory: [],
}

function getBusinessHourRows(i18n: ReturnType<typeof useI18n>) {
  return [
    { day: i18n.tr('Mandag-fredag', 'Monday-Friday'), hours: '08:00-17:00', inbox: i18n.tr('Prioritert support', 'Priority support') },
    { day: i18n.tr('Lørdag', 'Saturday'), hours: '10:00-14:00', inbox: i18n.tr('Overflow-kø', 'Overflow') },
  ]
}

function recordFrom(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' ? value as Record<string, unknown> : {}
}

function stringValue(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'string' && value.trim()) return value.trim()
  }
  return ''
}

function normalizeMember(value: unknown): LiveMember | null {
  const member = recordFrom(value)
  const user = recordFrom(member.user)
  const userId = stringValue(member, 'userId', 'user_id') || stringValue(user, 'id')
  const email = stringValue(
    member,
    'email',
    'userEmail',
    'user_email',
    'invited_email',
    'invitedEmail',
  ) || stringValue(user, 'email') || userId
  if (!userId && !email) return null

  return {
    userId: userId || email,
    name: stringValue(member, 'name', 'displayName', 'display_name')
      || stringValue(user, 'name', 'displayName', 'display_name')
      || undefined,
    email,
    role: stringValue(member, 'role') || 'member',
    status: stringValue(member, 'status') || 'active',
  }
}

function normalizeMemberList(payload: unknown): LiveMember[] {
  const record = recordFrom(payload)
  const members = Array.isArray(record.members)
    ? record.members
    : Array.isArray(payload)
      ? payload
      : []

  return members.map(normalizeMember).filter((member): member is LiveMember => Boolean(member))
}

export function VerevonWorkspaceSettingsPage(props: {
  section?: WorkspaceSettingsSectionId
}) {
  const i18n = useI18n()
  const session = getSession()
  const section = () => props.section ?? 'workspace'
  const details = () => getWorkspaceSettingsSection(section())
  const orgId = () => session.activeOrg?.id ?? null
  const [billingAccount, setBillingAccount] = createSignal<BillingAccount | null>(null)
  const [billingLoading, setBillingLoading] = createSignal(false)
  const [billingError, setBillingError] = createSignal<string | null>(null)

  async function refreshBillingAccount(signal?: AbortSignal) {
    setBillingLoading(true)
    setBillingError(null)
    try {
      setBillingAccount(await loadBillingAccount(signal))
    } catch (reason) {
      if (reason instanceof Error && reason.name === 'AbortError') return
      setBillingError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke laste faktureringskonto.', 'Could not load billing account.'))
    } finally {
      if (!signal?.aborted) setBillingLoading(false)
    }
  }

  createEffect(() => {
    if (section() !== 'billing') return

    const controller = new AbortController()
    void refreshBillingAccount(controller.signal)
    onCleanup(() => controller.abort())
  })

  const billingStatusCards = (): StatusCard[] => [
    {
      label: i18n.tr('Gjeldende plan', 'Current plan'),
      value: billingAccount() ? planLabel(billingAccount()?.plan) : '—',
      detail: billingError() ?? (billingAccount()?.subscription_state
        ? `${i18n.tr('Status', 'Status')}: ${billingAccount()?.subscription_state}`
        : billingLoading()
          ? i18n.tr('Laster planinformasjon …', 'Loading plan information...')
          : i18n.tr('Planinformasjon utilgjengelig', 'Plan information unavailable')),
      tone: billingAccount()?.subscription_state === 'past_due' ? 'warn' : billingAccount() ? 'ok' : 'neutral',
    },
    {
      label: i18n.tr('Kreditter', 'Credits'),
      value: billingAccount()?.credits != null ? String(billingAccount()?.credits) : '—',
      detail: i18n.tr('Tilgjengelige kreditter på denne planen', 'Available credits on this plan'),
      tone: 'neutral',
    },
    {
      label: i18n.tr('Betalingsmetode', 'Payment method'),
      value: billingAccount()?.provider_customer_id?.payment ? i18n.tr('Lagret', 'Stored') : '—',
      detail: billingAccount()?.provider_customer_id?.payment
        ? i18n.tr('Betalingskunde er synkronisert.', 'Payment customer is synced.')
        : i18n.tr('Legg til en betalingsmetode gjennom kassen.', 'Add a payment method through checkout.'),
      tone: billingAccount()?.provider_customer_id?.payment ? 'ok' : 'neutral',
    },
  ]

  const liveStatusCards = createMemo<Record<WorkspaceSettingsSectionId, StatusCard[]>>(() => ({
    ...sectionStatusCards,
    billing: billingStatusCards(),
    members: sectionStatusCards.members,
  }))

  return (
    <Switch
      fallback={
        <WorkspaceSettingsChrome
          section={section()}
          details={details()}
          orgId={orgId()}
          billingAccount={billingAccount()}
          billingError={billingError()}
          billingLoading={billingLoading()}
          liveStatusCards={liveStatusCards()}
          onRefreshBilling={() => refreshBillingAccount()}
        />
      }
    >
      <Match when={section() === 'router-policy'}>
        <RouterPolicyPage />
      </Match>
      <Match when={section() === 'finetune'}>
        <FinetuneJobsPage />
      </Match>
    </Switch>
  )
}

function WorkspaceSettingsChrome(props: {
  section: WorkspaceSettingsSectionId
  details: ReturnType<typeof getWorkspaceSettingsSection>
  orgId: string | null
  billingAccount: BillingAccount | null
  billingError: string | null
  billingLoading: boolean
  liveStatusCards: Record<WorkspaceSettingsSectionId, StatusCard[]>
  onRefreshBilling: () => Promise<void>
}) {
  const i18n = useI18n()
  const details = () => props.details
  const section = () => props.section
  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow={i18n.tr('Administrator', 'Admin')}
        title={details().title}
        description={details().description}
      />

      <Show when={props.liveStatusCards[section()].length > 0}>
        <StatusGrid cards={props.liveStatusCards[section()]} />
      </Show>

      <section id={details().id} class="verevon-settings-section verevon-settings-section--after-status">
        <WorkspaceSettingsSection
          section={section()}
          orgId={props.orgId}
          billingAccount={props.billingAccount}
          billingError={props.billingError}
          billingLoading={props.billingLoading}
          onRefreshBilling={props.onRefreshBilling}
        />
      </section>

      <SettingsSaveActions saveLabel={details().saveLabel} />
    </SettingsSurface>
  )
}

export default function SettingsPage(props: { section?: string }) {
  const section = () => props.section && isWorkspaceSettingsSection(props.section)
    ? props.section
    : 'workspace'

  return <VerevonWorkspaceSettingsPage section={section()} />
}

function WorkspaceSettingsSection(props: {
  billingAccount: BillingAccount | null
  billingError: string | null
  billingLoading: boolean
  section: WorkspaceSettingsSectionId
  orgId: string | null
  onRefreshBilling: () => Promise<void>
}) {
  return (
    <Switch fallback={<WorkspaceSection />}>
      <Match when={props.section === 'members'}>
        <MembersSection orgId={props.orgId} />
      </Match>
      <Match when={props.section === 'platform-users'}>
        <PlatformUsersSection />
      </Match>
      <Match when={props.section === 'billing'}>
        <BillingSection
          account={props.billingAccount}
          error={props.billingError}
          loading={props.billingLoading}
          onRefresh={props.onRefreshBilling}
        />
      </Match>
      <Match when={props.section === 'sso'}>
        <SsoSection />
      </Match>
      <Match when={props.section === 'org-security'}>
        <OrgSecuritySection />
      </Match>
      <Match when={props.section === 'integrations'}>
        <IntegrationsSection />
      </Match>
      <Match when={props.section === 'trust'}>
        <TrustCenterSection />
      </Match>
      <Match when={props.section === 'mcp'}>
        <McpServersSection />
      </Match>
      <Match when={props.section === 'skills'}>
        <SkillsSection />
      </Match>
      <Match when={props.section === 'org-instructions'}>
        <OrgInstructionsSection />
      </Match>
      <Match when={props.section === 'plugins'}>
        <PluginsSection />
      </Match>
      <Match when={props.section === 'cron'}>
        <CronSchedulesSection />
      </Match>
      <Match when={props.section === 'quotas'}>
        <OrgQuotasSection />
      </Match>
      <Match when={props.section === 'memory'}>
        <MemorySection />
      </Match>
    </Switch>
  )
}

function WorkspaceSection() {
  const i18n = useI18n()
  return (
    <>
      <SectionHeader
        title={i18n.tr('Grunnleggende arbeidsområde', 'Workspace basics')}
        description={i18n.tr(
          'Delte arbeidsområdefelt som påvirker URL-er, standardverdier og support-ruting.',
          'Shared workspace fields that affect URLs, defaults, and support routing.',
        )}
      />
      <div class="verevon-settings-field-grid">
        <SettingsField id="workspace-name" label={i18n.tr('Arbeidsområdenavn', 'Workspace name')} value="coresystem-as" />
        <SettingsField id="workspace-url" label={i18n.tr('Arbeidsområde-URL', 'Workspace URL')} value="coresystem-as.verevon.ai" />
        <SettingsField id="primary-domain" label={i18n.tr('Primært domene', 'Primary domain')} value="coresystem.no" />
        <SettingsSelect
          id="data-region"
          label={i18n.tr('Dataregion', 'Data region')}
          value="europe"
          options={[
            { value: 'europe', label: i18n.tr('Europa', 'Europe') },
            { value: 'us', label: i18n.tr('USA', 'United States') },
          ]}
        />
        <SettingsSelect
          id="default-language"
          label={i18n.tr('Standardspråk', 'Default language')}
          value="english"
          options={[
            { value: 'english', label: i18n.tr('Engelsk', 'English') },
            { value: 'norwegian', label: i18n.tr('Norsk', 'Norwegian') },
            { value: 'french', label: i18n.tr('Fransk', 'French') },
          ]}
        />
        <SettingsField id="admin-owner" label={i18n.tr('Administratoreier', 'Admin owner')} value="Author Name" />
      </div>
      <div class="verevon-settings-feature-grid">
        <FeaturePanel
          title={i18n.tr('Verifiserte domener', 'Verified domains')}
          description={i18n.tr(
            'Domeneoppføringer klare for DNS-verifisering og kundevendte lenker.',
            'Domain records ready for DNS verification and customer-facing links.',
          )}
          actionLabel={i18n.tr('Legg til domene', 'Add domain')}
        >
          <p class="verevon-settings-panel-note">{i18n.tr('Ingen domener er verifisert for denne organisasjonen.', 'No domains have been verified for this organization.')}</p>
        </FeaturePanel>
        <FeaturePanel
          title={i18n.tr('Åpningstider', 'Business hours')}
          description={i18n.tr(
            'Arbeidsområdeomfattende rutingvinduer for eierskap av support-innboks.',
            'Workspace-wide routing windows for support inbox ownership.',
          )}
          actionLabel={i18n.tr('Rediger tidsplan', 'Edit schedule')}
        >
          <div class="verevon-settings-row-divider">
            <For each={getBusinessHourRows(i18n)}>
              {(row) => <DataRow primary={row.day} secondary={row.inbox} meta={row.hours} />}
            </For>
          </div>
        </FeaturePanel>
      </div>
      <OrgDeletionDangerZone />
    </>
  )
}

function MembersSection(props: { orgId: string | null }) {
  const i18n = useI18n()
  const session = getSession()
  const [members, setMembers] = createSignal<LiveMember[]>([])
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [actionBusy, setActionBusy] = createSignal(false)
  const [inviteEmail, setInviteEmail] = createSignal('')
  const [inviteRole, setInviteRole] = createSignal<MembershipRole>('member')
  const [confirmRemoveId, setConfirmRemoveId] = createSignal<string | null>(null)

  const loadMembers = async (orgId: string, signal?: AbortSignal) => {
    const data = await requestJson<MemberListResponse | LiveMember[]>(
      `/api/v1/orgs/${encodeURIComponent(orgId)}/members`,
      { signal },
    )
    setMembers(normalizeMemberList(data))
  }

  createEffect(() => {
    const orgId = props.orgId
    setMembers([])
    setError(null)

    if (!orgId) {
      setLoading(false)
      return
    }

    setLoading(true)
    const controller = new AbortController()

    loadMembers(orgId, controller.signal)
      .then(() => {
        setLoading(false)
      })
      .catch((reason: unknown) => {
        if (reason instanceof Error && reason.name === 'AbortError') return
        setError(i18n.tr('Kunne ikke laste medlemmer.', 'Could not load members.'))
        setLoading(false)
      })

    onCleanup(() => controller.abort())
  })

  const invite = async () => {
    const orgId = props.orgId
    const email = inviteEmail().trim().toLowerCase()
    if (!orgId || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email) || actionBusy()) {
      setActionError(i18n.tr('Skriv inn en gyldig e-postadresse før du inviterer et medlem.', 'Enter a valid email address before inviting a member.'))
      return
    }
    setActionBusy(true)
    setActionError(null)
    try {
      await inviteMember(orgId, email, inviteRole())
      setInviteEmail('')
      await loadMembers(orgId)
    } catch {
      // inviteMember can fail (e.g. a transient 502) after the invite was
      // already durably recorded server-side. Re-fetch and check whether the
      // invited email is now listed (any status — including a pending
      // invite) before asserting failure, instead of trusting the network
      // error alone — otherwise an admin sees a false "could not be sent"
      // error and may resend a duplicate invite for one that already went
      // out.
      let reconciled = true
      try {
        await loadMembers(orgId)
      } catch {
        reconciled = false
      }
      if (!reconciled) {
        setActionError(i18n.tr(
          'Vi fikk ikke bekreftet om invitasjonen ble sendt. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the invitation was sent. Please wait a moment before trying again.",
        ))
      } else if (members().some((member) => member.email.toLowerCase() === email)) {
        setInviteEmail('')
      } else {
        setActionError(i18n.tr('Invitasjonen kunne ikke sendes. Kontroller administratortilgangen din og prøv igjen.', 'The invitation could not be sent. Verify your admin access and try again.'))
      }
    } finally {
      setActionBusy(false)
    }
  }

  const changeRole = async (member: LiveMember, role: MembershipRole) => {
    const orgId = props.orgId
    if (!orgId || actionBusy() || member.role === role) return
    setActionBusy(true)
    setActionError(null)
    try {
      await updateMemberRole(orgId, member.userId, role)
      await loadMembers(orgId)
    } catch {
      setActionError(i18n.tr('Medlemsrollen kunne ikke endres. Eierens rolle er beskyttet og forblir uendret.', 'The member role could not be changed. The owner invariant is preserved.'))
    } finally {
      setActionBusy(false)
    }
  }

  const remove = async (member: LiveMember) => {
    const orgId = props.orgId
    if (!orgId || actionBusy()) return
    if (confirmRemoveId() !== member.userId) {
      setConfirmRemoveId(member.userId)
      return
    }
    setActionBusy(true)
    setActionError(null)
    try {
      await removeMember(orgId, member.userId)
      setConfirmRemoveId(null)
      await loadMembers(orgId)
    } catch {
      // removeMember can fail (e.g. a transient 502) after the member was
      // already durably removed server-side. Re-fetch and check whether
      // they're still listed before asserting failure, instead of trusting
      // the network error alone.
      let reconciled = true
      try {
        await loadMembers(orgId)
      } catch {
        reconciled = false
      }
      if (!reconciled) {
        setActionError(i18n.tr(
          'Vi fikk ikke bekreftet om medlemmet ble fjernet. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the member was removed. Please wait a moment before trying again.",
        ))
      } else if (members().some((item) => item.userId === member.userId)) {
        setActionError(i18n.tr('Medlemmet kunne ikke fjernes. Auth Core beholdt organisasjonseierens rolle intakt.', 'The member could not be removed. Auth Core kept the organization owner invariant intact.'))
      } else {
        setConfirmRemoveId(null)
      }
    } finally {
      setActionBusy(false)
    }
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Medlemmer og roller', 'Members & roles')}
        description={i18n.tr('Inviter kollegaer, tildel tilgang og se status på seter.', 'Invite teammates, assign access, and review seat status.')}
      />
      <div class="verevon-settings-invite-grid">
        <SettingsField
          id="invite-email"
          label={i18n.tr('Inviter via e-post', 'Invite by email')}
          type="email"
          placeholder="teammate@company.com"
          value={inviteEmail()}
          onInput={(event) => setInviteEmail(event.currentTarget.value)}
        />
        <SettingsSelect
          id="invite-role"
          label={i18n.tr('Rolle', 'Role')}
          value={inviteRole()}
          onChange={(event) => setInviteRole(event.currentTarget.value as MembershipRole)}
          options={[
            { value: 'member', label: i18n.tr('Medlem', 'Member') },
            { value: 'admin', label: i18n.tr('Administrator', 'Admin') },
          ]}
        />
        <SettingsButton
          variant="primary"
          disabled={actionBusy() || !props.orgId}
          onClick={() => void invite()}
        >
          {i18n.tr('Inviter medlem', 'Invite member')}
        </SettingsButton>
      </div>
      <Show when={actionError()}>
        <p role="alert" class="verevon-settings-empty-row">{actionError()}</p>
      </Show>
      <div class="verevon-settings-list-card">
        <Show when={!loading()} fallback={<p class="verevon-settings-empty-row">{i18n.tr('Laster medlemmer …', 'Loading members...')}</p>}>
          <Show when={!error()} fallback={<p class="verevon-settings-empty-row">{error()}</p>}>
            <Show when={members().length > 0} fallback={<p class="verevon-settings-empty-row">{i18n.tr('Ingen medlemmer funnet.', 'No members found.')}</p>}>
              <For each={members()}>
                {(member) => (
                  <div class="verevon-settings-member-row">
                    <div>
                      <p>{member.name ?? member.email}</p>
                      <span>{member.email}</span>
                    </div>
                    <select
                      aria-label={`${i18n.tr('Rolle for', 'Role for')} ${member.name ?? member.email}`}
                      value={member.role}
                      disabled={actionBusy() || member.role === 'owner'}
                      onChange={(event) => void changeRole(member, event.currentTarget.value as MembershipRole)}
                    >
                      <Show when={member.role === 'owner'}>
                        <option value="owner">{i18n.tr('Eier', 'Owner')}</option>
                      </Show>
                      <option value="member">{i18n.tr('Medlem', 'Member')}</option>
                      <option value="admin">{i18n.tr('Administrator', 'Admin')}</option>
                    </select>
                    <span>{member.status}</span>
                    <button
                      type="button"
                      disabled={
                        actionBusy()
                        || member.role === 'owner'
                        || member.userId === session.user?.id
                      }
                      aria-label={
                        confirmRemoveId() === member.userId
                          ? `${i18n.tr('Bekreft fjerning av', 'Confirm remove')} ${member.name ?? member.email}`
                          : `${i18n.tr('Fjern', 'Remove')} ${member.name ?? member.email}`
                      }
                      class="verevon-settings-icon-button"
                      onClick={() => void remove(member)}
                    >
                      {confirmRemoveId() === member.userId ? i18n.tr('Bekreft', 'Confirm') : i18n.tr('Fjern', 'Remove')}
                    </button>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </Show>
      </div>
      <div class="verevon-settings-feature-panel verevon-settings-feature-panel--spaced">
        <div class="verevon-settings-feature-panel__header">
          <div>
            <h3>{i18n.tr('Innebygde roller', 'Built-in roles')}</h3>
            <p>
              {i18n.tr(
                'Auth Core støtter for øyeblikket eier, administrator og medlem. Overføring av eierskap og administrasjon av egendefinerte roller krever egne, eksplisitte avtaler.',
                'Auth Core currently supports owner, admin, and member. Owner transfer and custom role administration require separate explicit contracts.',
              )}
            </p>
          </div>
        </div>
      </div>
    </>
  )
}

type PlatformUser = {
  id: string
  email: string
  name: string
  role: string
  banned: boolean
  emailVerified: boolean
}

type ListUsersResponse = { users?: unknown[]; total?: number }

function normalizePlatformUsers(data: ListUsersResponse | unknown[]): PlatformUser[] {
  const rows = Array.isArray(data) ? data : Array.isArray(data?.users) ? data.users : []
  return rows.map((raw) => {
    const r = recordFrom(raw)
    return {
      id: stringValue(r, 'id'),
      email: stringValue(r, 'email'),
      name: stringValue(r, 'name'),
      role: stringValue(r, 'role') || 'user',
      banned: r.banned === true,
      emailVerified: r.emailVerified === true || r.email_verified === true,
    }
  }).filter((u) => u.id !== '')
}

/**
 * Cross-org user directory for platform super-admins. Calls the gateway's
 * `/api/v1/admin/users` (which proxies Better Auth admin list-users — NOT
 * org-scoped), so this lists every user in the deployment regardless of org.
 * The gateway + auth-core both gate on the top-level admin/superadmin role;
 * a non-admin who reaches this section sees the 403 message instead of data.
 */
function PlatformUsersSection() {
  const i18n = useI18n()
  const [users, setUsers] = createSignal<PlatformUser[]>([])
  const [loading, setLoading] = createSignal(true)
  const [error, setError] = createSignal<string | null>(null)

  createEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    setError(null)
    requestJson<ListUsersResponse | PlatformUser[]>(
      '/api/v1/admin/users?limit=500',
      { signal: controller.signal },
    )
      .then((data) => {
        setUsers(normalizePlatformUsers(data))
        setLoading(false)
      })
      .catch((reason: unknown) => {
        if (reason instanceof Error && reason.name === 'AbortError') return
        setError(i18n.tr('Kunne ikke laste brukere. Denne visningen krever plattform-superadministrator.', 'Could not load users. This view requires a platform super-admin.'))
        setLoading(false)
      })
    onCleanup(() => controller.abort())
  })

  return (
    <>
      <SectionHeader
        title={i18n.tr('Alle brukere', 'All users')}
        description={i18n.tr('Alle brukere på tvers av alle organisasjoner. Kun for plattform-superadministrator.', 'Every user across all organizations. Platform super-admin only.')}
      />
      <div class="verevon-settings-list-card">
        <Show when={!loading()} fallback={<p class="verevon-settings-empty-row">{i18n.tr('Laster brukere …', 'Loading users...')}</p>}>
          <Show when={!error()} fallback={<p class="verevon-settings-empty-row">{error()}</p>}>
            <Show when={users().length > 0} fallback={<p class="verevon-settings-empty-row">{i18n.tr('Ingen brukere funnet.', 'No users found.')}</p>}>
              <For each={users()}>
                {(user) => (
                  <div class="verevon-settings-member-row">
                    <div>
                      <p>{user.name || user.email}</p>
                      <span>{user.email}</span>
                    </div>
                    <strong>{user.role}</strong>
                    <span>{user.banned ? i18n.tr('utestengt', 'banned') : user.emailVerified ? i18n.tr('verifisert', 'verified') : i18n.tr('ikke verifisert', 'unverified')}</span>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </Show>
      </div>
    </>
  )
}

function formatSubscriptionStatus(status?: string | null): string {
  if (!status) return '—'
  return status.replaceAll('_', ' ')
}

function quotaValue(account: BillingAccount | null, metric: string): number | null {
  const value = account?.quota_limits?.[metric]
  return typeof value === 'number' ? value : null
}

function quotaDisplay(value: number | null, i18n: ReturnType<typeof useI18n>): string {
  if (value == null) return '—'
  if (value < 0) return i18n.tr('Ubegrenset', 'Unlimited')
  return String(value)
}

function settingsCheckoutUrl(kind: 'cancel' | 'success', plan: BillingPlanId): string {
  if (typeof window === 'undefined') return `/settings/billing?checkout=${kind}&plan=${plan}`
  const url = new URL('/settings/billing', window.location.origin)
  url.searchParams.set('checkout', kind)
  url.searchParams.set('plan', plan)
  return url.toString()
}

function clearCheckoutParams() {
  if (typeof window === 'undefined') return
  const url = new URL(window.location.href)
  for (const key of ['checkout', 'payment_id', 'payment_intent_client_secret', 'status', 'plan']) {
    url.searchParams.delete(key)
  }
  window.history.replaceState(null, '', `${url.pathname}${url.search}${url.hash}`)
}

function BillingSection(props: {
  account: BillingAccount | null
  error: string | null
  loading: boolean
  onRefresh: () => Promise<void>
}) {
  const i18n = useI18n()
  const [selectedPlan, setSelectedPlan] = createSignal<BillingPlanId>('standard')
  const [checkoutSession, setCheckoutSession] = createSignal<CheckoutSession>()
  const [startingCheckout, setStartingCheckout] = createSignal(false)
  const [confirmingCheckout, setConfirmingCheckout] = createSignal(false)
  const [message, setMessage] = createSignal<string | null>(null)
  const [checkoutError, setCheckoutError] = createSignal<string | null>(null)

  const currentPaidPlan = () => paidBillingPlan(props.account?.plan)
  const plan = () => props.account ? planLabel(props.account.plan) : '—'
  const status = () => formatSubscriptionStatus(props.account?.subscription_state)
  const credits = () => props.account?.credits != null ? String(props.account.credits) : '—'
  const seatLimit = () => quotaValue(props.account, 'users')
  const apiCallLimit = () => quotaValue(props.account, 'api_calls')
  const storageLimit = () => quotaValue(props.account, 'storage_mb')

  createEffect(() => {
    const current = currentPaidPlan()
    if (current) setSelectedPlan(current)
  })

  onMount(() => {
    if (typeof window === 'undefined') return

    const params = new URLSearchParams(window.location.search)
    const checkoutState = params.get('checkout')
    const planParam = paidBillingPlan(params.get('plan')) ?? selectedPlan()

    if (checkoutState === 'cancel') {
      setSelectedPlan(planParam)
      setMessage(i18n.tr('Betalingen ble avbrutt.', 'Payment was cancelled.'))
      clearCheckoutParams()
      return
    }

    if (checkoutState !== 'success') return

    const paymentId = params.get('payment_id') || undefined
    const clientSecret = params.get('payment_intent_client_secret') || undefined
    const providerStatus = params.get('status') || 'processing'
    setSelectedPlan(planParam)

    if (!paymentId && !clientSecret) {
      setCheckoutError(i18n.tr('Betalingsreferanse mangler. Start kassen på nytt.', 'Payment reference is missing. Start checkout again.'))
      clearCheckoutParams()
      return
    }

    void finalizeCheckout({
      paymentId,
      clientSecret,
      status: providerStatus,
      plan: planParam,
    })
  })

  async function startPlanCheckout(planId: BillingPlanId) {
    const planOption = billingPlans.find((item) => item.id === planId)
    if (!planOption?.checkoutEnabled) {
      setMessage(i18n.tr('Egendefinerte planendringer håndteres av salgsavdelingen.', 'Custom plan changes are handled by sales.'))
      return
    }

    setSelectedPlan(planId)
    setMessage(null)
    setCheckoutError(null)
    setCheckoutSession(undefined)
    setStartingCheckout(true)

    try {
      const session = await startBillingCheckout({
        plan: planId,
        successUrl: settingsCheckoutUrl('success', planId),
        cancelUrl: settingsCheckoutUrl('cancel', planId),
      })

      const checkoutSurface = resolveCheckoutSurface(session)
      switch (checkoutSurface) {
        case 'nexi-embedded':
        case 'hyperswitch-embedded':
          setCheckoutSession(session)
          return
        case 'redirect':
          window.location.assign(session.url!)
          return
        default:
          throw new Error(i18n.tr('Kasseøkten inneholdt ikke en gyldig betalingsflate.', 'Checkout session did not include a valid payment surface.'))
      }
    } catch (reason) {
      setCheckoutError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke starte kassen.', 'Could not start checkout.'))
    } finally {
      setStartingCheckout(false)
    }
  }

  async function finalizeCheckout(payment: {
    clientSecret?: string
    paymentId?: string
    plan?: BillingPlanId
    status: string
  }) {
    const planId = payment.plan ?? selectedPlan()
    setConfirmingCheckout(true)
    setMessage(null)
    setCheckoutError(null)

    try {
      const result = await confirmBillingCheckout({
        plan: planId,
        paymentId: payment.paymentId,
        clientSecret: payment.clientSecret,
      })

      if (!isCheckoutActivatingStatus(result.status)) {
        throw new Error(`${i18n.tr('Betalingsstatus er', 'Payment status is')} ${result.status}.`)
      }

      setCheckoutSession(undefined)
      setMessage(result.status === 'processing'
        ? i18n.tr('Betalingen behandles. Planen din forblir aktiv mens bekreftelsen fullføres.', 'Payment is processing. Your plan will stay active while confirmation completes.')
        : i18n.tr('Betaling bekreftet. Faktureringsplanen din er aktiv.', 'Payment confirmed. Your billing plan is active.'))
      await props.onRefresh()
      clearCheckoutParams()
    } catch (reason) {
      setCheckoutError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke bekrefte kassen.', 'Could not confirm checkout.'))
    } finally {
      setConfirmingCheckout(false)
    }
  }

  return (
    <>
      <SectionHeader title={i18n.tr('Plan og bruk', 'Plan & usage')} description={i18n.tr('Se gjennom plan, bruk, betalingsmetode og fakturaer.', 'Review plan, usage, payment method, and invoices.')} />
      <Show when={props.error}>
        {(error) => <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">{error()}</p>}
      </Show>
      <div class="verevon-settings-metric-grid">
        <Metric label={i18n.tr('Plan', 'Plan')} value={plan()} detail={`${i18n.tr('Status', 'Status')}: ${status()}`} />
        <Metric
          label={i18n.tr('Seter', 'Seats')}
          value={quotaDisplay(seatLimit(), i18n)}
          detail={seatLimit() != null ? i18n.tr('Seter inkludert i denne planen', 'Seats included on this plan') : i18n.tr('Setekvote utilgjengelig', 'Seat quota unavailable')}
        />
        <Metric label={i18n.tr('Kreditter', 'Credits')} value={credits()} detail={i18n.tr('Tilgjengelig på denne planen', 'Available on this plan')} />
      </div>
      <section class="verevon-settings-plan-picker" aria-labelledby="settings-billing-plan-heading">
        <div class="verevon-settings-section-header verevon-settings-section-header--compact">
          <h2 id="settings-billing-plan-heading">{i18n.tr('Velg plan', 'Choose plan')}</h2>
          <p>{i18n.tr('Endre arbeidsområdets plan gjennom billing-core-kassen. Betalte planer åpner den konfigurerte, sikre betalingsleverandøren.', 'Change the workspace plan through billing-core checkout. Paid plans open the configured secure payment provider.')}</p>
        </div>
        <div class="verevon-settings-plan-list">
          <For each={billingPlans}>
            {(billingPlan) => {
              const active = () => currentPaidPlan() === billingPlan.id
              const selected = () => selectedPlan() === billingPlan.id
              return (
                <article
                  class="verevon-settings-plan-row"
                  classList={{
                    'verevon-settings-plan-row--active': active(),
                    'verevon-settings-plan-row--selected': selected(),
                  }}
                >
                  <div class="verevon-settings-plan-row__copy">
                    <div>
                      <h3>{billingPlan.name}</h3>
                      <span>{billingPlan.priceLabel}</span>
                    </div>
                    <p>{billingPlan.description}</p>
                    <ul>
                      <For each={billingPlan.features}>{(feature) => <li>{feature}</li>}</For>
                    </ul>
                  </div>
                  <div class="verevon-settings-plan-row__actions">
                    <Show when={active()}>
                      <span class="verevon-settings-plan-pill">{i18n.tr('Gjeldende plan', 'Current plan')}</span>
                    </Show>
                    <SettingsButton
                      settingsSize="sm"
                      variant={selected() ? 'primary' : 'secondary'}
                      disabled={props.loading || startingCheckout() || confirmingCheckout() || active()}
                      onClick={() => void startPlanCheckout(billingPlan.id)}
                    >
                      {active()
                        ? i18n.tr('Aktiv', 'Active')
                        : billingPlan.checkoutEnabled
                          ? `${startingCheckout() && selected() ? i18n.tr('Starter', 'Starting') : i18n.tr('Aktiver', 'Activate')} ${billingPlan.name}`
                          : i18n.tr('Kontakt salg', 'Contact sales')}
                    </SettingsButton>
                  </div>
                </article>
              )
            }}
          </For>
        </div>
      </section>
      <Switch>
        <Match
          when={
            checkoutSession()?.provider === 'nexi' &&
            (checkoutSession()?.payment_id || checkoutSession()?.id) &&
            checkoutSession()?.publishable_key &&
            checkoutSession()?.client_url
              ? checkoutSession()
              : undefined
          }
        >
          {(session) => (
            <NexiCheckout
              session={session()}
              confirming={confirmingCheckout()}
              returnUrl={settingsCheckoutUrl('success', selectedPlan())}
              onConfirmed={(payment) => finalizeCheckout({ ...payment, plan: selectedPlan() })}
            />
          )}
        </Match>
        <Match
          when={
            checkoutSession()?.provider === 'hyperswitch' && checkoutSession()?.client_secret
              ? checkoutSession()
              : undefined
          }
        >
          {(session) => (
            <HyperswitchCheckout
              session={session()}
              confirming={confirmingCheckout()}
              returnUrl={settingsCheckoutUrl('success', selectedPlan())}
              onConfirmed={(payment) => finalizeCheckout({ ...payment, plan: selectedPlan() })}
            />
          )}
        </Match>
      </Switch>
      <Show when={message()}>
        {(text) => <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">{text()}</p>}
      </Show>
      <Show when={checkoutError()}>
        {(text) => <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">{text()}</p>}
      </Show>
      <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
        <SettingsField id="billing-email" label={i18n.tr('Faktureringsepost', 'Billing email')} type="email" placeholder="billing@yourcompany.com" />
        <SettingsSelect
          id="usage-cap"
          label={i18n.tr('Bruksgrense', 'Usage cap')}
          value="notify"
          options={[
            { value: 'notify', label: i18n.tr('Varsle ved 80 %', 'Notify at 80%') },
            { value: 'pause', label: i18n.tr('Sett på pause ved grense', 'Pause at limit') },
            { value: 'none', label: i18n.tr('Ingen grense', 'No cap') },
          ]}
        />
      </div>
      <div class="verevon-settings-billing-grid">
        <FeaturePanel
          title={i18n.tr('Fakturahistorikk', 'Invoice history')}
          description={i18n.tr('Arbeidsområdets fakturahistorikk og nedlastbare faktureringsposter.', 'Workspace invoice history and downloadable billing records.')}
          actionLabel={i18n.tr('Last ned CSV', 'Download CSV')}
        >
          <p class="verevon-settings-panel-note">{i18n.tr('Ingen fakturaer ennå - de vises her når en betalt plan er aktiv.', 'No invoices yet - invoices will appear here once a paid plan is active.')}</p>
        </FeaturePanel>
        <FeaturePanel
          title={i18n.tr('Forbrukskontroller', 'Spend controls')}
          description={i18n.tr('Plangrenser og kvotedetaljer for dette arbeidsområdet.', 'Plan limits and quota details for this workspace.')}
          actionLabel={i18n.tr('Konfigurer', 'Configure')}
        >
          <Show
            when={props.account != null}
            fallback={<p class="verevon-settings-panel-note">{i18n.tr('Laster plandetaljer …', 'Loading plan details...')}</p>}
          >
            <div class="verevon-settings-key-values">
              <p><span>{i18n.tr('Plan', 'Plan')}</span><strong>{plan()}</strong></p>
              <p><span>{i18n.tr('Status', 'Status')}</span><strong>{status()}</strong></p>
              <p><span>{i18n.tr('Kreditter', 'Credits')}</span><strong>{credits()}</strong></p>
              <p><span>{i18n.tr('API-kall', 'API calls')}</span><strong>{quotaDisplay(apiCallLimit(), i18n)}</strong></p>
              <p><span>{i18n.tr('Lagring', 'Storage')}</span><strong>{quotaDisplay(storageLimit(), i18n)}</strong></p>
            </div>
          </Show>
        </FeaturePanel>
      </div>
    </>
  )
}

function SsoSection() {
  const i18n = useI18n()
  return (
    <>
      <SectionHeader
        title={i18n.tr('SSO-konfigurasjon', 'SSO configuration')}
        description={i18n.tr('Konfigurer organisasjonens innlogging, domener og provisjonering.', 'Configure organization sign-in, domains, and provisioning.')}
      />
      <p class="verevon-settings-panel-note">
        {i18n.tr(
          'Enkel pålogging (SSO) er ikke konfigurert for denne organisasjonen. Ingen identitetsleverandør, godkjent domene, SCIM-token eller attributt-mapping er satt opp, og SSO/SCIM-konfigurasjon er ikke tilgjengelig i denne versjonen.',
          'Single sign-on is not configured for this organization. No identity provider, allowed domain, SCIM token, or attribute mapping is set up, and SSO/SCIM configuration is not available in this build.',
        )}
      </p>
    </>
  )
}

function RecentSecurityEvents() {
  const i18n = useI18n()
  const [events, setEvents] = createSignal<AuditEvent[]>([])
  const [loading, setLoading] = createSignal(true)
  const [loadFailed, setLoadFailed] = createSignal(false)

  onMount(() => {
    const controller = new AbortController()

    listAuditEvents({ limit: 25 }, controller.signal)
      .then((rows) => {
        setEvents(rows)
        setLoading(false)
      })
      .catch((reason: unknown) => {
        if (reason instanceof Error && reason.name === 'AbortError') return
        setEvents([])
        setLoadFailed(true)
        setLoading(false)
      })

    onCleanup(() => controller.abort())
  })

  return (
    <Show when={!loading()} fallback={<p class="verevon-settings-panel-note">{i18n.tr('Laster sikkerhetshendelser …', 'Loading security events...')}</p>}>
      <Show when={!loadFailed()} fallback={<p class="verevon-settings-panel-note" role="alert">{i18n.tr('Kunne ikke laste sikkerhetshendelser.', 'Could not load security events.')}</p>}>
      <Show when={events().length > 0} fallback={<p class="verevon-settings-panel-note">{i18n.tr('Ingen sikkerhetshendelser ennå', 'No security events yet')}</p>}>
        <div class="verevon-settings-row-divider">
          <For each={events()}>
            {(event, index) => {
              const dateStr = () => {
                if (!event.occurredAt || Number.isNaN(new Date(event.occurredAt).getTime())) return ''
                return new Date(event.occurredAt).toLocaleString()
              }
              const secondary = () => [
                event.actor || event.userId,
                event.actorRole,
                event.ipAddress,
                event.requestId,
              ].filter(Boolean).join(' · ')
              return (
                <DataRow
                  primary={`${event.event ?? i18n.tr('Hendelse', 'Event')}${event.outcome ? ` - ${event.outcome}` : ''}`}
                  secondary={secondary()}
                  meta={dateStr() || event.requestId || String(index())}
                />
              )
            }}
          </For>
        </div>
      </Show>
      </Show>
    </Show>
  )
}

function zdrTooltip(i18n: ReturnType<typeof useI18n>): string {
  return i18n.tr(
    'Zero Data Retention (ZDR): når dette er på, blir ikke interaktivt AI-innhold lagret, og det forlater ikke tjenesten. Slå det på for maksimalt personvern (null lagring). ZDR er et frivillig, betalt tillegg tilgjengelig på kvalifiserende (Pro/Enterprise) planer — av som standard til organisasjonen din aktiverer det.',
    'Zero Data Retention (ZDR): when on, interactive AI content is not retained and does not leave the service. Turn it on for maximum privacy (zero retention). ZDR is an opt-in, paid add-on available on qualifying (Pro/Enterprise) plans — off by default until your organization enables it.',
  )
}

function OrgSecuritySection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = () => session.activeOrg?.id ?? null
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const [zdr, setZdr] = createSignal(false)
  const [zdrEntitled, setZdrEntitled] = createSignal(false)
  const [zdrLoaded, setZdrLoaded] = createSignal(false)
  const [zdrBusy, setZdrBusy] = createSignal(false)
  const [zdrError, setZdrError] = createSignal<string | null>(null)
  const [supportAiMode, setSupportAiMode] = createSignal<SupportAIMode>('review')
  const [supportAiLoaded, setSupportAiLoaded] = createSignal(false)
  const [supportAiBusy, setSupportAiBusy] = createSignal(false)
  const [supportAiError, setSupportAiError] = createSignal<string | null>(null)

  onMount(async () => {
    const id = orgId()
    if (!id) {
      setZdrLoaded(true)
      setSupportAiLoaded(true)
      return
    }
    try {
      const [posture, entitled, mode] = await Promise.all([
        getOrganizationZdr(id),
        getOrganizationZdrEntitled(id).catch(() => false),
        getOrganizationSupportAIMode(id),
      ])
      setZdr(posture)
      setZdrEntitled(entitled)
      setSupportAiMode(mode)
    } catch {
      // Fall back to the product default (off) on read failure; live retention
      // enforcement stays fail-closed server-side regardless of this toggle.
    } finally {
      setZdrLoaded(true)
      setSupportAiLoaded(true)
    }
  })

  async function toggleZdr(next: boolean) {
    const id = orgId()
    if (!id || zdrBusy()) return
    const previous = zdr()
    setZdr(next) // optimistic
    setZdrBusy(true)
    setZdrError(null)
    try {
      setZdr(await updateOrganizationZdr(id, next))
    } catch (reason) {
      setZdr(previous) // revert on failure
      if (reason instanceof ApiError && reason.code === 'plan_upgrade_required') {
        setZdrError(i18n.tr('Zero Data Retention er tilgjengelig på en høyere plan. Oppgrader for å aktivere det.', 'Zero Data Retention is available on a higher plan. Upgrade to enable it.'))
      } else {
        setZdrError(
          reason instanceof Error ? reason.message : i18n.tr('Kunne ikke oppdatere Zero Data Retention.', 'Could not update Zero Data Retention.'),
        )
      }
    } finally {
      setZdrBusy(false)
    }
  }

  async function changeSupportAiMode(next: SupportAIMode) {
    const id = orgId()
    if (!id || supportAiBusy() || next === supportAiMode()) return
    const previous = supportAiMode()
    setSupportAiMode(next)
    setSupportAiBusy(true)
    setSupportAiError(null)
    try {
      setSupportAiMode(await updateOrganizationSupportAIMode(id, next))
    } catch (reason) {
      setSupportAiMode(previous)
      setSupportAiError(
        reason instanceof Error ? reason.message : i18n.tr('Kunne ikke oppdatere Support AI-modus.', 'Could not update the Support AI mode.'),
      )
    } finally {
      setSupportAiBusy(false)
    }
  }

  // Enabling ZDR is plan-gated; disabling is always allowed. Block the control
  // only when the org can't enable it and it is currently off.
  const zdrLocked = createMemo(() => !zdrEntitled() && !zdr())

  return (
    <>
      <SectionHeader
        title={i18n.tr('Sikkerhetspolicy', 'Security policy')}
        description={i18n.tr('Angi organisasjonsomfattende sikkerhetskrav og revisjonskontroller.', 'Set organization-wide security requirements and audit controls.')}
      />
      <div class="verevon-settings-divided-list">
        <ToggleRow
          title={zdrLocked() ? i18n.tr('Zero Data Retention (premiumfunksjon)', 'Zero Data Retention (premium)') : i18n.tr('Zero Data Retention', 'Zero Data Retention')}
          description={i18n.tr(
            'Når dette er på, blir ikke interaktivt AI-innhold lagret. Slå av for å la Verevon lagre samtalehistorikk og drive minnefunksjoner.',
            'When on, interactive AI content is not retained. Turn off to let Verevon store conversation history and power memory.',
          )}
          info={zdrTooltip(i18n)}
          enabled={zdr()}
          disabled={!isAdmin() || zdrBusy() || !zdrLoaded() || !orgId() || zdrLocked()}
          onChange={(value) => void toggleZdr(value)}
        />
        <div class="verevon-settings-toggle-row">
          <div>
            <p class="verevon-settings-toggle-title">{i18n.tr('Support AI-modus', 'Support AI mode')}</p>
            <span>{i18n.tr(
              'Av blokkerer modellkall fra Inbox. Assistent gir bare midlertidig hjelp. Gjennomgang lar Verevon lagre begrensede forslag som en operatør må godkjenne eller avvise.',
              'Off blocks Inbox model calls. Assist provides transient help only. Review lets Verevon retain bounded proposals that an operator must approve or reject.',
            )}</span>
          </div>
          <SettingsSelect
            id="support-ai-mode"
            label={i18n.tr('Support AI-modus', 'Support AI mode')}
            value={supportAiMode()}
            disabled={!isAdmin() || supportAiBusy() || !supportAiLoaded() || !orgId()}
            onChange={(event) => void changeSupportAiMode(event.currentTarget.value as SupportAIMode)}
            options={[
              { value: 'off', label: i18n.tr('Av', 'Off') },
              { value: 'assist', label: i18n.tr('Assistent', 'Assist') },
              { value: 'review', label: i18n.tr('Gjennomgang', 'Review') },
            ]}
          />
        </div>
        <For each={getSecurityControls(i18n)}>
          {(control) => (
            <ToggleRow title={control.title} description={control.description} enabled={false} disabled />
          )}
        </For>
      </div>
      <Show when={zdrError()}>
        <p class="verevon-settings-panel-note" role="alert">{zdrError()}</p>
      </Show>
      <Show when={supportAiError()}>
        <p class="verevon-settings-panel-note" role="alert">{supportAiError()}</p>
      </Show>
      <Show when={zdrLoaded() && zdrLocked()}>
        <p class="verevon-settings-panel-note">
          {i18n.tr(
            'Zero Data Retention er en personvernfunksjon for premiumplaner. Oppgrader planen din for å aktivere nullbevaringsbehandling.',
            'Zero Data Retention is a premium privacy feature. Upgrade your plan to enable zero-retention processing.',
          )}
        </p>
      </Show>
      <Show when={!isAdmin()}>
        <p class="verevon-settings-panel-note">{i18n.tr('Bare organisasjonseiere og administratorer kan endre Zero Data Retention eller Support AI-modus.', 'Only organization owners and admins can change Zero Data Retention or Support AI mode.')}</p>
      </Show>
      <p class="verevon-settings-panel-note">
        {i18n.tr(
          'MFA-, domenebegrensnings- og administratorrevisjonskontrollene under er ennå ikke koblet til en aktiv kilde for dette arbeidsområdet, så de vises som ikke konfigurert. De vil vise reell policytilstand så snart en org-sikkerhetsbackend er koblet til.',
          'The MFA, domain-restriction, and admin-audit controls below are not yet connected to a live source for this workspace, so they are shown unconfigured. They will reflect real policy state once an org-security backend is wired.',
        )}
      </p>
      <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
        <SettingsSelect
          id="session-duration"
          label={i18n.tr('Øktvarighet', 'Session duration')}
          value="30-days"
          options={[
            { value: '7-days', label: i18n.tr('7 dager', '7 days') },
            { value: '30-days', label: i18n.tr('30 dager', '30 days') },
            { value: '90-days', label: i18n.tr('90 dager', '90 days') },
          ]}
        />
      </div>
      <FeaturePanel
        title={i18n.tr('Nylige sikkerhetshendelser', 'Recent security events')}
        description={i18n.tr('Nylige organisasjonsendringer som administratorer bør gjennomgå.', 'Recent organization changes that administrators should review.')}
        actionLabel={i18n.tr('Åpne revisjonslogg', 'Open audit log')}
        class="verevon-settings-feature-panel--spaced"
      >
        <RecentSecurityEvents />
      </FeaturePanel>
    </>
  )
}

function IntegrationsSection() {
  const i18n = useI18n()
  const session = getSession()
  const [summary, setSummary] = createSignal<IntegrationSettingsSummary | null>(null)
  const [actionBusy, setActionBusy] = createSignal<string | null>(null)
  const [loadFailed, setLoadFailed] = createSignal(false)
  const [syncProgress, setSyncProgress] = createSignal<Record<string, string>>({})
  const [notice, setNotice] = createSignal<string | null>(null)
  const eventSources: EventSource[] = []
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  let refreshGeneration = 0

  const refresh = async (targetOrgId = orgId(), generation = refreshGeneration) => {
    try {
      const nextSummary = await loadIntegrationSettingsSummary(targetOrgId)
      if (generation !== refreshGeneration || targetOrgId !== orgId()) return
      setSummary(nextSummary)
      setLoadFailed(false)
    } catch {
      if (generation !== refreshGeneration || targetOrgId !== orgId()) return
      setLoadFailed(true)
    }
  }

  createEffect(() => {
    const targetOrgId = orgId()
    const generation = ++refreshGeneration

    for (const source of eventSources) source.close()
    eventSources.splice(0, eventSources.length)
    setSummary(null)
    setSyncProgress({})
    setNotice(null)
    setLoadFailed(false)

    void refresh(targetOrgId, generation)
  })

  onCleanup(() => {
    for (const source of eventSources) source.close()
    eventSources.splice(0, eventSources.length)
  })

  const rows = createMemo(() => buildIntegrationRows(summary(), i18n))
  const socialRows = createMemo(() => rows().filter(isSocialIntegrationRow))
  const socialStats = createMemo(() => buildSocialIntegrationStats(summary()))
  const instagramInboxConnected = createMemo(() => summary()?.connections.some((connection) =>
    connection.providerKey === 'instagram'
    && isConnectedIntegrationStatus(connection.status)
    && connection.capabilities.includes('social.inbox.read'),
  ) ?? false)

  const runAction = async (
    row: IntegrationSettingsRow,
    action: 'connect' | 'connect-account' | 'disconnect' | 'instagram-inbox' | 'reconnect' | 'sync',
  ) => {
    // Reserve the window before the first await. Browsers otherwise treat the
    // eventual popup as unsolicited and block the provider consent screen.
    const reservedOAuthWindow = action === 'disconnect' || action === 'sync'
      ? null
      : reserveDirectOauthWindow()
    const busyKey = `${row.provider.key}:${action}`
    setActionBusy(busyKey)
    setNotice(null)

    try {
      if (action === 'connect' || action === 'connect-account') {
        const session = await requestJson<ConnectSessionResult>(
          `/api/v1/integrations/providers/${encodeURIComponent(row.provider.key)}/connect-session`,
          {
            method: 'POST',
            body: JSON.stringify({ bundles: connectBundlesForProvider(row.provider.key) }),
            headers: integrationHeaders(orgId()),
          },
        )
        await runSettingsOAuth(session, reservedOAuthWindow)
        setNotice(action === 'connect-account'
          ? `${row.name} ${i18n.tr('ekstra konto tilkoblet.', 'additional account connected.')}`
          : `${row.name} ${i18n.tr('tilkoblet.', 'connected.')}`)
      } else if (action === 'instagram-inbox' && row.connection && row.provider.key === 'meta') {
        const instagram = instagramInboxConnectionRequest()
        const session = await requestJson<ConnectSessionResult>(
          `/api/v1/integrations/providers/${encodeURIComponent(instagram.providerKey)}/connect-session`,
          {
            method: 'POST',
            body: JSON.stringify({ bundles: instagram.bundles }),
            headers: integrationHeaders(orgId()),
          },
        )
        await runSettingsOAuth(session, reservedOAuthWindow)
        setNotice(i18n.tr(
          'Instagram-tilgang er godkjent. Velg Synkroniser for å kontrollere den koblede profesjonelle kontoen.',
          'Instagram consent is complete. Select Sync to check the linked professional account.',
        ))
      } else if (action === 'reconnect' && row.connection) {
        // Reconnect === re-run the provider connect-session with the FULL bundle.
        // There is no /connections/:id/reconnect-session route (gateway + v2
        // integration-core only expose /providers/:provider/{connect,reconnect}-
        // session), and the OAuth callback reuses the existing org+provider
        // connection (FindActiveConnection) and upgrades its scopes in place — so
        // this both fixes the prior 404 AND upgrades a minimal onboarding-created
        // connection (profile-only) to inbox/publishing scopes. Sending an empty
        // body previously resolved to the onboarding bundle, silently keeping the
        // connection at profile-only.
        const session = await requestJson<ConnectSessionResult>(
          `/api/v1/integrations/providers/${encodeURIComponent(row.provider.key)}/connect-session`,
          {
            method: 'POST',
            body: JSON.stringify({ bundles: connectBundlesForProvider(row.provider.key) }),
            headers: integrationHeaders(orgId()),
          },
        )
        await runSettingsOAuth(session, reservedOAuthWindow)
        setNotice(`${row.name} ${i18n.tr('koblet til på nytt.', 'reconnected.')}`)
      } else if (action === 'disconnect' && row.connection) {
        await requestJson<{ disconnected: boolean }>(
          `/api/v1/integrations/connections/${encodeURIComponent(row.connection.id)}`,
          { method: 'DELETE', headers: integrationHeaders(orgId()) },
        )
        setNotice(`${row.name} ${i18n.tr('koblet fra.', 'disconnected.')}`)
      } else if (action === 'sync' && row.connection) {
        const result = await triggerSync(orgId(), row.connection.id)
        const jobId = result.syncJob?.id
        setSyncProgress((prev) => ({
          ...prev,
          [row.connection!.id]: result.syncJob?.status ?? 'queued',
        }))
        if (jobId) watchSyncProgress(row.connection.id, jobId, setSyncProgress, eventSources, refresh)
      }
      await refresh()
    } catch (error) {
      if (reservedOAuthWindow && !reservedOAuthWindow.closed) reservedOAuthWindow.close()
      setNotice(error instanceof Error ? error.message : i18n.tr('Integrasjonshandlingen mislyktes.', 'Integration action failed.'))
    } finally {
      setActionBusy(null)
    }
  }

  return (
    <>
      <SectionHeader title={i18n.tr('Arbeidsområdeintegrasjoner', 'Workspace integrations')} description={i18n.tr('Koble til delte systemer som brukes av arbeidsområdet.', 'Connect shared systems used by the workspace.')} />
      <Show when={summary()}>
        {(current) => (
          <div class="verevon-settings-metric-grid">
            <Metric
              label={i18n.tr('Tilkoblet', 'Connected')}
              value={String(current().metrics.connected)}
              detail={`${current().metrics.readyProviders} ${i18n.tr('leverandører konfigurert', 'providers configured')}`}
            />
            <Metric
              label={i18n.tr('Synkroniserer', 'Syncing')}
              value={String(current().metrics.syncing)}
              detail={i18n.tr('Aktive synkroniseringsjobber synlige fra integration-corev2', 'Live sync jobs visible from integration-corev2')}
            />
            <Metric
              label={i18n.tr('Krever oppmerksomhet', 'Attention')}
              value={String(current().metrics.failed)}
              detail={i18n.tr('Kilder som trenger ny tilkobling eller gjennomgang', 'Sources needing reconnect or review')}
            />
          </div>
        )}
      </Show>
      <section class="verevon-settings-social-layer" aria-labelledby="verevon-settings-social-title">
        <div class="verevon-settings-social-layer__header">
          <div>
            <span>{i18n.tr('Sosialt lag', 'Social layer')}</span>
            <h3 id="verevon-settings-social-title">{i18n.tr('Publisering, innboks og kampanjeadaptere', 'Publishing, inbox, and campaign adapters')}</h3>
            <p>
              {i18n.tr(
                'integration-core eier OAuth og token-leier; social-core bruker avgrensede kapabiliteter for leverandørspesifikke arbeidsflyter.',
                'integration-core owns OAuth and token leases; social-core consumes scoped capabilities for provider-specific workflows.',
              )}
            </p>
          </div>
          <a class="verevon-settings-button verevon-settings-button--sm" href="/social/calendar">{i18n.tr('Åpne kalender', 'Open calendar')}</a>
        </div>
        <div class="verevon-settings-social-summary">
          <div>
            <strong>{socialStats().providers}</strong>
            <span>{i18n.tr('sosiale leverandører', 'social providers')}</span>
          </div>
          <div>
            <strong>{socialStats().connected}</strong>
            <span>{i18n.tr('tilkoblet', 'connected')}</span>
          </div>
          <div>
            <strong>{socialStats().publishingReady}</strong>
            <span>{i18n.tr('klar for publisering', 'publish-ready')}</span>
          </div>
          <div>
            <strong>{socialStats().inboxReady}</strong>
            <span>{i18n.tr('klar for innboks', 'inbox-ready')}</span>
          </div>
        </div>
        <Show
          when={socialRows().length > 0}
          fallback={<p class="verevon-settings-subnote">{i18n.tr('Sosiale leverandører er ennå ikke eksponert av integration-core.', 'Social providers have not been exposed by integration-core yet.')}</p>}
        >
          <div class="verevon-settings-social-provider-grid">
            <For each={socialRows()}>
              {(integration) => (
                <article class="verevon-settings-social-provider">
                  <div class="verevon-settings-social-provider__top">
                    <div>
                      <p>{integration.name}</p>
                      <span>{socialProviderRole(integration.provider, i18n)}</span>
                    </div>
                    <span class="verevon-settings-integration-status">{integration.status}</span>
                  </div>
                  <p>{integration.detail}</p>
                  <div class="verevon-settings-social-capabilities">
                    <For each={socialCapabilityLabels(integration, i18n)}>
                      {(capability) => <span>{capability}</span>}
                    </For>
                  </div>
                  <div class="verevon-settings-social-provider__actions">
                    <IntegrationRowActions
                      busyKey={actionBusy()}
                      instagramInboxConnected={instagramInboxConnected()}
                      row={integration}
                      onAction={runAction}
                    />
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </section>
      <div class="verevon-settings-list-card">
        <For each={rows()}>
          {(integration) => {
            const rowSyncProgress = () =>
              'connection' in integration && integration.connection
                ? syncProgress()[integration.connection.id]
                : undefined
            const groups = createMemo(() => capabilityGroups(integration.provider, integration.connection?.capabilities))
            const twoWay = () => groups().reads.length > 0 && groups().writes.length > 0
            return (
              <div class="verevon-settings-integration-row">
                <div class="verevon-settings-integration-row__main">
                  <div class="verevon-settings-integration-row__head">
                    <p>{integration.name}</p>
                    <Show when={groups().reads.length || groups().writes.length}>
                      <span class="verevon-settings-integration-direction">
                        {twoWay() ? i18n.tr('Toveis', 'Two-way') : groups().writes.length ? i18n.tr('Skriver', 'Writes') : i18n.tr('Leser', 'Reads')}
                      </span>
                    </Show>
                  </div>
                  <span>{rowSyncProgress() ? `${integration.detail} · ${rowSyncProgress()}` : integration.detail}</span>
                  <div class="verevon-settings-integration-caps">
                    <Show when={groups().reads.length}>
                      <span class="verevon-settings-integration-caps__group">
                        <em>{i18n.tr('Leser', 'Reads')}</em> {groups().reads.join(' · ')}
                      </span>
                    </Show>
                    <Show when={groups().writes.length}>
                      <span class="verevon-settings-integration-caps__group">
                        <em>{i18n.tr('Skriver', 'Writes')}</em> {groups().writes.join(' · ')}
                      </span>
                    </Show>
                  </div>
                </div>
                <div>
                  <span class="verevon-settings-integration-status">{integration.status}</span>
                  {'provider' in integration ? (
                    <IntegrationRowActions
                      busyKey={actionBusy()}
                      instagramInboxConnected={instagramInboxConnected()}
                      row={integration}
                      onAction={runAction}
                    />
                  ) : null}
                </div>
              </div>
            )
          }}
        </For>
      </div>
      <p class="verevon-settings-subnote">
        {i18n.tr(
          'Kildeinnhold og graf-endringer administreres senere i Kunnskap, der endringer kan gjennomgås og revideres.',
          'Source contents and graph edits are managed later in Knowledge, where changes can be reviewed and audited.',
        )}
        {loadFailed() ? ` ${i18n.tr('Integrasjonstilstanden kunne ikke oppdateres fra de lokale tjenestene.', 'Integration state could not be refreshed from the local services.')}` : ''}
      </p>
      <Show when={notice()}>
        {(message) => <p class="verevon-settings-status-message" role="status">{message()}</p>}
      </Show>
      <FeaturePanel
        title={i18n.tr('Webhook-levering', 'Webhook delivery')}
        description={i18n.tr('Leveringshelse for delte arbeidsområdeautomasjoner.', 'Delivery health for shared workspace automations.')}
        actionLabel={i18n.tr('Vis logger', 'View logs')}
        class="verevon-settings-feature-panel--spaced"
      >
        <p class="verevon-settings-panel-note">
          {i18n.tr('Ingen telemetri for webhook-levering er tilgjengelig for dette arbeidsområdet ennå.', 'No webhook delivery telemetry is available for this workspace yet.')}
        </p>
      </FeaturePanel>
    </>
  )
}

async function loadIntegrationSettingsSummary(orgId: string): Promise<IntegrationSettingsSummary> {
  const headers = integrationHeaders(orgId)
  const [providersResult, connectionsResult] = await Promise.all([
    requestJson<IntegrationSettingsProvidersResponse>('/api/v1/integrations/providers', { headers }),
    requestJson<IntegrationSettingsConnectionsResponse>('/api/v1/integrations/connections', { headers })
      .catch(() => ({ connections: [] })),
  ])
  const providers = arrayValue(providersResult.providers).map(normalizeIntegrationProvider)
  const connections = arrayValue(connectionsResult.connections)
    .map(normalizeIntegrationConnection)
    .filter((connection) => !connection.deletedAt)

  return {
    metrics: {
      connected: connections.filter((connection) => isConnectedIntegrationStatus(connection.status)).length,
      failed: connections.filter((connection) => isFailedIntegrationStatus(connection.status)).length,
      readyProviders: providers.filter((provider) => provider.configured && provider.directOAuthReady).length,
      syncing: connections.filter((connection) => isSyncingIntegrationStatus(connection.syncStatus, connection.latestSyncJob?.status)).length,
      totalProviders: providers.length,
    },
    providers,
    connections,
  }
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

function stringArrayValue(value: unknown): string[] {
  return arrayValue(value).filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
}

function normalizeIntegrationProvider(value: unknown): IntegrationSettingsProvider {
  const provider = recordFrom(value)
  const capabilities = arrayValue(provider.capabilities)
    .map((capability): IntegrationCapability | null => {
      const record = recordFrom(capability)
      const key = stringValue(record, 'key')
      if (!key) return null
      const rawDirection = stringValue(record, 'direction')
      // Trust the backend direction; fall back to a key-suffix heuristic so
      // older backends (no direction field) still render read/write correctly.
      const direction: 'read' | 'write' =
        rawDirection === 'write' || rawDirection === 'read'
          ? rawDirection
          : capabilityDirectionFromKey(key)
      return {
        key,
        label: stringValue(record, 'label') || undefined,
        description: stringValue(record, 'description') || undefined,
        direction,
        scopes: stringArrayValue(record.scopes),
        sensitive: record.sensitive === true,
      }
    })
    .filter((capability): capability is IntegrationCapability => Boolean(capability))

  return {
    key: stringValue(provider, 'key'),
    label: stringValue(provider, 'label', 'name') || stringValue(provider, 'key'),
    category: stringValue(provider, 'category') || 'source',
    configured: provider.configured === true,
    status: stringValue(provider, 'status') || 'unknown',
    missingConfig: stringArrayValue(provider.missingConfig),
    directOAuthReady: provider.directOAuthReady === true,
    capabilities,
    metaSdk: normalizeMetaSdk(provider.metaSdk),
    supersededBy: stringValue(provider, 'supersededBy', 'superseded_by') || undefined,
  }
}

function normalizeMetaSdk(value: unknown): MetaFacebookSdkConfig | undefined {
  const meta = recordFrom(value)
  if (Object.keys(meta).length === 0) return undefined
  return {
    enabled: meta.enabled === true,
    appId: stringValue(meta, 'appId', 'app_id') || undefined,
    apiVersion: stringValue(meta, 'apiVersion', 'api_version') || undefined,
    locale: stringValue(meta, 'locale') || undefined,
    loginConfigId: stringValue(meta, 'loginConfigId', 'login_config_id', 'configId', 'config_id') || undefined,
  }
}

function normalizeIntegrationConnection(value: unknown): IntegrationSettingsConnection {
  const connection = recordFrom(value)
  const providerKey = stringValue(connection, 'providerKey', 'provider_key')
  const scopes = stringArrayValue(connection.scopes)
  const providerContext = recordFrom(connection.providerContext ?? connection.provider_context)
  const providerAccountId = stringValue(connection, 'providerAccountId', 'provider_account_id') || undefined
  const providerEmail = stringValue(connection, 'providerEmail', 'provider_email')
    || stringValue(providerContext, 'mailbox_address', 'email')
    || undefined
  const providerWorkspaceName = stringValue(providerContext, 'notion_workspace_name', 'workspace_name') || undefined

  return {
    id: stringValue(connection, 'id'),
    providerKey,
    providerLabel: stringValue(connection, 'providerLabel', 'provider_label') || providerKey,
    displayName: stringValue(connection, 'displayName', 'display_name', 'providerAccountId', 'provider_account_id') || providerKey,
    providerAccountId,
    providerEmail,
    providerWorkspaceName,
    status: stringValue(connection, 'status') || 'unknown',
    capabilities: stringArrayValue(connection.capabilities),
    scopeCount: typeof connection.scopeCount === 'number' ? connection.scopeCount : scopes.length,
    syncStatus: stringValue(connection, 'syncStatus', 'sync_status', 'lastSyncStatus', 'last_sync_status') || 'unknown',
    deletedAt: stringValue(connection, 'deletedAt', 'deleted_at') || undefined,
    latestSyncJob: normalizeLatestSyncJob(connection.latestSyncJob ?? connection.latest_sync_job),
  }
}

function normalizeLatestSyncJob(value: unknown): IntegrationSettingsConnection['latestSyncJob'] {
  const job = recordFrom(value)
  const status = stringValue(job, 'status')
  if (!status) return undefined
  return { status, updatedAt: stringValue(job, 'updatedAt', 'updated_at') || undefined }
}

function integrationHeaders(orgId: string): HeadersInit | undefined {
  const trimmed = orgId.trim()
  return trimmed ? { 'x-verevon-org-id': trimmed } : undefined
}

function isFailedIntegrationStatus(status: string): boolean {
  return ['failed', 'error', 'needs_reconnect', 'needs_refresh', 'revoked', 'expired'].includes(status.trim().toLowerCase())
}

function isConnectedIntegrationStatus(status: string): boolean {
  return ['active', 'connected'].includes(status.trim().toLowerCase())
}

function isSyncingIntegrationStatus(syncStatus: string, latestJobStatus?: string): boolean {
  return [syncStatus, latestJobStatus ?? ''].some((status) =>
    ['queued', 'running', 'syncing', 'waiting_provider', 'handoff_data_plane'].includes(status.trim().toLowerCase()),
  )
}

function buildIntegrationRows(summary: IntegrationSettingsSummary | null, i18n: ReturnType<typeof useI18n>): IntegrationSettingsRow[] {
  if (!summary) return []

  const connectionsByProvider = new Map<string, IntegrationSettingsConnection[]>()
  for (const connection of summary.connections) {
    const providerConnections = connectionsByProvider.get(connection.providerKey) ?? []
    providerConnections.push(connection)
    connectionsByProvider.set(connection.providerKey, providerConnections)
  }
  return summary.providers.flatMap((provider) => {
    // Superseded providers are capabilities of the unified Meta integration,
    // not separate operator-facing products. Existing standalone connections
    // remain active in integration-core but are summarized by the Meta card.
    if (provider.supersededBy) return []
    const connections = connectionsByProvider.get(provider.key) ?? []
    return connections.length > 0
      ? connections.map((connection) => buildIntegrationRow(provider, connection, i18n, connections.length))
      : [buildIntegrationRow(provider, undefined, i18n, 0)]
  })
}

function buildIntegrationRow(
  provider: IntegrationSettingsProvider,
  connection: IntegrationSettingsConnection | undefined,
  i18n: ReturnType<typeof useI18n>,
  connectionCount: number,
): IntegrationSettingsRow {
  if (connection) {
      const needsReconnect = isFailedIntegrationStatus(connection.status)
      const accountDetails = [
        connection.providerEmail,
        connection.providerWorkspaceName,
        connection.displayName,
        connection.providerAccountId && connection.providerAccountId !== connection.displayName ? connection.providerAccountId : null,
      ].filter(Boolean)
      return {
        name: provider.label,
        detail: [
          accountDetails.join(' · '),
          connectionCount > 1 ? `${connectionCount} ${i18n.tr('kontoer', 'accounts')}` : null,
          connection.capabilities.length > 0 ? `${connection.capabilities.length} ${i18n.tr('kapabiliteter', 'capabilities')}` : null,
          connection.scopeCount > 0 ? `${connection.scopeCount} ${i18n.tr('tilganger', 'scopes')}` : null,
          connection.latestSyncJob?.status ? `${i18n.tr('synk', 'sync')} ${connection.latestSyncJob.status}` : null,
        ].filter(Boolean).join(' · '),
        status: needsReconnect ? i18n.tr('Krever ny tilkobling', 'Needs reconnect') : i18n.tr('Tilkoblet', 'Connected'),
        action: 'connected',
        connection,
        provider,
      }
    }

  if (!provider.configured) {
    return {
      name: provider.label,
      detail: provider.missingConfig.length > 0
        ? `${i18n.tr('Mangler', 'Missing')} ${provider.missingConfig.slice(0, 2).join(', ')}`
        : i18n.tr('Leverandørens legitimasjon er ikke konfigurert', 'Provider credentials are not configured'),
      status: i18n.tr('Mangler konfigurasjon', 'Missing config'),
      action: 'missing',
      provider,
    }
  }

  if (!provider.directOAuthReady) {
    return {
      name: provider.label,
      detail: `${provider.category} ${i18n.tr('adapter er konfigurert for administratoroppsett', 'adapter is configured for admin setup')}`,
      status: i18n.tr('Administratoroppsett', 'Admin setup'),
      action: 'admin',
      provider,
    }
  }

  return {
    name: provider.label,
    detail: `${provider.category} ${i18n.tr('kilde', 'source')} · ${provider.capabilities.length} ${i18n.tr('kapabiliteter', 'capabilities')}`,
    status: i18n.tr('Klar', 'Ready'),
    action: 'connect',
    provider,
  }
}

function isSocialIntegrationRow(row: IntegrationSettingsRow): boolean {
  return row.provider.category === 'social'
}

function buildSocialIntegrationStats(summary: IntegrationSettingsSummary | null) {
  // Superseded providers (the four legacy Meta entries) are excluded from the
  // provider count so "Meta" is one platform, but their existing connections
  // still count as connected.
  const socialProviders = summary?.providers.filter(
    (provider) => provider.category === 'social' && !provider.supersededBy,
  ) ?? []
  const socialProviderKeys = new Set(
    (summary?.providers ?? [])
      .filter((provider) => provider.category === 'social')
      .map((provider) => provider.key),
  )
  const socialConnections = summary?.connections.filter((connection) => socialProviderKeys.has(connection.providerKey)) ?? []
  const providerByKey = new Map((summary?.providers ?? []).map((provider) => [provider.key, provider]))
  const canonicalConnectionKeys = new Set(socialConnections
    .filter((connection) => isConnectedIntegrationStatus(connection.status))
    .map((connection) => providerByKey.get(connection.providerKey)?.supersededBy ?? connection.providerKey))

  return {
    providers: socialProviders.length,
    connected: canonicalConnectionKeys.size,
    publishingReady: socialConnections.filter((connection) => connection.capabilities.includes('social.post.write')).length,
    inboxReady: socialConnections.filter((connection) => connection.capabilities.includes('social.inbox.read')).length,
  }
}

function socialProviderRole(provider: IntegrationSettingsProvider, i18n: ReturnType<typeof useI18n>): string {
  switch (provider.key) {
    case 'meta':
      return i18n.tr('Facebook-sider, Instagram, WhatsApp og Meta Ads i én tilkobling', 'Facebook Pages, Instagram, WhatsApp, and Meta Ads in one connection')
    case 'facebook':
      return i18n.tr('Sidepublisering, kommentarer, innboks og analyse', 'Page publishing, comments, inbox, and analytics')
    case 'instagram':
      return i18n.tr('Mediepublisering, meldinger og analyse', 'Media publishing, messaging, and analytics')
    case 'whatsapp':
      return i18n.tr('WhatsApp Business-meldinger og kundesamtaler', 'WhatsApp Business messaging and customer conversations')
    case 'meta-ads':
      return i18n.tr('Meta Ads-konto, kampanje- og rapporteringsarbeidsflyt', 'Meta Ads account, campaign, and reporting workflows')
    case 'linkedin':
      return i18n.tr('Organisasjonsinnlegg, kommentarer og rapportering', 'Organization posts, comments, and reporting')
    case 'x':
      return i18n.tr('Innlegg, svar, direktemeldinger og målinger', 'Posts, replies, direct messages, and metrics')
    case 'tiktok':
      return i18n.tr('Content Posting API-opplastinger og statussporing', 'Content Posting API uploads and status tracking')
    case 'snapchat':
      return i18n.tr('Annonser, kreativt innhold, kampanje- og rapporteringsarbeidsflyt', 'Ads, creative, campaign, and reporting workflows')
    default:
      return i18n.tr('Sosial arbeidsflytadapter', 'Social workflow adapter')
  }
}

function socialCapabilityLabels(row: IntegrationSettingsRow, i18n: ReturnType<typeof useI18n>): string[] {
  const connectionCapabilities = row.connection?.capabilities ?? []
  const providerCapabilities = row.provider.capabilities.map((capability) => capability.key)
  // Once a connection exists, render only provider-verified grants. Falling
  // back to the provider catalog made incomplete Meta grants appear inbox- and
  // publishing-ready even when Graph reported only public_profile.
  const source = row.connection ? connectionCapabilities : providerCapabilities
  const socialCapabilities = source
    .filter((capability) => capability.startsWith('social.'))
    .map((capability) => formatSocialCapability(capability, i18n))

  return socialCapabilities.length > 0 ? socialCapabilities.slice(0, 4) : [i18n.tr('gjennomgang kreves', 'review required')]
}

function formatSocialCapability(capability: string, i18n: ReturnType<typeof useI18n>): string {
  switch (capability) {
    case 'social.profile.read':
      return i18n.tr('profil', 'profile')
    case 'social.post.write':
      return i18n.tr('publisering', 'publishing')
    case 'social.media.upload':
      return i18n.tr('media', 'media')
    case 'social.inbox.read':
      return i18n.tr('innboks', 'inbox')
    case 'social.analytics.read':
      return i18n.tr('analyse', 'analytics')
    case 'social.ads.manage':
      return i18n.tr('annonser', 'ads')
    default:
      return capability.replace(/^social\./, '').replace(/\./g, ' ')
  }
}

// Fallback for backends that don't yet emit capability.direction: infer it from
// the key so read/write rendering is correct either way (mirrors the Go
// capabilityDirection heuristic in integration-corev2).
function capabilityDirectionFromKey(key: string): 'read' | 'write' {
  const k = key.toLowerCase()
  const writeMarkers = ['.write', '.send', '.manage', '.post', '.publish', '.upload', '.create', '.delete', '.update']
  if (writeMarkers.some((m) => k.includes(m))) return 'write'
  if (['publishing', 'actions', 'write', 'send', 'manage'].includes(k)) return 'write'
  return 'read'
}

// Human label for any capability: prefer the backend label, else the last
// dotted segment of the key (e.g. "sharepoint.read" -> "sharepoint").
function formatCapabilityLabel(capability: IntegrationCapability): string {
  if (capability.label && capability.label.trim()) return capability.label.trim()
  const parts = capability.key.split('.')
  const tail = parts.length > 1 ? parts.slice(0, -1).join(' ') : parts[0]
  return (tail || capability.key).replace(/[-_]/g, ' ')
}

// Consistent read/write grouping for EVERY provider (not just social). Returns
// deduped, capped label lists so the UI can show "Reads … / Writes …" uniformly.
function capabilityGroups(
  provider: IntegrationSettingsProvider,
  grantedCapabilities?: string[],
): { reads: string[]; writes: string[] } {
  const reads: string[] = []
  const writes: string[] = []
  const granted = grantedCapabilities ? new Set(grantedCapabilities) : null
  for (const cap of provider.capabilities) {
    if (granted && !granted.has(cap.key)) continue
    const dir = cap.direction ?? capabilityDirectionFromKey(cap.key)
    const label = formatCapabilityLabel(cap)
    const bucket = dir === 'write' ? writes : reads
    if (!bucket.includes(label)) bucket.push(label)
  }
  return { reads: reads.slice(0, 4), writes: writes.slice(0, 4) }
}

function IntegrationRowActions(props: {
  busyKey: string | null
  instagramInboxConnected: boolean
  onAction: (row: IntegrationSettingsRow, action: 'connect' | 'connect-account' | 'disconnect' | 'instagram-inbox' | 'reconnect' | 'sync') => void
  row: IntegrationSettingsRow
}) {
  const i18n = useI18n()
  const busy = () => props.busyKey?.startsWith(`${props.row.provider.key}:`) ?? false
  return (
    <Switch>
      <Match when={props.row.action === 'connect'}>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'connect')}>
          {busy() ? i18n.tr('Åpner', 'Opening') : i18n.tr('Koble til', 'Connect')}
        </SettingsButton>
      </Match>
      <Match when={props.row.action === 'connected'}>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'connect-account')}>
          {busy() ? i18n.tr('Åpner', 'Opening') : i18n.tr('Koble til annen konto', 'Connect another account')}
        </SettingsButton>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'sync')}>
          {i18n.tr('Synkroniser', 'Sync')}
        </SettingsButton>
        <Show when={props.row.provider.key === 'meta' && !props.instagramInboxConnected && !props.row.connection?.capabilities.includes('social.inbox.read')}>
          <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'instagram-inbox')}>
            {i18n.tr('Aktiver Instagram-innboks', 'Enable Instagram inbox')}
          </SettingsButton>
        </Show>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'reconnect')}>
          {i18n.tr('Koble til på nytt', 'Reconnect')}
        </SettingsButton>
        <SettingsButton settingsSize="sm" danger disabled={busy()} onClick={() => void props.onAction(props.row, 'disconnect')}>
          {i18n.tr('Koble fra', 'Disconnect')}
        </SettingsButton>
      </Match>
    </Switch>
  )
}

// Delegates to the shared COOP-safe runner: completion is confirmed by
// polling the connect-session status endpoint server-side (postMessage kept
// as a fast path). Providers like X/LinkedIn/Microsoft set
// Cross-Origin-Opener-Policy on their login pages, which severs the popup
// handle — `popup.closed` then lies and postMessage from the callback page
// never arrives, so any window-state heuristic misreports "closed".
function runSettingsOAuth(session: ConnectSessionResult, reservedWindow?: Window | null): Promise<void> {
  if (session.authMode !== 'direct-oauth' || !session.connectUrl || !session.sessionToken) {
    return Promise.reject(new Error('Integration service returned an incomplete connect session.'))
  }
  return runDirectOauthWindow({
    connectUrl: session.connectUrl,
    sessionToken: session.sessionToken,
  }, reservedWindow)
}

function watchSyncProgress(
  connectionId: string,
  jobId: string,
  setSyncProgress: (next: (prev: Record<string, string>) => Record<string, string>) => Record<string, string>,
  eventSources: EventSource[],
  onTerminal?: () => void,
) {
  const source = new EventSource(`/api/v1/integrations/sync-jobs/${encodeURIComponent(jobId)}/events`)
  eventSources.push(source)
  const close = () => {
    source.close()
    const index = eventSources.indexOf(source)
    if (index >= 0) eventSources.splice(index, 1)
  }
  const update = (event: MessageEvent) => {
    const status = syncStatusFromEvent(event)
    if (!status) return
    setSyncProgress((prev) => ({ ...prev, [connectionId]: status }))
    if (['completed', 'failed', 'cancelled'].includes(status)) {
      close()
      onTerminal?.()
    }
  }
  source.addEventListener('sync.queued', update)
  source.addEventListener('sync.running', update)
  source.addEventListener('sync.waiting_provider', update)
  source.addEventListener('sync.handoff_data_plane', update)
  source.addEventListener('sync.completed', update)
  source.addEventListener('sync.failed', update)
  source.addEventListener('sync.cancelled', update)
  source.addEventListener('sync.snapshot', update)
  source.onerror = () => close()
}

function syncStatusFromEvent(event: MessageEvent): string | null {
  try {
    const payload = JSON.parse(String(event.data)) as {
      type?: string
      syncJob?: { status?: unknown }
    }
    if (payload.syncJob && typeof payload.syncJob.status === 'string') return payload.syncJob.status
    if (typeof payload.type === 'string') return payload.type.replace(/^sync\./, '')
  } catch {
    return null
  }
  return null
}
