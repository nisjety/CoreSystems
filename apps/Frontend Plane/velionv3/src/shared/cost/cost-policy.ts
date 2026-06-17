type ModelTier = 'fast' | 'balanced' | 'deep'

export type LlmTaskEstimate = {
  textLength: number
  itemCount: number
  risk: 'low' | 'medium' | 'high'
}

export type CostPolicyDecision = {
  tier: ModelTier
  reason: string
  maxPromptTokens: number
}

export function selectModelTier(estimate: LlmTaskEstimate): CostPolicyDecision {
  if (estimate.risk === 'high' || estimate.textLength > 12_000 || estimate.itemCount > 40) {
    return {
      tier: 'deep',
      reason: 'Complex or risky work needs stronger reasoning and approval context.',
      maxPromptTokens: 48_000,
    }
  }

  if (estimate.risk === 'medium' || estimate.textLength > 4_000 || estimate.itemCount > 12) {
    return {
      tier: 'balanced',
      reason: 'Moderate context needs a balanced model before escalation.',
      maxPromptTokens: 20_000,
    }
  }

  return {
    tier: 'fast',
    reason: 'Small low-risk task should use the cheapest competent route.',
    maxPromptTokens: 8_000,
  }
}
