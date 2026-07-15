import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from 'solid-js'
import RouterPolicyPage from '@/features/router-policy/components/RouterPolicyPage'
import FinetuneJobsPage from '@/features/finetune/components/FinetuneJobsPage'
import { TrustCenterSection } from '@/features/settings/components/TrustCenterSection'
import { McpServersSection } from '@/features/settings/components/McpServersSection'
import { SkillsSection } from '@/features/settings/components/SkillsSection'
import { PluginsSection } from '@/features/settings/components/PluginsSection'
import { HyperswitchCheckout } from '@/features/billing/components/HyperswitchCheckout'
import { NexiCheckout } from '@/features/billing/components/NexiCheckout'
import { runDirectOauthWindow } from '@/shared/integrations/provider-auth-window'
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
import {
  ensureMetaLogin,
  isMetaFacebookSdkEnabled,
  loadMetaFacebookSdk,
  type MetaFacebookSdkConfig,
} from '@/shared/integrations/meta-facebook-sdk'
import { getSession } from '@/shared/session/session-store'
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
  status: string
  capabilities: string[]
  scopeCount: number
  syncStatus: string
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
const securityControls: { title: string; description: string }[] = [
  {
    title: 'Require MFA for admins',
    description: 'Admins must use multi-factor authentication before accessing organization settings.',
  },
  {
    title: 'Restrict sign-in to verified domains',
    description: 'Only users with approved workspace domains can sign in.',
  },
  {
    title: 'Log admin configuration changes',
    description: 'Keep an audit trail for billing, member, SSO, and integration changes.',
  },
]

const sectionStatusCards: Record<WorkspaceSettingsSectionId, StatusCard[]> = {
  // Phase 4 PR-2 de-fake: the workspace + integrations status grids carried a
  // fabricated posture ('Verified' / 'aquatiq.no is ready' / 'Connected apps
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
  // Plugins renders its own live list + form, so it carries no shared status grid.
  plugins: [],
}

const businessHourRows = [
  { day: 'Monday-Friday', hours: '08:00-17:00', inbox: 'Priority support' },
  { day: 'Saturday', hours: '10:00-14:00', inbox: 'Overflow' },
]

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

export function VelionWorkspaceSettingsPage(props: {
  section?: WorkspaceSettingsSectionId
}) {
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
      setBillingError(reason instanceof Error ? reason.message : 'Could not load billing account.')
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
      label: 'Current plan',
      value: billingAccount() ? planLabel(billingAccount()?.plan) : '—',
      detail: billingError() ?? (billingAccount()?.subscription_state
        ? `Status: ${billingAccount()?.subscription_state}`
        : billingLoading()
          ? 'Loading plan information...'
          : 'Plan information unavailable'),
      tone: billingAccount()?.subscription_state === 'past_due' ? 'warn' : billingAccount() ? 'ok' : 'neutral',
    },
    {
      label: 'Credits',
      value: billingAccount()?.credits != null ? String(billingAccount()?.credits) : '—',
      detail: 'Available credits on this plan',
      tone: 'neutral',
    },
    {
      label: 'Payment method',
      value: billingAccount()?.provider_customer_id?.payment ? 'Stored' : '—',
      detail: billingAccount()?.provider_customer_id?.payment
        ? 'Payment customer is synced.'
        : 'Add a payment method through checkout.',
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
  const details = () => props.details
  const section = () => props.section
  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow="Admin"
        title={details().title}
        description={details().description}
      />

      <Show when={props.liveStatusCards[section()].length > 0}>
        <StatusGrid cards={props.liveStatusCards[section()]} />
      </Show>

      <section id={details().id} class="velion-settings-section velion-settings-section--after-status">
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

  return <VelionWorkspaceSettingsPage section={section()} />
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
      <Match when={props.section === 'plugins'}>
        <PluginsSection />
      </Match>
    </Switch>
  )
}

