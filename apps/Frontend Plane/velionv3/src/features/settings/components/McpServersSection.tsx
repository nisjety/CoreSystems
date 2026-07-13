import { Loader2, Plug, ShieldCheck, Trash2, Users } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  deleteMcpServer,
  listMcpServers,
  registerMcpServer,
  shareMcpServer,
  type McpServer,
} from '@/shared/api/mcp-client'
import { ApiError } from '@/shared/api/http'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * MCP-servere — registrer og administrer eksterne MCP-tjenere agenten kan bruke.
 *
 * Lister registrerte MCP-tjenere (navn, URL, transport, status og tillatte
 * verktøy) og lar en admin registrere nye eller fjerne eksisterende. Den
 * hemmelige `token`-en sendes kun ved registrering — model-gateway utelater den
 * alltid fra svar, så den vises aldri her.
 */

const TRANSPORT_OPTIONS = ['stdio', 'http', 'sse'] as const

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
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const currentUserId = createMemo(() => session.user?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [servers, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listMcpServers(id).then((result) => result.servers),
  )

  // Registration form state.
  const [name, setName] = createSignal('')
  const [url, setUrl] = createSignal('')
  const [transport, setTransport] = createSignal<string>('stdio')
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
    setTransport('stdio')
    setToken('')
    setAllowlist('')
    setScope('user')
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
        err instanceof ApiError ? err.message : 'Kunne ikke oppdatere delingen.',
      )
    } finally {
      setBusyServerId(null)
    }
  }

  const handleRegister = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedUrl = url().trim()
    if (!trimmedName || !trimmedUrl) {
      setFormError('Navn og URL er påkrevd.')
      return
    }

    // Transport and URL scheme MUST agree, or discovery silently fails and the
    // server registers but exposes zero tools. The most common mistake is
    // leaving transport on the default «stdio» while pasting an https:// URL:
    // model-gateway then tries to spawn that URL as a local command
    // (parse_stdio_command requires a stdio:// scheme), discovery errors, the
    // empty allowlist enumerates nothing, and the UI shows the server as active
    // with no working tools. Catch it here with a clear, actionable message.
    const activeTransport = transport()
    if (activeTransport === 'stdio' && !trimmedUrl.startsWith('stdio://')) {
      setFormError(
        'For «stdio»-transport må URL være en stdio://-kommando (f.eks. ' +
          'stdio:///usr/local/bin/mcp-server --flag). Skal du koble til en ' +
          'ekstern HTTPS-tjener, velg transport «http» i stedet.',
      )
      return
    }
    if (
      (activeTransport === 'http' || activeTransport === 'sse') &&
      !/^https?:\/\//i.test(trimmedUrl)
    ) {
      setFormError(
        `For «${activeTransport}»-transport må URL starte med http:// eller https://.`,
      )
      return
    }

    setSubmitting(true)
    setFormError(null)
    try {
      const tools = parseAllowlist(allowlist())
      // Non-admins can never request org scope; the gateway also enforces this.
      const effectiveScope = scope() === 'org' && isAdmin() ? 'org' : 'user'
      await registerMcpServer(id, {
        name: trimmedName,
        url: trimmedUrl,
        transport: transport(),
        token: token().trim() || undefined,
        tool_allowlist: tools.length > 0 ? tools : undefined,
        enabled: true,
        scope: effectiveScope,
      })
      resetForm()
      await refetch()
    } catch (err) {
      setFormError(
        err instanceof ApiError ? err.message : 'Kunne ikke registrere MCP-tjeneren.',
      )
    } finally {
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
        err instanceof ApiError ? err.message : 'Kunne ikke fjerne MCP-tjeneren.',
      )
    } finally {
      setBusyServerId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="MCP-servere"
        description="Registrer eksterne MCP-tjenere agenten kan bruke, og styr hvilke verktøy som er tillatt. Den hemmelige tokenen sendes kun ved registrering og vises aldri her."
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={servers.error}>
        <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
          Kunne ikke laste MCP-tjenere.
        </p>
      </Show>

      <Show
        when={!servers.loading}
        fallback={
          <p class="velion-settings-subnote" role="status" aria-busy="true">
            Laster MCP-tjenere…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="velion-settings-list-card">
              <p class="velion-settings-empty-row">
                Ingen MCP-tjenere er registrert ennå. Legg til en nedenfor.
              </p>
            </div>
          }
        >
          <div class="velion-settings-list-card">
            <For each={list()}>
              {(server) => (
                <div class="velion-settings-integration-row">
                  <div>
                    <p>
                      {server.name}{' '}
                      <span class="velion-trust-chip" aria-label={`Synlighet: ${scopeLabel(server)}`}>
                        {scopeLabel(server)}
                      </span>
                    </p>
                    <span>
                      {server.url} · {server.transport} ·{' '}
                      {server.enabled ? 'aktiv' : 'deaktivert'}
                      <Show when={server.scope === 'user' && server.shared_with.length > 0}>
                        {' '}· delt med {server.shared_with.length}
                      </Show>
                    </span>
                    <Show when={server.tool_allowlist.length > 0}>
                      <span class="velion-trust-chips">
                        <For each={server.tool_allowlist}>
                          {(tool) => <span class="velion-trust-chip">{tool}</span>}
                        </For>
                      </span>
                    </Show>
                    <Show when={shareServerId() === server.server_id}>
                      <div class="velion-settings-field-grid velion-settings-field-grid--spaced">
                        <label for={`mcp-share-${server.server_id}`} class="velion-settings-field">
                          <span class="velion-settings-label">Del med (bruker-ID-er)</span>
                          <span class="velion-settings-input-wrap">
                            <VelionInput
                              id={`mcp-share-${server.server_id}`}
                              value={shareUserIds()}
                              placeholder="kommaseparerte bruker-ID-er"
                              onInput={(event) => setShareUserIds(event.currentTarget.value)}
                              class="velion-settings-input"
                            />
                          </span>
                          <span class="velion-settings-help">
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

      <form class="velion-settings-field-grid velion-settings-field-grid--spaced" onSubmit={handleRegister}>
        <label for="mcp-name" class="velion-settings-field">
          <span class="velion-settings-label">Navn</span>
          <span class="velion-settings-input-wrap">
            <VelionInput
              id="mcp-name"
              value={name()}
              required
              placeholder="f.eks. github-tools"
              onInput={(event) => setName(event.currentTarget.value)}
              class="velion-settings-input"
            />
          </span>
        </label>

        <label for="mcp-url" class="velion-settings-field">
          <span class="velion-settings-label">URL</span>
          <span class="velion-settings-input-wrap">
            <VelionInput
              id="mcp-url"
              value={url()}
              required
              placeholder="stdio:///path --flags eller https://host"
              onInput={(event) => setUrl(event.currentTarget.value)}
              class="velion-settings-input"
            />
          </span>
        </label>

        <label for="mcp-transport" class="velion-settings-field">
          <span class="velion-settings-label">Transport</span>
          <span class="velion-settings-input-wrap">
            <select
              id="mcp-transport"
              value={transport()}
              onChange={(event) => setTransport(event.currentTarget.value)}
              class="velion-settings-input velion-settings-select"
            >
              <For each={TRANSPORT_OPTIONS}>
                {(option) => <option value={option}>{option}</option>}
              </For>
            </select>
          </span>
        </label>

        <label for="mcp-scope" class="velion-settings-field">
          <span class="velion-settings-label">Synlighet</span>
          <span class="velion-settings-input-wrap">
            <select
              id="mcp-scope"
              value={scope()}
              onChange={(event) =>
                setScope(event.currentTarget.value === 'org' ? 'org' : 'user')
              }
              class="velion-settings-input velion-settings-select"
            >
              <option value="user">Privat (bare meg)</option>
              <Show when={isAdmin()}>
                <option value="org">Hele organisasjonen</option>
              </Show>
            </select>
          </span>
          <span class="velion-settings-help">
            Bare administratorer kan opprette org-dekkende tjenere. Andre beholder
            sine private og kan dele dem med bestemte personer.
          </span>
        </label>

        <label for="mcp-token" class="velion-settings-field">
          <span class="velion-settings-label">Token (hemmelig, valgfritt)</span>
          <span class="velion-settings-input-wrap">
            <VelionInput
              id="mcp-token"
              type="password"
              value={token()}
              autocomplete="off"
              placeholder="Bærer-token for autentisering"
              onInput={(event) => setToken(event.currentTarget.value)}
              class="velion-settings-input"
            />
          </span>
          <span class="velion-settings-help">
            Lagres hos model-gateway og returneres aldri til grensesnittet.
          </span>
        </label>

        <label for="mcp-allowlist" class="velion-settings-field">
          <span class="velion-settings-label">Tillatte verktøy (valgfritt)</span>
          <span class="velion-settings-input-wrap">
            <VelionInput
              id="mcp-allowlist"
              value={allowlist()}
              placeholder="kommaseparert, f.eks. read_file, list_dir"
              onInput={(event) => setAllowlist(event.currentTarget.value)}
              class="velion-settings-input"
            />
          </span>
          <span class="velion-settings-help">
            La stå tom for å tillate alle oppdagede verktøy.
          </span>
        </label>

        <Show when={formError()}>
          {(message) => (
            <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <div>
          <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
            <Show
              when={submitting()}
              fallback={<><Plug size={14} aria-hidden="true" /> Registrer MCP-server</>}
            >
              <Loader2 size={14} aria-hidden="true" /> Registrerer…
            </Show>
          </SettingsButton>
        </div>
      </form>

      <p class="velion-settings-subnote">
        <ShieldCheck size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten — den sendes aldri fra nettleseren. Tokenen behandles som
        en hemmelighet og vises ikke i listen.
      </p>
    </>
  )
}
