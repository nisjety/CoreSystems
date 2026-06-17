/**
 * Nango API Client - Universal API connector platform (500+ integrations)
 */

export interface NangoIntegration {
  id: string
  provider: string
  auth_type: string
  created_at: string
}

export interface NangoConnection {
  id: string
  provider: string
  status: 'active' | 'inactive' | 'error'
  created_at: string
}

export class NangoClient {
  constructor(private baseUrl: string, private apiKey = '') {}

  private async request<T>(method: string, endpoint: string, body?: unknown): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }

    if (this.apiKey) {
      headers['X-API-Key'] = this.apiKey
    }

    const response = await fetch(url, {
      method,
      headers,
      credentials: 'include',
      body: body ? JSON.stringify(body) : undefined,
    })

    if (!response.ok) {
      throw new Error(`Nango error: ${response.status}`)
    }
    return response.json()
  }

  async listIntegrations(): Promise<NangoIntegration[]> {
    const payload = await this.request<{
      data?: { integrations?: NangoIntegration[] }
      integrations?: NangoIntegration[]
    } | NangoIntegration[]>('GET', '/api/v1/integrations')

    if (Array.isArray(payload)) {
      return payload
    }

    return payload.data?.integrations ?? payload.integrations ?? []
  }

  async getIntegration(id: string) {
    return this.request('GET', '/api/v1/integrations/' + id)
  }

  async listConnections() {
    return this.request('GET', '/api/v1/connections')
  }

  async getConnection(connectionId: string) {
    return this.request('GET', '/api/v1/connections/' + connectionId)
  }

  async initiateOAuth(provider: string, redirectTo: string, scopes?: string[]): Promise<string> {
    const response = await this.request<{ oauth_url?: string }>('POST', '/api/v1/oauth/init', {
      provider,
      redirect_to: redirectTo,
      scopes: scopes || [],
    })
    return response.oauth_url ?? ''
  }

  async exchangeCode(code: string, connectionId: string) {
    return this.request('POST', '/api/v1/oauth/callback', {
      code,
      connection_id: connectionId,
    })
  }

  async sync(connectionId: string, syncName: string, model?: string) {
    return this.request('POST', '/api/v1/sync', {
      connection_id: connectionId,
      sync_name: syncName,
      model,
    })
  }

  async proxyRequest(connectionId: string, method: string, endpoint: string, body?: any) {
    return this.request(method, '/api/v1/proxy', {
      connection_id: connectionId,
      method,
      endpoint,
      body,
    })
  }

  async health() {
    try {
      await this.request('GET', '/api/v1/health')
      return { ok: true }
    } catch {
      return { ok: false }
    }
  }
}

export function createNangoClientFromEnv() {
  let baseUrl = ''
  let apiKey = ''

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_NANGO_API_URL) {
    baseUrl = process.env.EXTERNAL_NANGO_API_URL
  }

  if (!baseUrl && typeof window !== 'undefined') {
    baseUrl = localStorage.getItem('nango_api_url') || '/api/external/nango'
  }

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_NANGO_API_KEY) {
    apiKey = process.env.EXTERNAL_NANGO_API_KEY
  }

  if (!apiKey && typeof window !== 'undefined') {
    apiKey = localStorage.getItem('nango_api_key') || ''
  }

  return new NangoClient(baseUrl, apiKey)
}
