import { Database, Loader2, ShieldCheck, ShieldOff, Sparkles, Unplug } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  aggregateToolActions,
  aggregateWorkspaceActivity,
  listToolActionEvents,
  type ToolActionSummary,
  type WorkspaceAiActivity,
} from '@/shared/api/audit-client'
import {
  disconnectConnection,
  getIntegrationProfile,
  listConnections,
  listProviders,
  type IntegrationConnection,
  type IntegrationProfile,
  type IntegrationProvider,
} from '@/shared/api/integrations-client'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { isGateOpen } from '@/shared/context/ownership-gate'

/**
 * Trust Center — per-connected-app transparency.
 *
 * One row per connected integration: what it can access (granted scopes /
 * declared capabilities), what data categories the AI has actually fetched
 * (aggregated from `tool_action` audit events), whether the AI has used it at
 * all, the retention class, last sync, and a disconnect control.
 *
 * Disconnect reuses the existing integrations client; the UI copy notes that
 * it also purges cached data (the backend erasure is wired separately).
 */

interface TrustCenterData {
  connections: IntegrationConnection[]
  providers: IntegrationProvider[]
  profile: IntegrationProfile | null
  toolActions: Map<string, ToolActionSummary>
  /** Workspace-level per-data-category AI activity (the honest, attribution-free view). */
  workspaceActivity: WorkspaceAiActivity
  /** Whether the audit read succeeded — drives the empty-vs-no-telemetry copy. */
  auditAvailable: boolean
}

async function loadTrustCenter(orgId: string): Promise<TrustCenterData> {
  // Connections drive the table; providers supply capability fallbacks; the
  // profile supplies last-sync. The audit read may legitimately fail (events
  // not yet flowing) — degrade gracefully rather than failing the whole page.
  const [connections, providers, profile] = await Promise.all([
    listConnections(orgId).catch(() => [] as IntegrationConnection[]),
    listProviders(orgId).catch(() => [] as IntegrationProvider[]),
    getIntegrationProfile(orgId).catch(() => null),
  ])

  let toolActions = new Map<string, ToolActionSummary>()
  let workspaceActivity: WorkspaceAiActivity = { totalEvents: 0, zdrEvents: 0, categories: [], tools: [] }
  let auditAvailable = false
  try {
    const events = await listToolActionEvents()
    toolActions = aggregateToolActions(events)
    workspaceActivity = aggregateWorkspaceActivity(events)
    auditAvailable = true
  } catch {
    auditAvailable = false
  }

  return { connections, providers, profile, toolActions, workspaceActivity, auditAvailable }
}

function providerFor(
  data: TrustCenterData,
  connection: IntegrationConnection,
): IntegrationProvider | undefined {
  return data.providers.find((provider) => provider.id === connection.providerId)
}

function appName(data: TrustCenterData, connection: IntegrationConnection): string {
  return (
    connection.providerName ??
    providerFor(data, connection)?.name ??
    connection.providerId
  )
}

/**
 * Permissions: prefer the connection's granted OAuth scopes; fall back to the
 * provider's declared capability labels. Empty when neither is exposed.
 */
function permissionLabels(data: TrustCenterData, connection: IntegrationConnection): string[] {
  if (connection.scopes && connection.scopes.length > 0) {
    return connection.scopes
  }
  if (connection.capabilities && connection.capabilities.length > 0) {
    return connection.capabilities
  }
  const provider = providerFor(data, connection)
  return provider?.capabilities?.map((capability) => capability.key) ?? []
}

/** Match a connection to its aggregated tool-action summary by id or provider. */
function toolActionFor(
  data: TrustCenterData,
  connection: IntegrationConnection,
): ToolActionSummary | undefined {
  const candidates = [connection.id, connection.providerId, appName(data, connection)]
    .filter((value): value is string => Boolean(value))
    .map((value) => value.toLowerCase())
  for (const candidate of candidates) {
    const match = data.toolActions.get(candidate)
    if (match) return match
  }
  return undefined
}

function retentionLabel(connection: IntegrationConnection): string {
  if (connection.retention && connection.retention.trim()) return connection.retention.trim()
  switch (connection.dataClass) {
    case 'customer':
      return 'Customer data · default org retention'
    case 'organization':
      return 'Organization data · default org retention'
    case 'public':
      return 'Public data · no special retention'
    default:
      // Placeholder pending a per-connection retention contract from the backend.
      return 'Org retention policy'
  }
}

