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
    headers: { 'x-velion-org-id': orgId },
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
    headers: { 'x-velion-org-id': orgId },
  })
}

/**
 * Begin connecting an MCP server that requires real OAuth 2.1 login (e.g.
 * Visma Net) rather than a static token. Response carries the
 * `authorization_url` to navigate the browser to next — the actual
 * connection completes on the server's own OAuth consent screen and lands
 * back on `/settings?mcp_oauth=connected|error`, not on any promise this
 * call resolves.
 */
export function startMcpOAuth(
  orgId: string,
  body: {
    name: string
    url: string
    tool_allowlist?: string[]
    scope?: 'user' | 'org'
  },
): Promise<{ authorization_url: string }> {
  return requestJson<{ authorization_url: string }>('/api/v1/mcp/servers/oauth/start', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function deleteMcpServer(orgId: string, serverId: string): Promise<void> {
  return requestJson<void>(`/api/v1/mcp/servers/${encodeURIComponent(serverId)}`, {
    method: 'DELETE',
    headers: { 'x-velion-org-id': orgId },
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
      headers: { 'x-velion-org-id': orgId },
    },
  )
}
