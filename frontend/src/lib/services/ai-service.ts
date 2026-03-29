import { apiClient } from '@/lib/api-client'

export interface ReasoningRequest {
  query: string
  strategy?: 'auto' | 'chain_of_thought' | 'tree_of_thought' | 'graph_reasoning' | 'symbolic_reasoning' | 'causal_reasoning'
  depth?: 'fast' | 'standard' | 'deep' | 'expert'
  context?: Record<string, unknown>
  max_steps?: number
  max_branches?: number
  max_hops?: number
  require_citations?: boolean
  enable_verification?: boolean
}

export interface ReasoningStep {
  step: number
  action: string
  thought: string
  result?: unknown
  confidence: number
  sources: string[]
  method?: string
}

export interface ReasoningResponse {
  query: string
  answer: string
  reasoning_trace: ReasoningStep[]
  strategy_used: string
  confidence: number
  reasoning_time_ms: number
  alternative_explanations: string[]
  knowledge_graph_path?: string | null
  verification_result?: Record<string, unknown> | null
  metadata: Record<string, unknown>
}

class AIServiceAPI {
  async reason(data: ReasoningRequest): Promise<ReasoningResponse> {
    return apiClient.post<ReasoningResponse>('/api/reasoning/reason', data)
  }
}

export const aiService = new AIServiceAPI()