function formatLastSync(value?: string): string {
  if (!value) return 'Never'
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString()
}

export function TrustCenterSection() {
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  const [data, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => loadTrustCenter(id),
  )

  const [busyConnectionId, setBusyConnectionId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const connections = createMemo(() => data()?.connections ?? [])

  const handleDisconnect = async (connection: IntegrationConnection) => {
    const id = orgId()
    if (!id || busyConnectionId()) return
    setBusyConnectionId(connection.id)
    setActionError(null)
    try {
      await disconnectConnection(id, connection.id)
      await refetch()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : 'Could not disconnect this app.')
    } finally {
      setBusyConnectionId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Connected apps"
        description="Every app the workspace has connected, what it can access, and exactly what the AI has done with it."
      />

      {/* Per-document privacy guarantee — gated on isGateOpen() so the claim
          appears ONLY when the backend is actually enforcing per-user privacy
          (strict + live identity). It states only what the retrieval path
          delivers; no claim is shown when the gate is closed. */}
      <Show when={isGateOpen()}>
        <section class="velion-trust-activity" aria-label="Per-document privacy">
          <header class="velion-trust-activity__head">
            <h3>Per-document privacy</h3>
          </header>
          <p class="velion-trust-muted">
            Documents marked Private are visible only to their owner and the people they're
            explicitly shared with — enforced everywhere a document is read: list views, search,
            retrieval, and the AI agent's grounding. Sharing grants are the single source of truth;
            there is no separate display-only flag.
          </p>
        </section>
      </Show>

      <Show when={actionError()}>
        {(message) => (
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={data()}
        fallback={
          <p class="velion-settings-subnote" role="status" aria-busy="true">
            Loading connected apps…
          </p>
        }
      >
        {(loaded) => (
          <>
            <section class="velion-trust-activity" aria-label="Workspace AI activity">
              <header class="velion-trust-activity__head">
                <Sparkles size={14} aria-hidden="true" />
                <span>AI activity in this workspace</span>
              </header>
              <Show
                when={loaded().auditAvailable}
                fallback={
                  <p class="velion-trust-muted">
                    Telemetry pending — no AI tool-action events are flowing yet.
                  </p>
                }
              >
                <Show
                  when={loaded().workspaceActivity.totalEvents > 0}
                  fallback={
                    <p class="velion-trust-muted">
                      No AI tool-action activity has been recorded for this workspace yet.
                    </p>
                  }
                >
                  <p class="velion-trust-activity__summary">
                    {loaded().workspaceActivity.totalEvents} recorded tool actions
                    {' · '}
                    {loaded().workspaceActivity.zdrEvents} under zero data retention
                  </p>
                  <div class="velion-trust-chips">
                    <For each={loaded().workspaceActivity.categories}>
                      {(cat) => (
                        <span class="velion-trust-chip velion-trust-chip--data">
                          <Database size={12} aria-hidden="true" />
                          {cat.category} · {cat.count}
                          <Show when={cat.zdrCount > 0}>
                            <span
                              class="velion-trust-activity__zdr"
                              title={`${cat.zdrCount} processed under zero data retention`}
                            >
                              <ShieldOff size={11} aria-hidden="true" /> {cat.zdrCount} ZDR
                            </span>
                          </Show>
                        </span>
                      )}
                    </For>
                  </div>
                  <Show when={loaded().workspaceActivity.tools.length > 0}>
                    <p class="velion-settings-subnote">Tools the AI invoked</p>
                    <div class="velion-trust-chips">
                      <For each={loaded().workspaceActivity.tools}>
                        {(tool) => (
                          <span class="velion-trust-chip velion-trust-chip--tool">
                            <Sparkles size={12} aria-hidden="true" />
                            {tool}
                          </span>
                        )}
                      </For>
                    </div>
                  </Show>
                  <p class="velion-settings-subnote">
                    Counted across the whole workspace — per-connection attribution is shown in the
                    table below only where an event could be reliably attributed.
                  </p>
                </Show>
              </Show>
            </section>

            <Show
              when={connections().length > 0}
              fallback={
                <div class="velion-settings-list-card">
                  <p class="velion-settings-empty-row">
                    No apps are connected yet. Connect a source under Integrations to see it here.
                  </p>
                </div>
              }
            >
            <div class="velion-trust-table" role="table" aria-label="Connected app transparency">
              <div class="velion-trust-row velion-trust-row--head" role="row">
                <span role="columnheader">App</span>
                <span role="columnheader">Permissions</span>
                <span role="columnheader">Data fetched</span>
                <span role="columnheader">Used by AI?</span>
                <span role="columnheader">Retention</span>
                <span role="columnheader">Last sync</span>
                <span role="columnheader" class="velion-trust-cell--actions">
                  <span class="velion-trust-sr">Disconnect</span>
                </span>
              </div>

              <For each={connections()}>
                {(connection) => {
                  const permissions = () => permissionLabels(loaded(), connection)
                  const summary = () => toolActionFor(loaded(), connection)
                  const usedByAi = () => Boolean(summary())
                  return (
                    <div class="velion-trust-row" role="row">
                      <span class="velion-trust-cell velion-trust-cell--app" role="cell">
                        <strong>{appName(loaded(), connection)}</strong>
                        <small>{connection.status}</small>
                      </span>

                      <span class="velion-trust-cell" role="cell">
                        <Show
                          when={permissions().length > 0}
                          fallback={<span class="velion-trust-muted">Not reported</span>}
                        >
                          <span class="velion-trust-chips">
                            <For each={permissions()}>
                              {(scope) => <span class="velion-trust-chip">{scope}</span>}
                            </For>
                          </span>
                        </Show>
                      </span>

                      <span class="velion-trust-cell" role="cell">
                        <Show
                          when={(summary()?.dataCategories.length ?? 0) > 0}
                          fallback={
                            <span class="velion-trust-muted">
                              {loaded().auditAvailable ? 'None recorded' : 'Telemetry pending'}
                            </span>
                          }
                        >
                          <span class="velion-trust-chips">
                            <For each={summary()?.dataCategories ?? []}>
                              {(category) => (
                                <span class="velion-trust-chip velion-trust-chip--data">
                                  <Database size={12} aria-hidden="true" />
                                  {category}
                                </span>
                              )}
                            </For>
                          </span>
                        </Show>
                      </span>

                      <span class="velion-trust-cell" role="cell">
                        <Show
                          when={usedByAi()}
                          fallback={
                            <span
                              class="velion-trust-pill velion-trust-pill--idle"
                              title="No tool-action event could be attributed to this connection. Per-connection attribution is not yet reliable, so this is not a claim that the AI has never used it."
                            >
                              Attribution unavailable
                            </span>
                          }
                        >
                          <span class="velion-trust-pill velion-trust-pill--active">
                            <Sparkles size={12} aria-hidden="true" />
                            Yes
                          </span>
                        </Show>
                      </span>

                      <span class="velion-trust-cell velion-trust-muted" role="cell">
                        {retentionLabel(connection)}
                      </span>

                      <span class="velion-trust-cell velion-trust-muted" role="cell">
                        {formatLastSync(connection.lastSyncAt)}
                      </span>

                      <span class="velion-trust-cell velion-trust-cell--actions" role="cell">
                        <SettingsButton
                          settingsSize="sm"
                          danger
                          disabled={busyConnectionId() === connection.id}
                          onClick={() => void handleDisconnect(connection)}
                          aria-label={`Disconnect ${appName(loaded(), connection)}`}
                        >
                          <Show
                            when={busyConnectionId() === connection.id}
                            fallback={<><Unplug size={14} aria-hidden="true" /> Disconnect</>}
                          >
                            <Loader2 size={14} class="velion-trust-spin" aria-hidden="true" />
                            Disconnecting…
                          </Show>
                        </SettingsButton>
                      </span>
                    </div>
                  )
                }}
              </For>
            </div>

            <p class="velion-settings-subnote">
              <ShieldCheck size={14} aria-hidden="true" /> Disconnecting an app revokes its access
              and purges its cached data. "Yes" under "Used by AI?" means a tool-action event was
              attributed to this connection; "Attribution unavailable" means none could be —
              not that the AI has never used it.
            </p>
          </Show>
          </>
        )}
      </Show>
    </>
  )
}