function WorkspaceSection() {
  return (
    <>
      <SectionHeader title="Workspace basics" description="Shared workspace fields that affect URLs, defaults, and support routing." />
      <div class="velion-settings-field-grid">
        <SettingsField id="workspace-name" label="Workspace name" value="aquatiq-as" />
        <SettingsField id="workspace-url" label="Workspace URL" value="aquatiq-as.velion.ai" />
        <SettingsField id="primary-domain" label="Primary domain" value="aquatiq.no" />
        <SettingsSelect
          id="data-region"
          label="Data region"
          value="europe"
          options={[
            { value: 'europe', label: 'Europe' },
            { value: 'us', label: 'United States' },
          ]}
        />
        <SettingsSelect
          id="default-language"
          label="Default language"
          value="english"
          options={[
            { value: 'english', label: 'English' },
            { value: 'norwegian', label: 'Norwegian' },
            { value: 'french', label: 'French' },
          ]}
        />
        <SettingsField id="admin-owner" label="Admin owner" value="Author Name" />
      </div>
      <div class="velion-settings-feature-grid">
        <FeaturePanel
          title="Verified domains"
          description="Domain records ready for DNS verification and customer-facing links."
          actionLabel="Add domain"
        >
          <p class="velion-settings-panel-note">No domains have been verified for this organization.</p>
        </FeaturePanel>
        <FeaturePanel
          title="Business hours"
          description="Workspace-wide routing windows for support inbox ownership."
          actionLabel="Edit schedule"
        >
          <div class="velion-settings-row-divider">
            <For each={businessHourRows}>
              {(row) => <DataRow primary={row.day} secondary={row.inbox} meta={row.hours} />}
            </For>
          </div>
        </FeaturePanel>
      </div>
    </>
  )
}

