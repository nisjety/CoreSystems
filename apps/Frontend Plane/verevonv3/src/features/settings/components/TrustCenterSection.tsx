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
import { useI18n } from '@/shared/i18n'

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

function retentionLabel(connection: IntegrationConnection, i18n: ReturnType<typeof useI18n>): string {
  if (connection.retention && connection.retention.trim()) return connection.retention.trim()
  switch (connection.dataClass) {
    case 'customer':
      return i18n.tr('Kundedata · standard organisasjonsretensjon', 'Customer data · default org retention')
    case 'organization':
      return i18n.tr('Organisasjonsdata · standard organisasjonsretensjon', 'Organization data · default org retention')
    case 'public':
      return i18n.tr('Offentlige data · ingen spesiell retensjon', 'Public data · no special retention')
    default:
      // Placeholder pending a per-connection retention contract from the backend.
      return i18n.tr('Organisasjonens retensjonspolicy', 'Org retention policy')
  }
}

function formatLastSync(value: string | undefined, i18n: ReturnType<typeof useI18n>): string {
  if (!value) return i18n.tr('Aldri', 'Never')
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString()
}

export function TrustCenterSection() {
  const i18n = useI18n()
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
      setActionError(err instanceof Error ? err.message : i18n.tr('Kunne ikke koble fra denne appen.', 'Could not disconnect this app.'))
    } finally {
      setBusyConnectionId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title={i18n.tr('Tilkoblede apper', 'Connected apps')}
        description={i18n.tr(
          'Alle apper arbeidsområdet har koblet til, hva de kan få tilgang til, og nøyaktig hva AI-en har gjort med dem.',
          'Every app the workspace has connected, what it can access, and exactly what the AI has done with it.',
        )}
      />

      {/* Per-document privacy guarantee — gated on isGateOpen() so the claim
          appears ONLY when the backend is actually enforcing per-user privacy
          (strict + live identity). It states only what the retrieval path
          delivers; no claim is shown when the gate is closed. */}
      <Show when={isGateOpen()}>
        <section class="verevon-trust-activity" aria-label={i18n.tr('Personvern per dokument', 'Per-document privacy')}>
          <header class="verevon-trust-activity__head">
            <h3>{i18n.tr('Personvern per dokument', 'Per-document privacy')}</h3>
          </header>
          <p class="verevon-trust-muted">
            {i18n.tr(
              'Dokumenter merket Privat er kun synlige for eieren og personene de er uttrykkelig delt med — håndhevet overalt et dokument leses: listevisninger, søk, henting og AI-agentens kildegrunnlag. Delingstillatelser er den eneste kilden til sannhet; det finnes ingen separat visningsflagg.',
              "Documents marked Private are visible only to their owner and the people they're explicitly shared with — enforced everywhere a document is read: list views, search, retrieval, and the AI agent's grounding. Sharing grants are the single source of truth; there is no separate display-only flag.",
            )}
          </p>
        </section>
      </Show>

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={data()}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster tilkoblede apper…', 'Loading connected apps…')}
          </p>
        }
      >
        {(loaded) => (
          <>
            <section class="verevon-trust-activity" aria-label={i18n.tr('AI-aktivitet i arbeidsområdet', 'Workspace AI activity')}>
              <header class="verevon-trust-activity__head">
                <Sparkles size={14} aria-hidden="true" />
                <span>{i18n.tr('AI-aktivitet i dette arbeidsområdet', 'AI activity in this workspace')}</span>
              </header>
              <Show
                when={loaded().auditAvailable}
                fallback={
                  <p class="verevon-trust-muted">
                    {i18n.tr('Telemetri avventes — ingen AI-verktøyshendelser er registrert ennå.', 'Telemetry pending — no AI tool-action events are flowing yet.')}
                  </p>
                }
              >
                <Show
                  when={loaded().workspaceActivity.totalEvents > 0}
                  fallback={
                    <p class="verevon-trust-muted">
                      {i18n.tr('Ingen AI-verktøysaktivitet er registrert for dette arbeidsområdet ennå.', 'No AI tool-action activity has been recorded for this workspace yet.')}
                    </p>
                  }
                >
                  <p class="verevon-trust-activity__summary">
                    {loaded().workspaceActivity.totalEvents} {i18n.tr('registrerte verktøyshandlinger', 'recorded tool actions')}
                    {' · '}
                    {loaded().workspaceActivity.zdrEvents} {i18n.tr('under Zero Data Retention', 'under zero data retention')}
                  </p>
                  <div class="verevon-trust-chips">
                    <For each={loaded().workspaceActivity.categories}>
                      {(cat) => (
                        <span class="verevon-trust-chip verevon-trust-chip--data">
                          <Database size={12} aria-hidden="true" />
                          {cat.category} · {cat.count}
                          <Show when={cat.zdrCount > 0}>
                            <span
                              class="verevon-trust-activity__zdr"
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
                    <p class="verevon-settings-subnote">{i18n.tr('Verktøy AI-en tok i bruk', 'Tools the AI invoked')}</p>
                    <div class="verevon-trust-chips">
                      <For each={loaded().workspaceActivity.tools}>
                        {(tool) => (
                          <span class="verevon-trust-chip verevon-trust-chip--tool">
                            <Sparkles size={12} aria-hidden="true" />
                            {tool}
                          </span>
                        )}
                      </For>
                    </div>
                  </Show>
                  <p class="verevon-settings-subnote">
                    {i18n.tr(
                      'Talt på tvers av hele arbeidsområdet — attribusjon per tilkobling vises i tabellen under kun der en hendelse kunne knyttes pålitelig til en kilde.',
                      'Counted across the whole workspace — per-connection attribution is shown in the table below only where an event could be reliably attributed.',
                    )}
                  </p>
                </Show>
              </Show>
            </section>

            <Show
              when={connections().length > 0}
              fallback={
                <div class="verevon-settings-list-card">
                  <p class="verevon-settings-empty-row">
                    {i18n.tr('Ingen apper er koblet til ennå. Koble til en kilde under Integrasjoner for å se den her.', 'No apps are connected yet. Connect a source under Integrations to see it here.')}
                  </p>
                </div>
              }
            >
            <div class="verevon-trust-table" role="table" aria-label={i18n.tr('Åpenhet om tilkoblede apper', 'Connected app transparency')}>
              <div class="verevon-trust-row verevon-trust-row--head" role="row">
                <span role="columnheader">{i18n.tr('App', 'App')}</span>
                <span role="columnheader">{i18n.tr('Tillatelser', 'Permissions')}</span>
                <span role="columnheader">{i18n.tr('Data hentet', 'Data fetched')}</span>
                <span role="columnheader">{i18n.tr('Brukt av AI?', 'Used by AI?')}</span>
                <span role="columnheader">{i18n.tr('Retensjon', 'Retention')}</span>
                <span role="columnheader">{i18n.tr('Siste synk', 'Last sync')}</span>
                <span role="columnheader" class="verevon-trust-cell--actions">
                  <span class="verevon-trust-sr">{i18n.tr('Koble fra', 'Disconnect')}</span>
                </span>
              </div>

              <For each={connections()}>
                {(connection) => {
                  const permissions = () => permissionLabels(loaded(), connection)
                  const summary = () => toolActionFor(loaded(), connection)
                  const usedByAi = () => Boolean(summary())
                  return (
                    <div class="verevon-trust-row" role="row">
                      <span class="verevon-trust-cell verevon-trust-cell--app" role="cell">
                        <strong>{appName(loaded(), connection)}</strong>
                        <small>{connection.status}</small>
                      </span>

                      <span class="verevon-trust-cell" role="cell">
                        <Show
                          when={permissions().length > 0}
                          fallback={<span class="verevon-trust-muted">{i18n.tr('Ikke rapportert', 'Not reported')}</span>}
                        >
                          <span class="verevon-trust-chips">
                            <For each={permissions()}>
                              {(scope) => <span class="verevon-trust-chip">{scope}</span>}
                            </For>
                          </span>
                        </Show>
                      </span>

                      <span class="verevon-trust-cell" role="cell">
                        <Show
                          when={(summary()?.dataCategories.length ?? 0) > 0}
                          fallback={
                            <span class="verevon-trust-muted">
                              {loaded().auditAvailable ? i18n.tr('Ingen registrert', 'None recorded') : i18n.tr('Telemetri avventes', 'Telemetry pending')}
                            </span>
                          }
                        >
                          <span class="verevon-trust-chips">
                            <For each={summary()?.dataCategories ?? []}>
                              {(category) => (
                                <span class="verevon-trust-chip verevon-trust-chip--data">
                                  <Database size={12} aria-hidden="true" />
                                  {category}
                                </span>
                              )}
                            </For>
                          </span>
                        </Show>
                      </span>

                      <span class="verevon-trust-cell" role="cell">
                        <Show
                          when={usedByAi()}
                          fallback={
                            <span
                              class="verevon-trust-pill verevon-trust-pill--idle"
                              title={i18n.tr(
                                'Ingen verktøyshendelse kunne knyttes til denne tilkoblingen. Attribusjon per tilkobling er ennå ikke pålitelig, så dette er ikke en påstand om at AI-en aldri har brukt den.',
                                'No tool-action event could be attributed to this connection. Per-connection attribution is not yet reliable, so this is not a claim that the AI has never used it.',
                              )}
                            >
                              {i18n.tr('Attribusjon utilgjengelig', 'Attribution unavailable')}
                            </span>
                          }
                        >
                          <span class="verevon-trust-pill verevon-trust-pill--active">
                            <Sparkles size={12} aria-hidden="true" />
                            {i18n.tr('Ja', 'Yes')}
                          </span>
                        </Show>
                      </span>

                      <span class="verevon-trust-cell verevon-trust-muted" role="cell">
                        {retentionLabel(connection, i18n)}
                      </span>

                      <span class="verevon-trust-cell verevon-trust-muted" role="cell">
                        {formatLastSync(connection.lastSyncAt, i18n)}
                      </span>

                      <span class="verevon-trust-cell verevon-trust-cell--actions" role="cell">
                        <SettingsButton
                          settingsSize="sm"
                          danger
                          disabled={busyConnectionId() === connection.id}
                          onClick={() => void handleDisconnect(connection)}
                          aria-label={`${i18n.tr('Koble fra', 'Disconnect')} ${appName(loaded(), connection)}`}
                        >
                          <Show
                            when={busyConnectionId() === connection.id}
                            fallback={<><Unplug size={14} aria-hidden="true" /> {i18n.tr('Koble fra', 'Disconnect')}</>}
                          >
                            <Loader2 size={14} class="verevon-trust-spin" aria-hidden="true" />
                            {i18n.tr('Kobler fra…', 'Disconnecting…')}
                          </Show>
                        </SettingsButton>
                      </span>
                    </div>
                  )
                }}
              </For>
            </div>

            <p class="verevon-settings-subnote">
              <ShieldCheck size={14} aria-hidden="true" /> {i18n.tr(
                'Å koble fra en app tilbakekaller tilgangen og sletter mellomlagrede data. "Ja" under "Brukt av AI?" betyr at en verktøyshendelse ble knyttet til denne tilkoblingen; "Attribusjon utilgjengelig" betyr at ingen kunne knyttes — ikke at AI-en aldri har brukt den.',
                'Disconnecting an app revokes its access and purges its cached data. "Yes" under "Used by AI?" means a tool-action event was attributed to this connection; "Attribution unavailable" means none could be — not that the AI has never used it.',
              )}
            </p>
          </Show>
          </>
        )}
      </Show>
    </>
  )
}
