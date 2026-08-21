import { Loader2, Plug, Plus, ShieldCheck, Trash2, Users, X } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  connectMcpServer,
  deleteMcpServer,
  listMcpServers,
  shareMcpServer,
  type McpServer,
} from '@/shared/api/mcp-client'
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
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={(event) => void handleConnect(event)}>
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
              La stå tom for å tillate alle oppdagede verktøy.
            </span>
          </label>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
              <Show
                when={submitting()}
                fallback={<><Plug size={14} aria-hidden="true" /> Koble til</>}
              >
                <Loader2 size={14} aria-hidden="true" /> Kobler til…
              </Show>
            </SettingsButton>{' '}
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
