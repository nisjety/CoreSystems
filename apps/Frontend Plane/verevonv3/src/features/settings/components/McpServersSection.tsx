import { Loader2, Plug, Plus, RefreshCw, Search, ShieldCheck, Trash2, Users, X } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  connectMcpServer,
  deleteMcpServer,
  listMcpServers,
  probeMcpServer,
  shareMcpServer,
  type McpProbeAuth,
  type McpProbedTool,
  type McpServer,
} from '@/shared/api/mcp-client'
import { ApiError } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * MCP-servere — koble til og administrer eksterne MCP-tjenere agenten kan
 * bruke.
 *
 * Én tilkoblingsflyt for alt: brukeren oppgir bare navn og URL (pluss
 * valgfri delt hemmelighet / tillatte verktøy / synlighet). Verevon oppdager
 * selv om tjeneren krever OAuth 2.1-innlogging eller virker direkte — det
 * finnes ikke noe transport- eller autentiseringsvalg å ta stilling til.
 */

function parseAllowlist(raw: string): string[] {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
}

function parseUserIds(raw: string): string[] {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
}

type ServerScope = 'Privat' | 'Delt' | 'Org'

function scopeLabel(server: McpServer): ServerScope {
  if (server.scope === 'org') return 'Org'
  return server.shared_with.length > 0 ? 'Delt' : 'Privat'
}

/** The most tools the picker shows. The probe reports no more than this. */
const MCP_PROBE_TOOL_LIMIT = 64

/** The tool names `tool_allowlist` accepts: letters, digits, `_`, `.`, `:` or `-`, at most 128 bytes. */
const REGISTRABLE_TOOL_NAME = /^[A-Za-z0-9_.:-]{1,128}$/

/** `VITE_MCP_PROBE === 'true'`: the add flow probes a server and offers a tool picker. Off, the form is unchanged. */
function mcpProbeEnabled(): boolean {
  return import.meta.env.VITE_MCP_PROBE === 'true'
}

/** `VITE_CUSTOM_CONNECTORS === 'true'`: a server that needs a static token is routed to the stored-token flow. */
function customConnectorsEnabled(): boolean {
  return import.meta.env.VITE_CUSTOM_CONNECTORS === 'true'
}

/**
 * Whether a probed tool must stay unticked until a person picks it. Under the MCP defaults a tool is destructive
 * unless it says otherwise: a read-only hint settles it, and a destructive hint of `false` is the only other way
 * to be safe. A tool the server gave no hint for therefore starts unticked.
 */
function isDestructiveTool(tool: McpProbedTool): boolean {
  if (tool.read_only_hint === true) return false
  return tool.destructive_hint !== false
}

/** A probed tool that may be ticked up front: it is not destructive, and the registry accepts its name. */
function preselectable(tool: McpProbedTool): boolean {
  return !isDestructiveTool(tool) && REGISTRABLE_TOOL_NAME.test(tool.name)
}

/** The hint the server stated for a tool, in words a person can judge by. */
function toolHintLabel(tool: McpProbedTool): string {
  if (tool.read_only_hint === true) return 'Bare lesing'
  if (tool.destructive_hint === true) return 'Destruktiv'
  if (tool.destructive_hint === false) return 'Ikke destruktiv'
  return 'Ingen merknad'
}

/** Each tool name once, in the server's order, and no more than the picker shows. */
function distinctTools(tools: McpProbedTool[]): McpProbedTool[] {
  const seen = new Set<string>()
  const unique: McpProbedTool[] = []
  for (const tool of tools) {
    if (seen.has(tool.name)) continue
    seen.add(tool.name)
    unique.push(tool)
  }
  return unique.slice(0, MCP_PROBE_TOOL_LIMIT)
}

/**
 * The message for a failed probe. A missing route means the probe is off in this environment; a 400 means the
 * address is not one the registry accepts. Anything else is the typed copy for the code the gateway sent.
 */
function probeFailureMessage(err: unknown, tr: (noText: string, enText: string) => string): string {
  if (err instanceof ApiError && err.status === 404) {
    return tr(
      'Sjekken av MCP-tjenere er ikke slått på i dette miljøet.',
      'Checking MCP servers is not turned on in this environment.',
    )
  }
  if (err instanceof ApiError && err.status === 400) {
    return tr(
      'Adressen må være en offentlig HTTPS-adresse uten påloggingsdata, søkeparametere eller fragment.',
      'The address must be a public HTTPS address with no login details, query or fragment.',
    )
  }
  return translateApiError(err, tr, { no: 'Kunne ikke sjekke tjeneren.', en: 'Could not check the server.' })
}