function MembersSection(props: { orgId: string | null }) {
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
        setError('Could not load members.')
        setLoading(false)
      })

    onCleanup(() => controller.abort())
  })

  const invite = async () => {
    const orgId = props.orgId
    const email = inviteEmail().trim().toLowerCase()
    if (!orgId || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email) || actionBusy()) {
      setActionError('Enter a valid email address before inviting a member.')
      return
    }
    setActionBusy(true)
    setActionError(null)
    try {
      await inviteMember(orgId, email, inviteRole())
      setInviteEmail('')
      await loadMembers(orgId)
    } catch {
      setActionError('The invitation could not be sent. Verify your admin access and try again.')
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
      setActionError('The member role could not be changed. The owner invariant is preserved.')
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
      setActionError('The member could not be removed. Auth Core kept the organization owner invariant intact.')
    } finally {
      setActionBusy(false)
    }
  }

  return (
    <>
      <SectionHeader title="Members & roles" description="Invite teammates, assign access, and review seat status." />
      <div class="velion-settings-invite-grid">
        <SettingsField
          id="invite-email"
          label="Invite by email"
          type="email"
          placeholder="teammate@company.com"
          value={inviteEmail()}
          onInput={(event) => setInviteEmail(event.currentTarget.value)}
        />
        <SettingsSelect
          id="invite-role"
          label="Role"
          value={inviteRole()}
          onChange={(event) => setInviteRole(event.currentTarget.value as MembershipRole)}
          options={[
            { value: 'member', label: 'Member' },
            { value: 'admin', label: 'Admin' },
          ]}
        />
        <SettingsButton
          variant="primary"
          disabled={actionBusy() || !props.orgId}
          onClick={() => void invite()}
        >
          Invite member
        </SettingsButton>
      </div>
      <Show when={actionError()}>
        <p role="alert" class="velion-settings-empty-row">{actionError()}</p>
      </Show>
      <div class="velion-settings-list-card">
        <Show when={!loading()} fallback={<p class="velion-settings-empty-row">Loading members...</p>}>
          <Show when={!error()} fallback={<p class="velion-settings-empty-row">{error()}</p>}>
            <Show when={members().length > 0} fallback={<p class="velion-settings-empty-row">No members found.</p>}>
              <For each={members()}>
                {(member) => (
                  <div class="velion-settings-member-row">
                    <div>
                      <p>{member.name ?? member.email}</p>
                      <span>{member.email}</span>
                    </div>
                    <select
                      aria-label={`Role for ${member.name ?? member.email}`}
                      value={member.role}
                      disabled={actionBusy() || member.role === 'owner'}
                      onChange={(event) => void changeRole(member, event.currentTarget.value as MembershipRole)}
                    >
                      <Show when={member.role === 'owner'}>
                        <option value="owner">Owner</option>
                      </Show>
                      <option value="member">Member</option>
                      <option value="admin">Admin</option>
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
                          ? `Confirm remove ${member.name ?? member.email}`
                          : `Remove ${member.name ?? member.email}`
                      }
                      class="velion-settings-icon-button"
                      onClick={() => void remove(member)}
                    >
                      {confirmRemoveId() === member.userId ? 'Confirm' : 'Remove'}
                    </button>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </Show>
      </div>
      <div class="velion-settings-feature-panel velion-settings-feature-panel--spaced">
        <div class="velion-settings-feature-panel__header">
          <div>
            <h3>Built-in roles</h3>
            <p>
              Auth Core currently supports owner, admin, and member. Owner transfer and
              custom role administration require separate explicit contracts.
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
        setError('Could not load users. This view requires a platform super-admin.')
        setLoading(false)
      })
    onCleanup(() => controller.abort())
  })

  return (
    <>
      <SectionHeader
        title="All users"
        description="Every user across all organizations. Platform super-admin only."
      />
      <div class="velion-settings-list-card">
        <Show when={!loading()} fallback={<p class="velion-settings-empty-row">Loading users...</p>}>
          <Show when={!error()} fallback={<p class="velion-settings-empty-row">{error()}</p>}>
            <Show when={users().length > 0} fallback={<p class="velion-settings-empty-row">No users found.</p>}>
              <For each={users()}>
                {(user) => (
                  <div class="velion-settings-member-row">
                    <div>
                      <p>{user.name || user.email}</p>
                      <span>{user.email}</span>
                    </div>
                    <strong>{user.role}</strong>
                    <span>{user.banned ? 'banned' : user.emailVerified ? 'verified' : 'unverified'}</span>
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

function quotaDisplay(value: number | null): string {
  if (value == null) return '—'
  if (value < 0) return 'Unlimited'
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
      setMessage('Payment was cancelled.')
      clearCheckoutParams()
      return
    }

    if (checkoutState !== 'success') return

    const paymentId = params.get('payment_id') || undefined
    const clientSecret = params.get('payment_intent_client_secret') || undefined
    const providerStatus = params.get('status') || 'processing'
    setSelectedPlan(planParam)

    if (!paymentId && !clientSecret) {
      setCheckoutError('Payment reference is missing. Start checkout again.')
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
      setMessage('Custom plan changes are handled by sales.')
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
          throw new Error('Checkout session did not include a valid payment surface.')
      }
    } catch (reason) {
      setCheckoutError(reason instanceof Error ? reason.message : 'Could not start checkout.')
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
        throw new Error(`Payment status is ${result.status}.`)
      }

      setCheckoutSession(undefined)
      setMessage(result.status === 'processing'
        ? 'Payment is processing. Your plan will stay active while confirmation completes.'
        : 'Payment confirmed. Your billing plan is active.')
      await props.onRefresh()
      clearCheckoutParams()
    } catch (reason) {
      setCheckoutError(reason instanceof Error ? reason.message : 'Could not confirm checkout.')
    } finally {
      setConfirmingCheckout(false)
    }
  }

  return (
    <>
      <SectionHeader title="Plan & usage" description="Review plan, usage, payment method, and invoices." />
      <Show when={props.error}>
        {(error) => <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">{error()}</p>}
      </Show>
      <div class="velion-settings-metric-grid">
        <Metric label="Plan" value={plan()} detail={`Status: ${status()}`} />
        <Metric label="Seats" value={quotaDisplay(seatLimit())} detail={seatLimit() != null ? 'Seats included on this plan' : 'Seat quota unavailable'} />
        <Metric label="Credits" value={credits()} detail="Available on this plan" />
      </div>
      <section class="velion-settings-plan-picker" aria-labelledby="settings-billing-plan-heading">
        <div class="velion-settings-section-header velion-settings-section-header--compact">
          <h2 id="settings-billing-plan-heading">Choose plan</h2>
          <p>Change the workspace plan through billing-core checkout. Paid plans open the configured secure payment provider.</p>
        </div>
        <div class="velion-settings-plan-list">
          <For each={billingPlans}>
            {(billingPlan) => {
              const active = () => currentPaidPlan() === billingPlan.id
              const selected = () => selectedPlan() === billingPlan.id
              return (
                <article
                  class="velion-settings-plan-row"
                  classList={{
                    'velion-settings-plan-row--active': active(),
                    'velion-settings-plan-row--selected': selected(),
                  }}
                >
                  <div class="velion-settings-plan-row__copy">
                    <div>
                      <h3>{billingPlan.name}</h3>
                      <span>{billingPlan.priceLabel}</span>
                    </div>
                    <p>{billingPlan.description}</p>
                    <ul>
                      <For each={billingPlan.features}>{(feature) => <li>{feature}</li>}</For>
                    </ul>
                  </div>
                  <div class="velion-settings-plan-row__actions">
                    <Show when={active()}>
                      <span class="velion-settings-plan-pill">Current plan</span>
                    </Show>
                    <SettingsButton
                      settingsSize="sm"
                      variant={selected() ? 'primary' : 'secondary'}
                      disabled={props.loading || startingCheckout() || confirmingCheckout() || active()}
                      onClick={() => void startPlanCheckout(billingPlan.id)}
                    >
                      {active()
                        ? 'Active'
                        : billingPlan.checkoutEnabled
                          ? `${startingCheckout() && selected() ? 'Starting' : 'Activate'} ${billingPlan.name}`
                          : 'Contact sales'}
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
        {(text) => <p class="velion-settings-status-message velion-settings-status-message--success" role="status">{text()}</p>}
      </Show>
      <Show when={checkoutError()}>
        {(text) => <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">{text()}</p>}
      </Show>
      <div class="velion-settings-field-grid velion-settings-field-grid--spaced">
        <SettingsField id="billing-email" label="Billing email" type="email" placeholder="billing@yourcompany.com" />
        <SettingsSelect
          id="usage-cap"
          label="Usage cap"
          value="notify"
          options={[
            { value: 'notify', label: 'Notify at 80%' },
            { value: 'pause', label: 'Pause at limit' },
            { value: 'none', label: 'No cap' },
          ]}
        />
      </div>
      <div class="velion-settings-billing-grid">
        <FeaturePanel
          title="Invoice history"
          description="Workspace invoice history and downloadable billing records."
          actionLabel="Download CSV"
        >
          <p class="velion-settings-panel-note">Ingen fakturaer ennå - invoices will appear here once a paid plan is active.</p>
        </FeaturePanel>
        <FeaturePanel
          title="Spend controls"
          description="Plan limits and quota details for this workspace."
          actionLabel="Configure"
        >
          <Show
            when={props.account != null}
            fallback={<p class="velion-settings-panel-note">Loading plan details...</p>}
          >
            <div class="velion-settings-key-values">
              <p><span>Plan</span><strong>{plan()}</strong></p>
              <p><span>Status</span><strong>{status()}</strong></p>
              <p><span>Credits</span><strong>{credits()}</strong></p>
              <p><span>API calls</span><strong>{quotaDisplay(apiCallLimit())}</strong></p>
              <p><span>Storage</span><strong>{quotaDisplay(storageLimit())}</strong></p>
            </div>
          </Show>
        </FeaturePanel>
      </div>
    </>
  )
}

function SsoSection() {
  return (
    <>
      <SectionHeader title="SSO configuration" description="Configure organization sign-in, domains, and provisioning." />
      <p class="velion-settings-panel-note">
        Single sign-on is not configured for this organization. No identity
        provider, allowed domain, SCIM token, or attribute mapping is set up, and
        SSO/SCIM configuration is not available in this build.
      </p>
    </>
  )
}

function RecentSecurityEvents() {
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
    <Show when={!loading()} fallback={<p class="velion-settings-panel-note">Loading security events...</p>}>
      <Show when={!loadFailed()} fallback={<p class="velion-settings-panel-note" role="alert">Could not load security events.</p>}>
      <Show when={events().length > 0} fallback={<p class="velion-settings-panel-note">Ingen sikkerhetshendelser ennå</p>}>
        <div class="velion-settings-row-divider">
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
                  primary={`${event.event ?? 'Event'}${event.outcome ? ` - ${event.outcome}` : ''}`}
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

function OrgSecuritySection() {
  return (
    <>
      <SectionHeader title="Security policy" description="Set organization-wide security requirements and audit controls." />
      <div class="velion-settings-divided-list">
        <For each={securityControls}>
          {(control) => (
            <ToggleRow title={control.title} description={control.description} enabled={false} disabled />
          )}
        </For>
      </div>
      <p class="velion-settings-panel-note">
        Organization-wide security policy is not yet connected to a live source for this workspace, so these controls are shown unconfigured. They will reflect real policy state once an org-security backend is wired.
      </p>
      <div class="velion-settings-field-grid velion-settings-field-grid--spaced">
        <SettingsSelect
          id="session-duration"
          label="Session duration"
          value="30-days"
          options={[
            { value: '7-days', label: '7 days' },
            { value: '30-days', label: '30 days' },
            { value: '90-days', label: '90 days' },
          ]}
        />
      </div>
      <FeaturePanel
        title="Recent security events"
        description="Recent organization changes that administrators should review."
        actionLabel="Open audit log"
        class="velion-settings-feature-panel--spaced"
      >
        <RecentSecurityEvents />
      </FeaturePanel>
    </>
  )
}

function IntegrationsSection() {
  const session = getSession()
  const [summary, setSummary] = createSignal<IntegrationSettingsSummary | null>(null)
  const [actionBusy, setActionBusy] = createSignal<string | null>(null)
  const [loadFailed, setLoadFailed] = createSignal(false)
  const [syncProgress, setSyncProgress] = createSignal<Record<string, string>>({})
  const [notice, setNotice] = createSignal<string | null>(null)
  const eventSources: EventSource[] = []
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  const refresh = async () => {
    try {
      setSummary(await loadIntegrationSettingsSummary(orgId()))
      setLoadFailed(false)
    } catch {
      setLoadFailed(true)
    }
  }

  onMount(() => {
    void refresh()
  })

  onCleanup(() => {
    for (const source of eventSources) source.close()
    eventSources.splice(0, eventSources.length)
  })

  const rows = createMemo(() => buildIntegrationRows(summary()))
  const socialRows = createMemo(() => rows().filter(isSocialIntegrationRow))
  const socialStats = createMemo(() => buildSocialIntegrationStats(summary()))

  createEffect(() => {
    const config = socialRows()
      .map((row) => row.provider.metaSdk)
      .find(isMetaFacebookSdkEnabled)
    if (!config) return
    void loadMetaFacebookSdk(config).catch(() => undefined)
  })

  const runAction = async (
    row: IntegrationSettingsRow,
    action: 'connect' | 'disconnect' | 'reconnect' | 'sync',
  ) => {
    const busyKey = `${row.provider.key}:${action}`
    setActionBusy(busyKey)
    setNotice(null)

    try {
      if (action === 'connect') {
        await prepareMetaSdkLogin(row.provider)
        const session = await requestJson<ConnectSessionResult>(
          `/api/v1/integrations/providers/${encodeURIComponent(row.provider.key)}/connect-session`,
          {
            method: 'POST',
            body: JSON.stringify({ bundles: connectBundlesFor(row.provider) }),
            headers: integrationHeaders(orgId()),
          },
        )
        await runSettingsOAuth(session)
        setNotice(`${row.name} connected.`)
      } else if (action === 'reconnect' && row.connection) {
        await prepareMetaSdkLogin(row.provider)
        const session = await requestJson<ConnectSessionResult>(
          `/api/v1/integrations/connections/${encodeURIComponent(row.connection.id)}/reconnect-session`,
          { method: 'POST', body: JSON.stringify({}), headers: integrationHeaders(orgId()) },
        )
        await runSettingsOAuth(session)
        setNotice(`${row.name} reconnected.`)
      } else if (action === 'disconnect' && row.connection) {
        await requestJson<{ disconnected: boolean }>(
          `/api/v1/integrations/connections/${encodeURIComponent(row.connection.id)}`,
          { method: 'DELETE', headers: integrationHeaders(orgId()) },
        )
        setNotice(`${row.name} disconnected.`)
      } else if (action === 'sync' && row.connection) {
        const result = await requestJson<{ syncJob?: { id?: string; status?: string } }>(
          '/api/v1/integrations/sync-jobs',
          {
            method: 'POST',
            body: JSON.stringify({ connectionId: row.connection.id, reason: 'settings_user_requested', mode: 'incremental' }),
            headers: integrationHeaders(orgId()),
          },
        )
        const jobId = result.syncJob?.id
        setSyncProgress((prev) => ({
          ...prev,
          [row.connection!.id]: result.syncJob?.status ?? 'queued',
        }))
        if (jobId) watchSyncProgress(row.connection.id, jobId, setSyncProgress, eventSources, refresh)
      }
      await refresh()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Integration action failed.')
    } finally {
      setActionBusy(null)
    }
  }

  return (
    <>
      <SectionHeader title="Workspace integrations" description="Connect shared systems used by the workspace." />
      <Show when={summary()}>
        {(current) => (
          <div class="velion-settings-metric-grid">
            <Metric
              label="Connected"
              value={String(current().metrics.connected)}
              detail={`${current().metrics.readyProviders} providers configured`}
            />
            <Metric
              label="Syncing"
              value={String(current().metrics.syncing)}
              detail="Live sync jobs visible from integration-corev2"
            />
            <Metric
              label="Attention"
              value={String(current().metrics.failed)}
              detail="Sources needing reconnect or review"
            />
          </div>
        )}
      </Show>
      <section class="velion-settings-social-layer" aria-labelledby="velion-settings-social-title">
        <div class="velion-settings-social-layer__header">
          <div>
            <span>Social layer</span>
            <h3 id="velion-settings-social-title">Publishing, inbox, and campaign adapters</h3>
            <p>
              integration-core owns OAuth and token leases; social-core consumes scoped capabilities for provider-specific workflows.
            </p>
          </div>
          <a class="velion-settings-button velion-settings-button--sm" href="/social/calendar">Open calendar</a>
        </div>
        <div class="velion-settings-social-summary">
          <div>
            <strong>{socialStats().providers}</strong>
            <span>social providers</span>
          </div>
          <div>
            <strong>{socialStats().connected}</strong>
            <span>connected</span>
          </div>
          <div>
            <strong>{socialStats().publishingReady}</strong>
            <span>publish-ready</span>
          </div>
          <div>
            <strong>{socialStats().inboxReady}</strong>
            <span>inbox-ready</span>
          </div>
        </div>
        <Show
          when={socialRows().length > 0}
          fallback={<p class="velion-settings-subnote">Social providers have not been exposed by integration-core yet.</p>}
        >
          <div class="velion-settings-social-provider-grid">
            <For each={socialRows()}>
              {(integration) => (
                <article class="velion-settings-social-provider">
                  <div class="velion-settings-social-provider__top">
                    <div>
                      <p>{integration.name}</p>
                      <span>{socialProviderRole(integration.provider)}</span>
                    </div>
                    <span class="velion-settings-integration-status">{integration.status}</span>
                  </div>
                  <p>{integration.detail}</p>
                  <div class="velion-settings-social-capabilities">
                    <For each={socialCapabilityLabels(integration)}>
                      {(capability) => <span>{capability}</span>}
                    </For>
                  </div>
                  <div class="velion-settings-social-provider__actions">
                    <IntegrationRowActions
                      busyKey={actionBusy()}
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
      <div class="velion-settings-list-card">
        <For each={rows()}>
          {(integration) => {
            const rowSyncProgress = () =>
              'connection' in integration && integration.connection
                ? syncProgress()[integration.connection.id]
                : undefined
            const groups = createMemo(() => capabilityGroups(integration.provider))
            const twoWay = () => groups().reads.length > 0 && groups().writes.length > 0
            return (
              <div class="velion-settings-integration-row">
                <div class="velion-settings-integration-row__main">
                  <div class="velion-settings-integration-row__head">
                    <p>{integration.name}</p>
                    <Show when={groups().reads.length || groups().writes.length}>
                      <span class="velion-settings-integration-direction">
                        {twoWay() ? 'Toveis' : groups().writes.length ? 'Skriver' : 'Leser'}
                      </span>
                    </Show>
                  </div>
                  <span>{rowSyncProgress() ? `${integration.detail} · ${rowSyncProgress()}` : integration.detail}</span>
                  <div class="velion-settings-integration-caps">
                    <Show when={groups().reads.length}>
                      <span class="velion-settings-integration-caps__group">
                        <em>Leser</em> {groups().reads.join(' · ')}
                      </span>
                    </Show>
                    <Show when={groups().writes.length}>
                      <span class="velion-settings-integration-caps__group">
                        <em>Skriver</em> {groups().writes.join(' · ')}
                      </span>
                    </Show>
                  </div>
                </div>
                <div>
                  <span class="velion-settings-integration-status">{integration.status}</span>
                  {'provider' in integration ? (
                    <IntegrationRowActions
                      busyKey={actionBusy()}
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
      <p class="velion-settings-subnote">
        Source contents and graph edits are managed later in Knowledge, where changes can be reviewed and audited.
        {loadFailed() ? ' Integration state could not be refreshed from the local services.' : ''}
      </p>
      <Show when={notice()}>
        {(message) => <p class="velion-settings-status-message" role="status">{message()}</p>}
      </Show>
      <FeaturePanel
        title="Webhook delivery"
        description="Delivery health for shared workspace automations."
        actionLabel="View logs"
        class="velion-settings-feature-panel--spaced"
      >
        <p class="velion-settings-panel-note">
          No webhook delivery telemetry is available for this workspace yet.
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
  const connections = arrayValue(connectionsResult.connections).map(normalizeIntegrationConnection)

  return {
    metrics: {
      connected: connections.length,
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

  return {
    id: stringValue(connection, 'id'),
    providerKey,
    providerLabel: stringValue(connection, 'providerLabel', 'provider_label') || providerKey,
    displayName: stringValue(connection, 'displayName', 'display_name', 'providerAccountId', 'provider_account_id') || providerKey,
    status: stringValue(connection, 'status') || 'unknown',
    capabilities: stringArrayValue(connection.capabilities),
    scopeCount: typeof connection.scopeCount === 'number' ? connection.scopeCount : scopes.length,
    syncStatus: stringValue(connection, 'syncStatus', 'sync_status', 'lastSyncStatus', 'last_sync_status') || 'unknown',
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
  return trimmed ? { 'x-velion-org-id': trimmed } : undefined
}

function isFailedIntegrationStatus(status: string): boolean {
  return ['failed', 'error', 'needs_reconnect', 'revoked', 'expired'].includes(status.trim().toLowerCase())
}

function isSyncingIntegrationStatus(syncStatus: string, latestJobStatus?: string): boolean {
  return [syncStatus, latestJobStatus ?? ''].some((status) =>
    ['queued', 'running', 'syncing', 'waiting_provider', 'handoff_data_plane'].includes(status.trim().toLowerCase()),
  )
}

function buildIntegrationRows(summary: IntegrationSettingsSummary | null): IntegrationSettingsRow[] {
  if (!summary) return []

  const connectionsByProvider = new Map(summary.connections.map((connection) => [connection.providerKey, connection]))
  return summary.providers.flatMap((provider) => {
    const connection = connectionsByProvider.get(provider.key)
    // Superseded providers (facebook/instagram/whatsapp/meta-ads → the unified
    // "meta" card) are hidden from the new-connection list; they only surface
    // while an existing legacy connection is still attached.
    if (provider.supersededBy && !connection) return []
    return [buildIntegrationRow(provider, connection)]
  })
}

function buildIntegrationRow(
  provider: IntegrationSettingsProvider,
  connection: IntegrationSettingsConnection | undefined,
): IntegrationSettingsRow {
  if (connection) {
      return {
        name: provider.label,
        detail: [
          connection.displayName,
          connection.capabilities.length > 0 ? `${connection.capabilities.length} capabilities` : null,
          connection.scopeCount > 0 ? `${connection.scopeCount} scopes` : null,
          connection.latestSyncJob?.status ? `sync ${connection.latestSyncJob.status}` : null,
        ].filter(Boolean).join(' · '),
        status: 'Connected',
        action: 'connected',
        connection,
        provider,
      }
    }

  if (!provider.configured) {
    return {
      name: provider.label,
      detail: provider.missingConfig.length > 0
        ? `Missing ${provider.missingConfig.slice(0, 2).join(', ')}`
        : 'Provider credentials are not configured',
      status: 'Missing config',
      action: 'missing',
      provider,
    }
  }

  if (!provider.directOAuthReady) {
    return {
      name: provider.label,
      detail: `${provider.category} adapter is configured for admin setup`,
      status: 'Admin setup',
      action: 'admin',
      provider,
    }
  }

  return {
    name: provider.label,
    detail: `${provider.category} source · ${provider.capabilities.length} capabilities`,
    status: 'Ready',
    action: 'connect',
    provider,
  }
}

function isSocialIntegrationRow(row: IntegrationSettingsRow): boolean {
  return row.provider.category === 'social'
}

function connectBundlesFor(provider: IntegrationSettingsProvider): string[] {
  return provider.category === 'social' ? ['full'] : ['knowledge']
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

  return {
    providers: socialProviders.length,
    connected: socialConnections.length,
    publishingReady: socialConnections.filter((connection) => connection.capabilities.includes('social.post.write')).length,
    inboxReady: socialConnections.filter((connection) => connection.capabilities.includes('social.inbox.read')).length,
  }
}

function socialProviderRole(provider: IntegrationSettingsProvider): string {
  switch (provider.key) {
    case 'meta':
      return 'Facebook Pages, Instagram, WhatsApp, and Meta Ads in one connection'
    case 'facebook':
      return 'Page publishing, comments, inbox, and analytics'
    case 'instagram':
      return 'Media publishing, messaging, and analytics'
    case 'whatsapp':
      return 'WhatsApp Business messaging and customer conversations'
    case 'meta-ads':
      return 'Meta Ads account, campaign, and reporting workflows'
    case 'linkedin':
      return 'Organization posts, comments, and reporting'
    case 'x':
      return 'Posts, replies, direct messages, and metrics'
    case 'tiktok':
      return 'Content Posting API uploads and status tracking'
    case 'snapchat':
      return 'Ads, creative, campaign, and reporting workflows'
    default:
      return 'Social workflow adapter'
  }
}

async function prepareMetaSdkLogin(provider: IntegrationSettingsProvider): Promise<void> {
  if (!isMetaIntegrationProvider(provider.key) || !isMetaFacebookSdkEnabled(provider.metaSdk)) return
  await ensureMetaLogin(provider.metaSdk)
}

function isMetaIntegrationProvider(providerKey: string): boolean {
  return ['meta', 'facebook', 'instagram', 'whatsapp', 'meta-ads'].includes(providerKey.trim().toLowerCase())
}

function socialCapabilityLabels(row: IntegrationSettingsRow): string[] {
  const connectionCapabilities = row.connection?.capabilities ?? []
  const providerCapabilities = row.provider.capabilities.map((capability) => capability.key)
  const source = connectionCapabilities.length > 0 ? connectionCapabilities : providerCapabilities
  const socialCapabilities = source
    .filter((capability) => capability.startsWith('social.'))
    .map(formatSocialCapability)

  return socialCapabilities.length > 0 ? socialCapabilities.slice(0, 4) : ['review required']
}

function formatSocialCapability(capability: string): string {
  switch (capability) {
    case 'social.profile.read':
      return 'profile'
    case 'social.post.write':
      return 'publishing'
    case 'social.media.upload':
      return 'media'
    case 'social.inbox.read':
      return 'inbox'
    case 'social.analytics.read':
      return 'analytics'
    case 'social.ads.manage':
      return 'ads'
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
function capabilityGroups(provider: IntegrationSettingsProvider): { reads: string[]; writes: string[] } {
  const reads: string[] = []
  const writes: string[] = []
  for (const cap of provider.capabilities) {
    const dir = cap.direction ?? capabilityDirectionFromKey(cap.key)
    const label = formatCapabilityLabel(cap)
    const bucket = dir === 'write' ? writes : reads
    if (!bucket.includes(label)) bucket.push(label)
  }
  return { reads: reads.slice(0, 4), writes: writes.slice(0, 4) }
}

function IntegrationRowActions(props: {
  busyKey: string | null
  onAction: (row: IntegrationSettingsRow, action: 'connect' | 'disconnect' | 'reconnect' | 'sync') => void
  row: IntegrationSettingsRow
}) {
  const busy = () => props.busyKey?.startsWith(`${props.row.provider.key}:`) ?? false
  return (
    <Switch>
      <Match when={props.row.action === 'connect'}>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'connect')}>
          {busy() ? 'Opening' : 'Connect'}
        </SettingsButton>
      </Match>
      <Match when={props.row.action === 'connected'}>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'sync')}>
          Sync
        </SettingsButton>
        <SettingsButton settingsSize="sm" disabled={busy()} onClick={() => void props.onAction(props.row, 'reconnect')}>
          Reconnect
        </SettingsButton>
        <SettingsButton settingsSize="sm" danger disabled={busy()} onClick={() => void props.onAction(props.row, 'disconnect')}>
          Disconnect
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
function runSettingsOAuth(session: ConnectSessionResult): Promise<void> {
  if (session.authMode !== 'direct-oauth' || !session.connectUrl || !session.sessionToken) {
    return Promise.reject(new Error('Integration service returned an incomplete connect session.'))
  }
  return runDirectOauthWindow({
    connectUrl: session.connectUrl,
    sessionToken: session.sessionToken,
  })
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
