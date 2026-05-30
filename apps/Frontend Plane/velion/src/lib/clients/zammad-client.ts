/**
 * Zammad API Client - Type-safe client for customer support ticketing
 */

export interface ZammadTicket {
  id: number
  number: number
  title: string
  state: string
  priority: string
  customer_id: number
  group_id: number
  created_at: string
  updated_at: string
}

export interface CreateZammadTicketRequest {
  title: string
  group: string
  customer_email: string
  article?: { body: string; content_type?: string }
  priority?: string
}

export class ZammadClient {
  constructor(private baseUrl: string, private token: string) {}

  private async request<T>(method: string, endpoint: string, body?: unknown): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`
    const response = await fetch(url, {
      method,
      headers: {
        'Authorization': `Bearer ${this.token}`,
        'Content-Type': 'application/json',
      },
      body: body ? JSON.stringify(body) : undefined,
    })

    if (!response.ok) {
      throw new Error(`Zammad error: ${response.status}`)
    }
    return response.json()
  }

  async getTickets(limit = 50, page = 1): Promise<ZammadTicket[]> {
    return this.request<ZammadTicket[]>('GET', '/api/v1/tickets?limit=' + limit + '&page=' + page)
  }

  async getTicket(id: number): Promise<ZammadTicket> {
    return this.request<ZammadTicket>('GET', '/api/v1/tickets/' + id)
  }

  async createTicket(data: CreateZammadTicketRequest) {
    return this.request('POST', '/api/v1/tickets', {
      title: data.title,
      group: data.group,
      customer_email: data.customer_email,
      article: data.article || { body: '' },
    })
  }

  async updateTicket(id: number, updates: any) {
    return this.request('PATCH', '/api/v1/tickets/' + id, updates)
  }

  async addComment(ticketId: number, body: string) {
    return this.request('POST', '/api/v1/tickets/' + ticketId + '/articles', {
      body,
      type: 'comment',
      content_type: 'text/plain',
    })
  }

  async health() {
    try {
      await this.request('GET', '/api/v1/version')
      return { ok: true }
    } catch {
      return { ok: false }
    }
  }
}

export function createZammadClientFromEnv() {
  let baseUrl = ''
  let token = ''

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_ZAMMAD_API_URL) {
    baseUrl = process.env.EXTERNAL_ZAMMAD_API_URL
  }

  if (!baseUrl && typeof window !== 'undefined') {
    baseUrl = localStorage.getItem('zammad_api_url') || 'http://localhost:3012'
  }

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_ZAMMAD_TOKEN) {
    token = process.env.EXTERNAL_ZAMMAD_TOKEN
  }

  if (!token && typeof window !== 'undefined') {
    token = localStorage.getItem('zammad_token') || ''
  }

  return new ZammadClient(baseUrl, token)
}