export function McpServersSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const currentUserId = createMemo(() => session.user?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [servers, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listMcpServers(id).then((result) => result.servers),
  )

  const [showAddForm, setShowAddForm] = createSignal(false)

  // Connect form state — one shared field set for every server, whether it
  // turns out to need OAuth or not; the caller never picks which.
  const [name, setName] = createSignal('')
  const [url, setUrl] = createSignal('')
  const [token, setToken] = createSignal('')
  const [allowlist, setAllowlist] = createSignal('')
  const [scope, setScope] = createSignal<'user' | 'org'>('user')

  // The probe-first add flow (VITE_MCP_PROBE). `probed` is the last answer, kept with the address it was
  // for, so an address that has since changed is never judged by another server's answer. `picks` holds the
  // tools ticked for a server that needs no credential.
  const probeEnabled = mcpProbeEnabled()
  const [probed, setProbed] = createSignal<{ url: string; auth: McpProbeAuth; tools: McpProbedTool[] } | null>(null)
  const [probing, setProbing] = createSignal(false)
  const [picks, setPicks] = createSignal<string[]>([])
  const probeCurrent = createMemo(() => {
    const result = probed()
    return result && result.url === url().trim() ? result : null
  })
  const probeAuth = createMemo<McpProbeAuth | null>(() => probeCurrent()?.auth ?? null)
  const probeTools = createMemo(() => probeCurrent()?.tools ?? [])

  const [submitting, setSubmitting] = createSignal(false)
  const [busyServerId, setBusyServerId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  // Per-server share state: the editing server id and its comma-separated ids.
  const [shareServerId, setShareServerId] = createSignal<string | null>(null)
  const [shareUserIds, setShareUserIds] = createSignal('')

  // Set once, from the `mcp_oauth` query param the callback redirect lands
  // with — never re-derived, so a later refresh of this page doesn't re-show
  // a stale banner for a connection attempt that already resolved.
  const [connectionNotice, setConnectionNotice] = createSignal<'connected' | 'error' | null>(null)

  createEffect(
    () => undefined,
    () => {
      if (typeof window === 'undefined') return
      const params = new URLSearchParams(window.location.search)
      const notice = params.get('mcp_oauth')
      if (notice !== 'connected' && notice !== 'error') return
      setConnectionNotice(notice)
      params.delete('mcp_oauth')
      params.delete('server_id')
      const rest = params.toString()
      window.history.replaceState(
        null,
        '',
        window.location.pathname + (rest ? `?${rest}` : '') + window.location.hash,
      )
      if (notice === 'connected') void refetch()
    },
  )

  const list = createMemo(() => servers() ?? [])

  const canModify = (server: McpServer): boolean => {
    if (server.scope === 'org') return isAdmin()
    return server.owner_user_id === currentUserId()
  }
  const canShare = (server: McpServer): boolean =>
    server.scope === 'user' && server.owner_user_id === currentUserId()

  const resetForm = () => {
    setName('')
    setUrl('')
    setToken('')
    setAllowlist('')
    setScope('user')
    setFormError(null)
    setProbed(null)
    setPicks([])
  }

  const openAddForm = () => {
    resetForm()
    setShowAddForm(true)
  }

  const closeAddForm = () => {
    setShowAddForm(false)
    resetForm()
  }

  const openShare = (server: McpServer) => {
    setActionError(null)
    setShareServerId(server.server_id)
    setShareUserIds(server.shared_with.join(', '))
  }

  const closeShare = () => {
    setShareServerId(null)
    setShareUserIds('')
  }

  const handleShare = async (server: McpServer) => {
    const id = orgId()
    if (!id || busyServerId()) return

    setBusyServerId(server.server_id)
    setActionError(null)
    try {
      await shareMcpServer(id, server.server_id, parseUserIds(shareUserIds()))
      closeShare()
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke oppdatere delingen.', en: 'Could not update the sharing.' }),
      )
    } finally {
      setBusyServerId(null)
    }
  }

  // The one connect flow: Verevon discovers whether the server needs OAuth
  // 2.1 login or works directly. On the OAuth branch this navigates the
  // browser away entirely — the connection completes (or fails) on the
  // server's own consent screen and lands back here via the callback
  // redirect, picked up by the onMount check above. On the direct branch the
  // server is already registered by the time this returns.
  const handleConnect = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedUrl = url().trim()
    if (!trimmedName || !trimmedUrl) {
      setFormError('Navn og URL er påkrevd.')
      return
    }
    if (!/^https:\/\//i.test(trimmedUrl)) {
      setFormError('URL må starte med https:// — en offentlig tjener Verevon kan oppdage og koble til.')
      return
    }

    setSubmitting(true)
    setFormError(null)
    try {
      const tools = parseAllowlist(allowlist())
      // Non-admins can never request org scope; the gateway also enforces this.
      const effectiveScope = scope() === 'org' && isAdmin() ? 'org' : 'user'
      const result = await connectMcpServer(id, {
        name: trimmedName,
        url: trimmedUrl,
        token: token().trim() || undefined,
        tool_allowlist: tools.length > 0 ? tools : undefined,
        scope: effectiveScope,
      })
      if (result.needs_oauth) {
        window.location.href = result.authorization_url
        return
      }
      closeAddForm()
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke koble til MCP-tjeneren.', en: 'Could not connect the MCP server.' }),
      )
      setSubmitting(false)
    }
  }

  // Step one of the probe-first flow: ask what the address needs. Nothing is saved by a probe.
  const handleProbe = async () => {
    if (probing() || submitting()) return
    const trimmedName = name().trim()
    const trimmedUrl = url().trim()
    if (!trimmedName || !trimmedUrl) {
      setFormError('Navn og URL er påkrevd.')
      return
    }
    if (!/^https:\/\//i.test(trimmedUrl)) {
      setFormError('URL må starte med https:// — en offentlig tjener Verevon kan oppdage og koble til.')
      return
    }

    setProbing(true)
    setFormError(null)
    try {
      const result = await probeMcpServer(trimmedUrl)
      const tools = distinctTools(result.tools)
      setProbed({ url: trimmedUrl, auth: result.auth, tools })
      // Nothing is ticked for a server that needs a credential: its tools are only known after sign-in.
      setPicks(result.auth === 'none' ? tools.filter(preselectable).map((tool) => tool.name) : [])
    } catch (err) {
      setProbed(null)
      setFormError(probeFailureMessage(err, i18n.tr))
    } finally {
      setProbing(false)
    }
  }

  const togglePick = (toolName: string, on: boolean) =>
    setPicks((current) => (on ? [...new Set([...current, toolName])] : current.filter((tool) => tool !== toolName)))

  // Registers the probed server through the existing connect call. `toolNames` is the picked set for a server
  // that needs no credential, or undefined for an OAuth server, which then takes the typed allowlist or every
  // tool its sign-in exposes. The connect call still sends a server that turns out to need OAuth to its
  // consent screen.
  const connectProbed = async (toolNames: string[] | undefined) => {
    const id = orgId()
    const current = probeCurrent()
    if (!id || submitting() || !current) return
    // A static-token server is never registered from here, and a server that needs no credential needs a pick.
    if (current.auth === 'bearer_required') return
    if (current.auth === 'none' && (!toolNames || toolNames.length === 0)) return

    const trimmedName = name().trim()
    if (!trimmedName) {
      setFormError('Navn og URL er påkrevd.')
      return
    }

    setSubmitting(true)
    setFormError(null)
    try {
      // Non-admins can never request org scope; the gateway also enforces this.
      const effectiveScope = scope() === 'org' && isAdmin() ? 'org' : 'user'
      const result = await connectMcpServer(id, {
        name: trimmedName,
        url: current.url,
        tool_allowlist: toolNames,
        scope: effectiveScope,
      })
      if (result.needs_oauth) {
        window.location.href = result.authorization_url
        return
      }
      setSubmitting(false)
      closeAddForm()
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke koble til MCP-tjeneren.', en: 'Could not connect the MCP server.' }),
      )
      setSubmitting(false)
    }
  }

  const handleDelete = async (server: McpServer) => {
    const id = orgId()
    if (!id || busyServerId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(`Fjerne MCP-tjeneren «${server.name}»?`)
    ) {
      return
    }

    setBusyServerId(server.server_id)
    setActionError(null)
    try {
      await deleteMcpServer(id, server.server_id)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke fjerne MCP-tjeneren.', en: 'Could not remove the MCP server.' }),
      )
    } finally {
      setBusyServerId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="MCP-servere"
        description="Eksterne MCP-tjenere agenten kan bruke. Verevon oppdager selv om en tjener krever OAuth-innlogging eller virker direkte — du oppgir bare navn og URL."
      />

      <div>
        <SettingsButton
          settingsSize="sm"
          variant="primary"
          onClick={() => (showAddForm() ? closeAddForm() : openAddForm())}
        >
          <Show when={showAddForm()} fallback={<><Plus size={14} aria-hidden="true" /> Legg til tjener</>}>
            <X size={14} aria-hidden="true" /> Lukk
          </Show>
        </SettingsButton>
      </div>

      <Show when={connectionNotice() === 'connected'}>
        <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">
          Tilkoblingen ble fullført. Den nye tjeneren vises i listen under.
        </p>
      </Show>
      <Show when={connectionNotice() === 'error'}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Tilkoblingen kunne ikke fullføres. Prøv igjen, eller kontroller at URL-en er riktig.
        </p>
      </Show>

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={showAddForm()}>
        <form
          class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
          onSubmit={(event) => {
            if (!probeEnabled) {
              void handleConnect(event)
              return
            }
            // With the probe on, Enter only runs the probe. Registering is always a button press, so a key press
            // can never save a server with the tools ticked by default.
            event.preventDefault()
            if (probeAuth() === null) void handleProbe()
          }}
        >
          <label for="mcp-name" class="verevon-settings-field">
            <span class="verevon-settings-label">Navn</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="mcp-name"
                value={name()}
                required
                placeholder="f.eks. visma-net"
                onInput={(event) => setName(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="mcp-url" class="verevon-settings-field">
            <span class="verevon-settings-label">URL</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="mcp-url"
                value={url()}
                required
                placeholder="https://mcp.finance.visma.net/mcp"
                onInput={(event) => setUrl(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
            <span class="verevon-settings-help">
              Offentlig HTTPS-tjener. Verevon oppdager selv om den krever OAuth-innlogging.
            </span>
          </label>

          <label for="mcp-scope" class="verevon-settings-field">
            <span class="verevon-settings-label">Synlighet</span>
            <span class="verevon-settings-input-wrap">
              <select
                id="mcp-scope"
                value={scope()}
                onChange={(event) =>
                  setScope(event.currentTarget.value === 'org' ? 'org' : 'user')
                }
                class="verevon-settings-input verevon-settings-select"
              >
                <option value="user">Privat (bare meg)</option>
                <Show when={isAdmin()}>
                  <option value="org">Hele organisasjonen</option>
                </Show>
              </select>
            </span>
            <span class="verevon-settings-help">
              Bare administratorer kan opprette org-dekkende tjenere. Andre beholder
              sine private og kan dele dem med bestemte personer.
            </span>
          </label>

          {/* With the probe on, the shared secret is not asked for: a server that needs one is routed elsewhere. */}
          <Show when={!probeEnabled}>
            <label for="mcp-token" class="verevon-settings-field">
              <span class="verevon-settings-label">Delt hemmelighet (valgfritt)</span>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="mcp-token"
                  type="password"
                  value={token()}
                  autocomplete="off"
                  placeholder="Kun hvis tjeneren ber om én, og ikke støtter innlogging"
                  onInput={(event) => setToken(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
              </span>
              <span class="verevon-settings-help">
                La stå tom med mindre tjeneren krever en delt token og ikke støtter ekte
                OAuth-innlogging. Lagres hos model-gateway og vises aldri igjen.
              </span>
            </label>
          </Show>

          {/* The probe's answer (VITE_MCP_PROBE): the auth requirement, then the tool picker for a server that needs no credential. */}
          <Show when={probeEnabled && probeAuth() === 'oauth'}>
            <p class="verevon-settings-panel-note" role="status">
              Tjeneren krever OAuth-innlogging. Verevon sender deg til innloggingssiden, og verktøyene vises først
              etter innloggingen. Skriv inn navnene under for å begrense dem.
            </p>
          </Show>
          <Show when={probeEnabled && probeAuth() === 'bearer_required'}>
            <p class="verevon-settings-panel-note" role="status">
              Tjeneren krever en statisk token.{' '}
              <Show
                when={customConnectorsEnabled()}
                fallback={<>Verevon kan ikke koble til slike tjenere herfra ennå.</>}
              >
                Legg den til under egendefinerte koblinger, der tokenet lagres i stedet for i dette skjemaet.
              </Show>
            </p>
          </Show>
          <Show when={probeEnabled && probeAuth() === 'none'}>
            <div
              class="verevon-settings-field-grid verevon-settings-field-grid--spaced"
              role="group"
              aria-labelledby="mcp-tools-title"
            >
              <div class="verevon-settings-field-span">
                <p id="mcp-tools-title" class="verevon-settings-label">
                  Verktøy tjeneren tilbyr ({probeTools().length})
                </p>
                <Show
                  when={probeTools().length > 0}
                  fallback={
                    <p class="verevon-settings-help">
                      Tjeneren tilbyr ingen verktøy, så den kan ikke kobles til ennå.
                    </p>
                  }
                >
                  <p class="verevon-settings-help">
                    Verktøy som kan endre data, og verktøy tjeneren ikke har merket, er ikke valgt på forhånd.
                  </p>
                </Show>
              </div>
              <For each={probeTools()}>
                {(tool) => (
                  <label class="verevon-settings-field">
                    <span class="verevon-settings-label">
                      <input
                        type="checkbox"
                        checked={picks().includes(tool.name)}
                        disabled={!REGISTRABLE_TOOL_NAME.test(tool.name)}
                        onChange={(event) => togglePick(tool.name, event.currentTarget.checked)}
                      />{' '}
                      {tool.name}{' '}
                      <span class="verevon-trust-chip">{toolHintLabel(tool)}</span>
                    </span>
                    <span class="verevon-settings-help">
                      {tool.description || 'Ingen beskrivelse.'}
                      <Show when={!REGISTRABLE_TOOL_NAME.test(tool.name)}>
                        {' '}Navnet kan ikke brukes i en tillatelsesliste.
                      </Show>
                    </span>
                  </label>
                )}
              </For>
            </div>
          </Show>

          {/* Off: the typed allowlist, as before. With the probe on: only for an OAuth server, which has no tool list yet. */}
          <Show when={!probeEnabled || probeAuth() === 'oauth'}>
            <label for="mcp-allowlist" class="verevon-settings-field">
              <span class="verevon-settings-label">Tillatte verktøy (valgfritt)</span>
              <span class="verevon-settings-input-wrap">
                <VerevonInput
                  id="mcp-allowlist"
                  value={allowlist()}
                  placeholder="kommaseparert, f.eks. get_skill, execute_query"
                  onInput={(event) => setAllowlist(event.currentTarget.value)}
                  class="verevon-settings-input"
                />
              </span>
              <span class="verevon-settings-help">
                {probeEnabled
                  ? 'La stå tom for å tillate alle verktøyene tjeneren tilbyr etter innlogging.'
                  : 'La stå tom for å tillate alle oppdagede verktøy.'}
              </span>
            </label>
          </Show>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <Show when={!probeEnabled}>
              <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
                <Show
                  when={submitting()}
                  fallback={<><Plug size={14} aria-hidden="true" /> Koble til</>}
                >
                  <Loader2 size={14} aria-hidden="true" /> Kobler til…
                </Show>
              </SettingsButton>{' '}
            </Show>
            <Show when={probeEnabled && probeAuth() === null}>
              <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={probing()}>
                <Show
                  when={probing()}
                  fallback={<><Search size={14} aria-hidden="true" /> Sjekk tjener</>}
                >
                  <Loader2 size={14} aria-hidden="true" /> Sjekker…
                </Show>
              </SettingsButton>{' '}
            </Show>
            <Show when={probeEnabled && probeAuth() === 'oauth'}>
              <SettingsButton
                type="button"
                variant="primary"
                settingsSize="sm"
                disabled={submitting()}
                onClick={() => {
                  const typed = parseAllowlist(allowlist())
                  void connectProbed(typed.length > 0 ? typed : undefined)
                }}
              >
                <Show
                  when={submitting()}
                  fallback={<><Plug size={14} aria-hidden="true" /> Koble til</>}
                >
                  <Loader2 size={14} aria-hidden="true" /> Kobler til…
                </Show>
              </SettingsButton>{' '}
            </Show>
            <Show when={probeEnabled && probeAuth() === 'none'}>
              <SettingsButton
                type="button"
                variant="primary"
                settingsSize="sm"
                disabled={submitting() || picks().length === 0}
                onClick={() => void connectProbed(picks())}
              >
                <Show
                  when={submitting()}
                  fallback={
                    <>
                      <Plug size={14} aria-hidden="true" />{' '}
                      {picks().length > 0 ? `Koble til med ${picks().length} verktøy` : 'Velg minst ett verktøy'}
                    </>
                  }
                >
                  <Loader2 size={14} aria-hidden="true" /> Kobler til…
                </Show>
              </SettingsButton>{' '}
            </Show>
            <Show when={probeEnabled && probeAuth() !== null}>
              <SettingsButton
                type="button"
                settingsSize="sm"
                disabled={probing() || submitting()}
                onClick={() => void handleProbe()}
              >
                <RefreshCw size={14} aria-hidden="true" /> Sjekk på nytt
              </SettingsButton>{' '}
            </Show>
            <SettingsButton settingsSize="sm" disabled={submitting()} onClick={closeAddForm}>
              Avbryt
            </SettingsButton>
          </div>
        </form>
      </Show>

      <Show when={servers.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste MCP-tjenere.
        </p>
      </Show>

      <Show
        when={!servers.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            Laster MCP-tjenere…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                Ingen MCP-tjenere er koblet til ennå. Trykk «Legg til tjener» for å koble til en.
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={list()}>
              {(server) => (
                <div class="verevon-settings-integration-row">
                  <div>
                    <p>
                      {server.name}{' '}
                      <span class="verevon-trust-chip" aria-label={`Synlighet: ${scopeLabel(server)}`}>
                        {scopeLabel(server)}
                      </span>
                    </p>
                    <span>
                      {server.url} · {server.enabled ? 'aktiv' : 'deaktivert'}
                      <Show when={server.scope === 'user' && server.shared_with.length > 0}>
                        {' '}· delt med {server.shared_with.length}
                      </Show>
                    </span>
                    <Show when={server.tool_allowlist.length > 0}>
                      <span class="verevon-trust-chips">
                        <For each={server.tool_allowlist}>
                          {(tool) => <span class="verevon-trust-chip">{tool}</span>}
                        </For>
                      </span>
                    </Show>
                    <Show when={shareServerId() === server.server_id}>
                      <div class="verevon-settings-field-grid verevon-settings-field-grid--spaced">
                        <label for={`mcp-share-${server.server_id}`} class="verevon-settings-field">
                          <span class="verevon-settings-label">Del med (bruker-ID-er)</span>
                          <span class="verevon-settings-input-wrap">
                            <VerevonInput
                              id={`mcp-share-${server.server_id}`}
                              value={shareUserIds()}
                              placeholder="kommaseparerte bruker-ID-er"
                              onInput={(event) => setShareUserIds(event.currentTarget.value)}
                              class="verevon-settings-input"
                            />
                          </span>
                          <span class="verevon-settings-help">
                            La stå tom for å fjerne all deling. Erstatter hele delingslisten.
                          </span>
                        </label>
                        <div>
                          <SettingsButton
                            settingsSize="sm"
                            variant="primary"
                            disabled={busyServerId() === server.server_id}
                            onClick={() => void handleShare(server)}
                          >
                            <Show
                              when={busyServerId() === server.server_id}
                              fallback={<>Lagre deling</>}
                            >
                              <Loader2 size={14} aria-hidden="true" /> Lagrer…
                            </Show>
                          </SettingsButton>{' '}
                          <SettingsButton
                            settingsSize="sm"
                            disabled={busyServerId() === server.server_id}
                            onClick={closeShare}
                          >
                            Avbryt
                          </SettingsButton>
                        </div>
                      </div>
                    </Show>
                  </div>
                  <div>
                    <Show when={canShare(server) && shareServerId() !== server.server_id}>
                      <SettingsButton
                        settingsSize="sm"
                        disabled={busyServerId() === server.server_id}
                        onClick={() => openShare(server)}
                        aria-label={`Del ${server.name}`}
                      >
                        <Users size={14} aria-hidden="true" /> Del
                      </SettingsButton>{' '}
                    </Show>
                    <Show when={canModify(server)}>
                      <SettingsButton
                        settingsSize="sm"
                        danger
                        disabled={busyServerId() === server.server_id}
                        onClick={() => void handleDelete(server)}
                        aria-label={`Fjern ${server.name}`}
                      >
                        <Show
                          when={busyServerId() === server.server_id}
                          fallback={<><Trash2 size={14} aria-hidden="true" /> Fjern</>}
                        >
                          <Loader2 size={14} aria-hidden="true" /> Fjerner…
                        </Show>
                      </SettingsButton>
                    </Show>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <p class="verevon-settings-subnote">
        <ShieldCheck size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten — den sendes aldri fra nettleseren.
      </p>
    </>
  )
}
