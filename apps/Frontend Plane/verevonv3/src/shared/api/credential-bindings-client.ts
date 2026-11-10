import { requestJson } from './http'

/**
 * An organisation's credential bindings and its custom REST connector (W4-CC-8). A binding holds one bearer token or
 * API key for one host, sealed by Integration Core. The secret is posted once and never returned, so no shape here
 * carries one. The action contracts are `connectors.create_credential_binding`, `connectors.revoke_credential_binding`
 * and `connectors.create_custom_rest`; they are human only (model access none) and admin only.
 */

/** What a binding holds: an MCP server's bearer token, or a REST API's bearer token or API key. */
export type CredentialBindingKind = 'mcp_bearer' | 'rest_api_key' | 'rest_bearer'

/** A binding as Integration Core reports it: metadata only, never the secret. */
export type CredentialBinding = Readonly<{
  id: string
  kind: string
  host: string
  headerName: string
  allowedMethods: readonly string[]
  allowedPathPrefixes: readonly string[]
  status: string
  createdAt: string
  revokedAt: string | null
}>

export type CredentialBindingInput = Readonly<{
  kind: CredentialBindingKind
  host: string
  headerName: string
  allowedMethods: readonly string[]
  allowedPathPrefixes: readonly string[]
  /** Sent once and never kept: the caller clears its field before the request is made. */
  secret: string
}>

export type CustomRestConnectorInput = Readonly<{
  name: string
  credentialBindingId: string
}>

export type StaticBearerMcpServerInput = Readonly<{
  name: string
  url: string
  /** The exact tool names the server may expose, 1 to 64. A static token cannot list them before the server is saved. */
  toolAllowlist: readonly string[]
  credentialBindingId: string
}>

const CREDENTIAL_BINDINGS_PATH = '/api/v1/credential-bindings'
const CUSTOM_REST_CONNECTOR_PATH = '/api/v1/connectors/custom-rest'
const MCP_SERVERS_PATH = '/api/v1/mcp/servers'

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function textsOf(value: unknown): string[] {
  const parts: unknown[] = Array.isArray(value) ? value : []
  return parts.map(textOf).filter((part) => part !== '')
}

/** One binding as Integration Core reports it. A row without an id is dropped, not guessed at. */
function bindingOf(raw: unknown): CredentialBinding | undefined {
  const row = asRecord(raw)
  if (!row) return undefined
  const id = textOf(row.id)
  if (!id) return undefined
  return {
    id,
    kind: textOf(row.kind),
    host: textOf(row.host),
    headerName: textOf(row.headerName) || textOf(row.header_name),
    allowedMethods: textsOf(row.allowedMethods ?? row.allowed_methods),
    allowedPathPrefixes: textsOf(row.allowedPathPrefixes ?? row.allowed_path_prefixes),
    status: textOf(row.status),
    createdAt: textOf(row.createdAt ?? row.created_at),
    revokedAt: textOf(row.revokedAt ?? row.revoked_at) || null,
  }
}

/** The organisation's credential bindings (`GET`), newest first, metadata only. Owners and admins only. */
export async function listCredentialBindings(): Promise<CredentialBinding[]> {
  const raw = await requestJson<unknown>(CREDENTIAL_BINDINGS_PATH)
  const rows: unknown[] = Array.isArray(asRecord(raw)?.bindings) ? (asRecord(raw)?.bindings as unknown[]) : []
  return rows
    .map(bindingOf)
    .filter((binding): binding is CredentialBinding => binding !== undefined)
}

/**
 * Stores a credential binding (`POST`, owners and admins only). The key is sealed upstream and the reply carries the
 * binding without it. The body names the fields the binding is created with; the organisation is never sent.
 */
export async function createCredentialBinding(input: CredentialBindingInput): Promise<CredentialBinding | undefined> {
  const raw = await requestJson<unknown>(CREDENTIAL_BINDINGS_PATH, {
    method: 'POST',
    body: JSON.stringify({
      kind: input.kind,
      host: input.host.trim(),
      header_name: input.headerName,
      allowed_methods: [...input.allowedMethods],
      allowed_path_prefixes: [...input.allowedPathPrefixes],
      secret: input.secret,
    }),
  })
  return bindingOf(asRecord(raw)?.binding)
}

/** Revokes one binding (`DELETE`, owners and admins only). Its stored secret is emptied upstream. */
export async function revokeCredentialBinding(bindingId: string): Promise<void> {
  await requestJson<unknown>(`${CREDENTIAL_BINDINGS_PATH}/${encodeURIComponent(bindingId.trim())}`, {
    method: 'DELETE',
  })
}

/**
 * Creates a custom REST connector that calls through one binding (`POST`, owners and admins only). The host, the
 * methods and the path prefixes are the binding's; the request names only the connector and the binding.
 */
export async function createCustomRestConnector(input: CustomRestConnectorInput): Promise<void> {
  await requestJson<unknown>(CUSTOM_REST_CONNECTOR_PATH, {
    method: 'POST',
    body: JSON.stringify({
      name: input.name.trim(),
      credential_binding_id: input.credentialBindingId.trim(),
    }),
  })
}

/**
 * Registers an MCP server whose bearer token is a stored binding (`auth_kind` `static_bearer`, W4-CC-5). The body names
 * the binding, never a token. The transport is HTTP, and the server keeps the default private scope.
 */
export function registerStaticBearerMcpServer(input: StaticBearerMcpServerInput): Promise<unknown> {
  return requestJson<unknown>(MCP_SERVERS_PATH, {
    method: 'POST',
    body: JSON.stringify({
      name: input.name.trim(),
      url: input.url.trim(),
      transport: 'http',
      auth_kind: 'static_bearer',
      credential_binding_id: input.credentialBindingId.trim(),
      tool_allowlist: [...input.toolAllowlist],
    }),
  })
}
