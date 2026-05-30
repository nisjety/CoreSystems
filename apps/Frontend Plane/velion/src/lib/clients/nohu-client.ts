/**
 * Nohu API Client - Workflow orchestration engine
 */

export interface NohuWorkflow {
  id: string
  name: string
  status: 'draft' | 'active' | 'paused' | 'archived'
  steps: NohuWorkflowStep[]
  created_at: string
}

export interface NohuWorkflowStep {
  id: string
  name: string
  action: string
  config: Record<string, any>
}

export interface NohuExecution {
  id: string
  workflow_id: string
  status: 'pending' | 'running' | 'completed' | 'failed'
  input: Record<string, any>
  output?: Record<string, any>
  error?: string
  created_at: string
}

export class NohuClient {
  constructor(private baseUrl: string, private apiKey: string) {}

  private async request<T>(method: string, endpoint: string, body?: unknown): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`
    const response = await fetch(url, {
      method,
      headers: {
        'X-API-Key': this.apiKey,
        'Content-Type': 'application/json',
      },
      body: body ? JSON.stringify(body) : undefined,
    })

    if (!response.ok) {
      throw new Error(`Nohu error: ${response.status}`)
    }
    return response.json()
  }

  async listWorkflows() {
    return this.request('GET', '/api/v1/workflows')
  }

  async getWorkflow(id: string) {
    return this.request('GET', '/api/v1/workflows/' + id)
  }

  async createWorkflow(data: any) {
    return this.request('POST', '/api/v1/workflows', data)
  }

  async updateWorkflow(id: string, updates: any) {
    return this.request('PATCH', '/api/v1/workflows/' + id, updates)
  }

  async publishWorkflow(id: string) {
    return this.request('POST', '/api/v1/workflows/' + id + '/publish', {})
  }

  async executeWorkflow(workflowId: string, input: Record<string, any>) {
    return this.request('POST', '/api/v1/workflows/' + workflowId + '/execute', { input })
  }

  async getExecution(executionId: string): Promise<NohuExecution> {
    return this.request('GET', '/api/v1/executions/' + executionId)
  }

  async listExecutions(workflowId: string) {
    return this.request('GET', '/api/v1/workflows/' + workflowId + '/executions')
  }

  async cancelExecution(executionId: string) {
    return this.request('POST', '/api/v1/executions/' + executionId + '/cancel', {})
  }

  async getExecutionLogs(executionId: string) {
    return this.request('GET', '/api/v1/executions/' + executionId + '/logs')
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

export function createNohuClientFromEnv() {
  let baseUrl = ''
  let apiKey = ''

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_NOHU_API_URL) {
    baseUrl = process.env.EXTERNAL_NOHU_API_URL
  }

  if (!baseUrl && typeof window !== 'undefined') {
    baseUrl = localStorage.getItem('nohu_api_url') || 'http://localhost:3014'
  }

  if (typeof process !== 'undefined' && process.env?.EXTERNAL_NOHU_API_KEY) {
    apiKey = process.env.EXTERNAL_NOHU_API_KEY
  }

  if (!apiKey && typeof window !== 'undefined') {
    apiKey = localStorage.getItem('nohu_api_key') || ''
  }

  return new NohuClient(baseUrl, apiKey)
}
