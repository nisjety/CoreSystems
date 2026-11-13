import { createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { useI18n } from '@/shared/i18n'
import { ApiError } from '@/shared/api/http'
import { getOrganizationAISettings } from '@/shared/api/organization-client'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import {
  createCredentialBinding,
  createCustomRestConnector,
  listCredentialBindings,
  registerStaticBearerMcpServer,
  revokeCredentialBinding,
  type CredentialBinding,
} from '@/shared/api/credential-bindings-client'

/** The methods a REST connector may be given. A new connector starts with GET only. */
const REST_METHODS = ['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'] as const

/** The headers a key may be sent in. Authorization makes a bearer binding; the others make an API-key binding. */
const REST_HEADERS = ['Authorization', 'X-Api-Key', 'Api-Key', 'X-Auth-Token'] as const

/** The methods an MCP server's binding allows: the streamable HTTP transport's POST, GET and DELETE. */
const MCP_METHODS = ['GET', 'POST', 'DELETE'] as const

/** The most path prefixes a binding may carry. */
const MAX_PREFIXES = 32

/** The most tools an MCP server may name. */
const MAX_TOOLS = 64

/** The tool names `tool_allowlist` accepts: letters, digits, `_`, `.`, `:` or `-`, at most 128 bytes. */
const TOOL_NAME = /^[A-Za-z0-9_.:-]{1,128}$/

/** A host is a bare DNS name: no scheme, port, path or userinfo. Integration Core checks the rest. */
const HOST_SHAPE = /^[a-z0-9-]+(\.[a-z0-9-]+)+$/

/** Lines or commas, trimmed, non-empty and unique. */
function parseList(raw: string): string[] {
  const items = raw
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter((item) => item !== '')
  return items.filter((item, index) => items.indexOf(item) === index)
}

/** Lines, trimmed, non-empty and unique. A path prefix may hold a comma, so only a new line separates two of them. */
function parseLines(raw: string): string[] {
  const items = raw
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter((item) => item !== '')
  return items.filter((item, index) => items.indexOf(item) === index)
}

/**
 * The host and path an MCP server's binding names. Null unless the address is https on the default port, with no
 * credentials, query, fragment or percent-encoding in its path.
 */
function mcpTarget(raw: string): { host: string; path: string } | null {
  let url: URL
  try {
    url = new URL(raw.trim())
  } catch {
    return null
  }
  if (url.protocol !== 'https:' || url.port !== '' || url.username !== '' || url.password !== '') return null
  if (url.search !== '' || url.hash !== '' || url.pathname.includes('%')) return null
  return { host: url.hostname.toLowerCase(), path: url.pathname === '' ? '/' : url.pathname }
}

/** Whether a failure is the switch being off on this gateway, which answers every route 404. */
function isDisabled(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404
}

/** Removes a binding a failed step left behind. True when the key is gone. */
async function removeBinding(bindingId: string): Promise<boolean> {
  try {
    await revokeCredentialBinding(bindingId)
    return true
  } catch {
    return false
  }
}

/**
 * The server address the MCP servers section hands to the stored-token form (W4-CC-2). The section sets it just before
 * it links here, and the next mount of the form takes it once. It lives in memory only, so a reload drops it, and the
 * address reaches neither the URL nor any storage.
 */
let pendingStoredTokenUrl: string | null = null

/** Opens the stored-token form with this server address filled in, on its next mount. */
export function setPendingStoredTokenUrl(url: string): void {
  pendingStoredTokenUrl = url
}

/** The address set by `setPendingStoredTokenUrl`, or an empty string. Taken once, so a later mount starts empty. */
function takePendingStoredTokenUrl(): string {
  const url = pendingStoredTokenUrl ?? ''
  pendingStoredTokenUrl = null
  return url
}

/**
 * Custom connectors (W4-CC-8): an organisation's stored keys and the connectors that use them. An owner or admin stores
 * a key for one host, then creates a custom REST connector that calls it, or registers an MCP server with a stored
 * token. Integration Core seals each key and the browser never reads one back; each form clears its key field as soon
 * as it is submitted. Behind `VITE_CUSTOM_CONNECTORS`, for owners and admins, and hidden for a zero-retention
 * organisation. The stored-token form opens with a server address that was handed over, when one is waiting.
 */
export function CustomConnectorSection(props: { orgId?: string | null }) {
  const i18n = useI18n()
  const enabled = import.meta.env.VITE_CUSTOM_CONNECTORS === 'true'
  const admin = hasWorkspaceAdminAccess(getSession())
  const orgId = () => props.orgId?.trim() || false
  const [settings] = createResource(orgId, async (id) => {
    try {
      return await getOrganizationAISettings(id)
    } catch {
      return undefined
    }
  })

  const [version, setVersion] = createSignal(0)
  const [stored] = createResource(
    () => (enabled && admin ? `v${version()}` : false),
    async () => {
      try {
        return { rows: await listCredentialBindings(), available: true }
      } catch (error) {
        return { rows: [] as CredentialBinding[], available: !isDisabled(error) }
      }
    },
  )

  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal<string | null>(null)
  const [failed, setFailed] = createSignal(false)
  const notify = (text: string, isFailure: boolean) => {
    setFailed(isFailure)
    setMessage(text)
  }

  const [restName, setRestName] = createSignal('')
  const [restHost, setRestHost] = createSignal('')
  const [restHeader, setRestHeader] = createSignal<(typeof REST_HEADERS)[number]>('Authorization')
  const [restMethods, setRestMethods] = createSignal<string[]>(['GET'])
  const [restPrefixes, setRestPrefixes] = createSignal('')
  const [restSecret, setRestSecret] = createSignal('')

  const [mcpName, setMcpName] = createSignal('')
  const [mcpUrl, setMcpUrl] = createSignal(takePendingStoredTokenUrl())
  const [mcpTools, setMcpTools] = createSignal('')
  const [mcpSecret, setMcpSecret] = createSignal('')

  function toggleMethod(method: string, on: boolean) {
    setRestMethods((current) => {
      const rest = current.filter((item) => item !== method)
      return on ? [...rest, method] : rest
    })
  }

  function kindLabel(kind: string): string {
    if (kind === 'mcp_bearer') return i18n.tr('MCP-server, bærer', 'MCP server, bearer')
    if (kind === 'rest_bearer') return i18n.tr('REST, bærer', 'REST, bearer')
    if (kind === 'rest_api_key') return i18n.tr('REST, API-nøkkel', 'REST, API key')
    return kind
  }

  async function revoke(binding: CredentialBinding) {
    setBusy(true)
    setMessage(null)
    try {
      await revokeCredentialBinding(binding.id)
      notify(i18n.tr('Nøkkelen er tilbakekalt.', 'The key was revoked.'), false)
      setVersion((current) => current + 1)
    } catch {
      notify(i18n.tr('Nøkkelen kunne ikke tilbakekalles.', 'The key could not be revoked.'), true)
    } finally {
      setBusy(false)
    }
  }

  async function submitRest(event: Event) {
    event.preventDefault()
    // The key leaves its field at once, whatever happens next: it is sent once and never kept here.
    const secret = restSecret()
    setRestSecret('')
    const name = restName().trim()
    const host = restHost().trim().toLowerCase()
    const header = restHeader()
    const methods = restMethods()
    const prefixes = parseLines(restPrefixes())
    const prefixesValid =
      prefixes.length > 0 && prefixes.length <= MAX_PREFIXES && prefixes.every((prefix) => prefix.startsWith('/'))
    if (!name || !HOST_SHAPE.test(host) || methods.length === 0 || !prefixesValid || !secret) {
      notify(
        i18n.tr(
          'Fyll ut navn, vert, minst én metode, minst én sti som begynner med / og nøkkelen. Verten er et domenenavn uten https:// eller sti.',
          'Fill in a name, a host, at least one method, at least one path that starts with / and the key. The host is a domain name without https:// or a path.',
        ),
        true,
      )
      return
    }
    setBusy(true)
    setMessage(null)
    let binding: CredentialBinding | undefined
    try {
      binding = await createCredentialBinding({
        kind: header === 'Authorization' ? 'rest_bearer' : 'rest_api_key',
        host,
        headerName: header,
        allowedMethods: methods,
        allowedPathPrefixes: prefixes,
        secret,
      })
    } catch {
      binding = undefined
    }
    if (!binding) {
      setBusy(false)
      notify(
        i18n.tr(
          'Nøkkelen kunne ikke lagres. Kontroller feltene og prøv igjen.',
          'The key could not be saved. Check the fields and try again.',
        ),
        true,
      )
      return
    }
    const bindingId = binding.id
    try {
      await createCustomRestConnector({ name, credentialBindingId: bindingId })
      setRestName('')
      setRestHost('')
      setRestPrefixes('')
      setRestMethods(['GET'])
      notify(i18n.tr('Koblingen er opprettet.', 'The connector was created.'), false)
    } catch {
      const removed = await removeBinding(bindingId)
      notify(
        removed
          ? i18n.tr(
              'Koblingen kunne ikke opprettes. Nøkkelen er fjernet igjen.',
              'The connector could not be created. The key was removed again.',
            )
          : i18n.tr(
              'Koblingen kunne ikke opprettes, og nøkkelen kunne ikke fjernes. Fjern den i listen.',
              'The connector could not be created, and the key could not be removed. Remove it from the list.',
            ),
        true,
      )
    } finally {
      setBusy(false)
      setVersion((current) => current + 1)
    }
  }

  async function submitMcp(event: Event) {
    event.preventDefault()
    // The token leaves its field at once, whatever happens next: it is sent once and never kept here.
    const secret = mcpSecret()
    setMcpSecret('')
    const name = mcpName().trim()
    const address = mcpUrl().trim()
    const target = mcpTarget(address)
    const tools = parseList(mcpTools())
    if (
      !name ||
      !target ||
      !HOST_SHAPE.test(target.host) ||
      !secret ||
      tools.length === 0 ||
      tools.length > MAX_TOOLS ||
      !tools.every((tool) => TOOL_NAME.test(tool))
    ) {
      notify(
        i18n.tr(
          'Fyll ut navn, en https-adresse uten portnummer, søk eller lenke, nøkkelen og 1 til 64 verktøynavn.',
          'Fill in a name, an https address with no port number, query or fragment, the key and 1 to 64 tool names.',
        ),
        true,
      )
      return
    }
    setBusy(true)
    setMessage(null)
    let binding: CredentialBinding | undefined
    try {
      binding = await createCredentialBinding({
        kind: 'mcp_bearer',
        host: target.host,
        headerName: 'Authorization',
        allowedMethods: MCP_METHODS,
        allowedPathPrefixes: [target.path],
        secret,
      })
    } catch {
      binding = undefined
    }
    if (!binding) {
      setBusy(false)
      notify(
        i18n.tr(
          'Nøkkelen kunne ikke lagres. Kontroller feltene og prøv igjen.',
          'The key could not be saved. Check the fields and try again.',
        ),
        true,
      )
      return
    }
    const bindingId = binding.id
    try {
      await registerStaticBearerMcpServer({
        name,
        url: address,
        toolAllowlist: tools,
        credentialBindingId: bindingId,
      })
      setMcpName('')
      setMcpUrl('')
      setMcpTools('')
      notify(
        i18n.tr('Serveren er registrert med den lagrede nøkkelen.', 'The server was registered with the stored key.'),
        false,
      )
    } catch {
      const removed = await removeBinding(bindingId)
      notify(
        removed
          ? i18n.tr(
              'Serveren kunne ikke registreres. Nøkkelen er fjernet igjen.',
              'The server could not be registered. The key was removed again.',
            )
          : i18n.tr(
              'Serveren kunne ikke registreres, og nøkkelen kunne ikke fjernes. Fjern den i listen.',
              'The server could not be registered, and the key could not be removed. Remove it from the list.',
            ),
        true,
      )
    } finally {
      setBusy(false)
      setVersion((current) => current + 1)
    }
  }

  return (
    <Show when={enabled && admin && settings()?.zdr === false}>
      <section class="verevon-settings-custom-connectors" aria-labelledby="verevon-custom-connectors-title">
        <h3 id="verevon-custom-connectors-title">{i18n.tr('Egne koblinger', 'Custom connectors')}</h3>
        <p role="note">
          {i18n.tr(
            'Nøkkelen lagres kryptert i integrasjonstjenesten, vises aldri igjen og sendes ikke til modellen. Bare eiere og administratorer kan endre dette.',
            'A key is sealed in the integration service, is never shown again and is never sent to the model. Only owners and admins can change this.',
          )}
        </p>
        <Show
          when={stored()?.available !== false}
          fallback={
            <p role="note">
              {i18n.tr(
                'Egne koblinger er ikke slått på i dette miljøet.',
                'Custom connectors are not turned on in this environment.',
              )}
            </p>
          }
        >
          <h4>{i18n.tr('Lagrede nøkler', 'Stored keys')}</h4>
          <Show
            when={(stored()?.rows ?? []).length > 0}
            fallback={<p>{i18n.tr('Ingen lagrede nøkler ennå.', 'No stored keys yet.')}</p>}
          >
            <ul>
              <For each={stored()?.rows ?? []}>
                {(binding) => (
                  <li>
                    <span>{binding.host}</span>
                    <span>{kindLabel(binding.kind)}</span>
                    <span>{binding.headerName}</span>
                    <span>{binding.allowedMethods.join(', ')}</span>
                    <span>{binding.allowedPathPrefixes.join(', ')}</span>
                    <span>
                      {binding.status === 'active' ? i18n.tr('Aktiv', 'Active') : i18n.tr('Tilbakekalt', 'Revoked')}
                    </span>
                    <Show when={binding.status === 'active'}>
                      <button type="button" disabled={busy()} onClick={() => void revoke(binding)}>
                        {i18n.tr(`Tilbakekall ${binding.host}`, `Revoke ${binding.host}`)}
                      </button>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <form class="verevon-settings-custom-connectors__rest" onSubmit={(event) => void submitRest(event)}>
            <h4>{i18n.tr('Ny REST-kobling', 'New REST connector')}</h4>
            <p>
              {i18n.tr(
                'Kallene går bare til verten, metodene og stiene du angir.',
                'Calls go only to the host, methods and paths you name.',
              )}
            </p>
            <label>
              {i18n.tr('Navn', 'Name')}
              <input
                type="text"
                maxlength={120}
                value={restName()}
                onInput={(event) => setRestName(event.currentTarget.value)}
              />
            </label>
            <label>
              {i18n.tr('Vert (uten https:// og sti)', 'Host (without https:// and a path)')}
              <input
                type="text"
                autocomplete="off"
                value={restHost()}
                onInput={(event) => setRestHost(event.currentTarget.value)}
              />
            </label>
            <label>
              {i18n.tr('Header for nøkkelen', 'Header for the key')}
              <select
                value={restHeader()}
                onChange={(event) => setRestHeader(event.currentTarget.value as (typeof REST_HEADERS)[number])}
              >
                <For each={REST_HEADERS}>{(header) => <option value={header}>{header}</option>}</For>
              </select>
            </label>
            <fieldset>
              <legend>{i18n.tr('Tillatte metoder', 'Allowed methods')}</legend>
              <For each={REST_METHODS}>
                {(method) => (
                  <label>
                    <input
                      type="checkbox"
                      checked={restMethods().includes(method)}
                      onChange={(event) => toggleMethod(method, event.currentTarget.checked)}
                    />
                    {method}
                  </label>
                )}
              </For>
            </fieldset>
            <label>
              {i18n.tr('Tillatte stier (én per linje, for eksempel /v1/)', 'Allowed paths (one per line, for example /v1/)')}
              <textarea value={restPrefixes()} onInput={(event) => setRestPrefixes(event.currentTarget.value)} />
            </label>
            <label>
              {i18n.tr('Nøkkel', 'Key')}
              <input
                type="password"
                autocomplete="off"
                value={restSecret()}
                onInput={(event) => setRestSecret(event.currentTarget.value)}
              />
            </label>
            <button type="submit" disabled={busy()}>
              {i18n.tr('Lagre nøkkel og opprett kobling', 'Save key and create connector')}
            </button>
          </form>

          <form class="verevon-settings-custom-connectors__mcp" onSubmit={(event) => void submitMcp(event)}>
            <h4>{i18n.tr('Bruk en lagret nøkkel', 'Use a stored token')}</h4>
            <p>
              {i18n.tr(
                'Verktøyene må navnes her. En nøkkel kan ikke brukes til å hente verktøylisten før serveren er lagret.',
                'Name the tools here. A key cannot be used to list the tools before the server is saved.',
              )}
            </p>
            <label>
              {i18n.tr('Navn på serveren', 'Server name')}
              <input
                type="text"
                maxlength={120}
                value={mcpName()}
                onInput={(event) => setMcpName(event.currentTarget.value)}
              />
            </label>
            <label>
              {i18n.tr('Adresse (https, standardporten)', 'Address (https, default port)')}
              <input
                type="url"
                value={mcpUrl()}
                onInput={(event) => setMcpUrl(event.currentTarget.value)}
              />
            </label>
            <label>
              {i18n.tr('Verktøy (ett per linje, høyst 64)', 'Tools (one per line, at most 64)')}
              <textarea value={mcpTools()} onInput={(event) => setMcpTools(event.currentTarget.value)} />
            </label>
            <label>
              {i18n.tr('Bærer-nøkkel', 'Bearer token')}
              <input
                type="password"
                autocomplete="off"
                value={mcpSecret()}
                onInput={(event) => setMcpSecret(event.currentTarget.value)}
              />
            </label>
            <button type="submit" disabled={busy()}>
              {i18n.tr('Lagre nøkkel og registrer server', 'Save key and register server')}
            </button>
          </form>
        </Show>
        <Show when={message()}>
          {(text) => <p role={failed() ? 'alert' : 'status'}>{text()}</p>}
        </Show>
      </section>
    </Show>
  )
}
