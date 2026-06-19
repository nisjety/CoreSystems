import { Database, Loader2, ShieldCheck, Sparkles, Unplug } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  aggregateToolActions,
  listToolActionEvents,
  type ToolActionSummary,
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
  let auditAvailable = false
  try {
    const events = await listToolActionEvents()
    toolActions = aggregateToolActions(events)
    auditAvailable = true
  } catch {
    auditAvailable = false
  }

  return { connections, providers, profile, toolActions, auditAvailable }
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
                          fallback={<span class="velion-trust-pill velion-trust-pill--idle">No</span>}
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
              and purges its cached data. "Used by AI?" and "Data fetched" reflect recorded
              tool-action audit events for this workspace.
            </p>
          </Show>
        )}
      </Show>
    </>
  )
}
