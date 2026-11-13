import { requestJson } from './http'

/**
 * A registered MCP server, mirroring model-gateway's `mcp_server_json` response
 * shape. The secret `token` is intentionally absent — the backend omits it from
 * every response, so it is never available to the UI.
 */
export interface McpServer {
  server_id: string
  name: string
  url: string
  transport: string
  tool_allowlist: string[]
  enabled: boolean
  /** `'user'` (private, default) or `'org'` (org-wide, admin-created). */
  scope: string
  /** Id of the user that owns the server (empty for org-wide servers). */
  owner_user_id: string
  /** User ids the owner has explicitly shared a private server with. */
  shared_with: string[]
}

export function listMcpServers(orgId: string): Promise<{ servers: McpServer[] }> {
  return requestJson<{ servers: McpServer[] }>('/api/v1/mcp/servers', {
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function registerMcpServer(
  orgId: string,
  body: {
    name: string
    url: string
    transport?: string
    token?: string
    tool_allowlist?: string[]
    enabled?: boolean
    server_id?: string
    scope?: 'user' | 'org'
  },
): Promise<McpServer> {
  return requestJson<McpServer>('/api/v1/mcp/servers', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

/** What `connectMcpServer` resolves to — a discriminated union on `needs_oauth`. */
export type McpConnectResult =
  | { needs_oauth: true; authorization_url: string }
  | (McpServer & { needs_oauth?: false })

/**
 * The one entry point for adding an MCP server. Verevon figures out the rest:
 * discovers whether the server needs real OAuth 2.1 login (e.g. Visma Net)
 * or works directly, so the caller never picks a transport or auth mode.
 *
 * If the result needs OAuth, navigate the browser to `authorization_url` —
 * the connection completes on the server's own consent screen and lands back
 * on `/settings/mcp?mcp_oauth=connected|error`, not on any promise this call
 * resolves. Otherwise the server is already registered and returned directly.
 */
export function connectMcpServer(
  orgId: string,
  body: {
    name: string
    url: string
    token?: string
    tool_allowlist?: string[]
    scope?: 'user' | 'org'
  },
): Promise<McpConnectResult> {
  return requestJson<McpConnectResult>('/api/v1/mcp/servers/connect', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

/** What a server needs before it answers a tool listing: none, an OAuth login, or a static bearer token. */
export type McpProbeAuth = 'none' | 'oauth' | 'bearer_required'

/**
 * One tool a probe reports. The hints are the server's own claims, shown for a person to judge; a hint is
 * null when the server gave none.
 */
export interface McpProbedTool {
  name: string
  description: string
  read_only_hint: boolean | null
  destructive_hint: boolean | null
}

/**
 * What `probeMcpServer` resolves to. `tools` is empty unless `auth` is `none`: a server's tools are listed
 * only when it needs no credential, and the probe never sends one.
 */
export interface McpProbeResult {
  auth: McpProbeAuth
  tools: McpProbedTool[]
}

/**
 * Ask what an MCP server needs before anyone saves it: its auth requirement and, when it needs no
 * credential, up to 64 of its tools. Nothing is registered, and the body is the address alone, so a
 * credential has no field to travel in.
 */
export function probeMcpServer(url: string): Promise<McpProbeResult> {
  return requestJson<McpProbeResult>('/api/v1/mcp/servers/probe', {
    method: 'POST',
    body: JSON.stringify({ url }),
  })
}

export function deleteMcpServer(orgId: string, serverId: string): Promise<void> {
  return requestJson<void>(`/api/v1/mcp/servers/${encodeURIComponent(serverId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * Replace a user-owned server's share set. The backend enforces OWNER-ONLY
 * authorization (403 otherwise) and returns the updated ownership fields.
 */
export function shareMcpServer(
  orgId: string,
  serverId: string,
  userIds: string[],
): Promise<{ server_id: string; scope: string; owner_user_id: string; shared_with: string[] }> {
  return requestJson<{ server_id: string; scope: string; owner_user_id: string; shared_with: string[] }>(
    `/api/v1/mcp/servers/${encodeURIComponent(serverId)}/share`,
    {
      method: 'POST',
      body: JSON.stringify({ user_ids: userIds }),
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}
