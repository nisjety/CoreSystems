// U2-1 follow-up (ui-ux-velion-gap.md §10): model registry aligned with
// the actual Azure OpenAI deployments on `core-ai-rg.cognitiveservices.
// azure.com`. Previous defaults referenced fictional names like
// `gpt-5.4-mini` which Azure returned 404 for — chat broke with
// "all providers exhausted after 0 total attempts" → "Kunne ikke hente
// svar fra Model Plane".
//
// Probed deployments (POST .../chat/completions?api-version=2025-01-01-preview):
//   gpt-4o-mini  → 200  (standard chat, max_tokens)
//   gpt-5-mini   → 200  (reasoning model, max_completion_tokens)
//   text-embedding-3-large → 400  (embedding deployment, different API)
//   gpt-4o, gpt-4, gpt-35-turbo, gpt-4-turbo → 404 (NOT provisioned)
//
// Anthropic + Google keys are also present in `apps/Model Plane v2/.env`
// — when the gateway routes a `claude-*` model the request goes via the
// Anthropic provider in inference-core, not Azure.

export type SupportedModel =
  | 'claude-sonnet-4-5'
  | 'claude-opus-4-1'
  | 'gpt-4o-mini'
  | 'gpt-5-mini'

export type ModelTier = 'low' | 'high'

interface ModelMeta {
  provider: 'anthropic' | 'openai' | 'azure'
  tier: ModelTier
  maxContextTokens: number
  label: string
}

export const SUPPORTED_MODELS: Record<SupportedModel, ModelMeta> = {
  // Standard chat — fastest, cheapest, exposed as Azure deployment of
  // the same name on core-ai-rg. Most user turns route here.
  'gpt-4o-mini': {
    provider: 'azure',
    tier: 'low',
    maxContextTokens: 128_000,
    label: 'GPT-4o Mini',
  },
  // Reasoning model — newer architecture, uses `max_completion_tokens`
  // (the inference-core AzureOpenAiProvider handles the rename when the
  // deployment name starts with `gpt-5` or `o1`/`o3`).
  'gpt-5-mini': {
    provider: 'azure',
    tier: 'high',
    maxContextTokens: 128_000,
    label: 'GPT-5 Mini (reasoning)',
  },
  // Anthropic models. Provider is registered when ANTHROPIC_API_KEY is set
  // on inference-core (see provider/fallback.rs). The Anthropic provider
  // accepts these literal model ids.
  'claude-sonnet-4-5': {
    provider: 'anthropic',
    tier: 'high',
    maxContextTokens: 200_000,
    label: 'Claude Sonnet 4.5',
  },
  'claude-opus-4-1': {
    provider: 'anthropic',
    tier: 'high',
    maxContextTokens: 200_000,
    label: 'Claude Opus 4.1',
  },
}

export const DEFAULT_MODEL: SupportedModel = 'gpt-4o-mini'
export const UPGRADE_MODEL: SupportedModel = 'gpt-5-mini'

export function isSupportedModel(value: string): value is SupportedModel {
  return value in SUPPORTED_MODELS
}

export type ComplexityHint = 'simple' | 'complex'

/**
 * Detect query complexity by cheap heuristics.
 * Used to decide whether to upgrade to a higher-tier model.
 */
export function detectComplexity(
  content: string,
  historyLength: number,
): ComplexityHint {
  if (historyLength >= 6 || content.length >= 500) return 'complex'

  const complexKeywords = [
    'compare', 'analyse', 'analyze', 'explain', 'difference', 'summarize',
    'summarise', 'step by step', 'why does', 'how does', 'root cause',
    'debug', 'investigate',
  ]
  const lower = content.toLowerCase()
  if (complexKeywords.some((kw) => lower.includes(kw))) return 'complex'

  return 'simple'
}

/**
 * Resolve which model to use for a request.
 *
 * Priority:
 * 1. Explicit request model override (client-specified)
 * 2. Complexity-based upgrade (when agent allows upgrades)
 * 3. Agent's configured model
 * 4. Default model
 *
 * The upgrade only applies when:
 * - `allowUpgrade` is true (agent has not locked the model)
 * - Query is classified as complex
 * - Agent's configured model is low-tier
 */
export function resolveModel(options: {
  requestModel?: string
  agentModel?: string
  allowedModels?: string[]
  complexity?: ComplexityHint
  allowUpgrade?: boolean
}): string {
  const { requestModel, agentModel, allowedModels, complexity, allowUpgrade } = options

  // Client override takes precedence — validate it's allowed.
  // Legacy alias remap: clients still sending `gpt-5.4-mini` (fictional)
  // get silently corrected to the real deployment.
  const normalizedRequest = normalizeLegacyAlias(requestModel)

  if (normalizedRequest) {
    if (allowedModels?.length && !allowedModels.includes(normalizedRequest)) {
      // Fall through to agent/default
    } else {
      return normalizedRequest
    }
  }

  const normalizedAgent = normalizeLegacyAlias(agentModel)

  // Complexity upgrade: only when agent model is low-tier
  if (
    allowUpgrade !== false &&
    complexity === 'complex' &&
    normalizedAgent &&
    isSupportedModel(normalizedAgent) &&
    SUPPORTED_MODELS[normalizedAgent].tier === 'low'
  ) {
    const upgrade = UPGRADE_MODEL
    if (!allowedModels?.length || allowedModels.includes(upgrade)) {
      return upgrade
    }
  }

  if (normalizedAgent) {
    if (!allowedModels?.length || allowedModels.includes(normalizedAgent)) {
      return normalizedAgent
    }
  }

  return DEFAULT_MODEL
}

// normalizeLegacyAlias maps deprecated model ids (still referenced by
// older agents in Convex `agents` rows) to the real deployment name.
// Keep this list short — the goal is to migrate all consumers off the
// fictional names, not to grow a parallel registry.
function normalizeLegacyAlias(model: string | undefined | null): string | undefined {
  if (!model) return undefined
  const aliases: Record<string, SupportedModel> = {
    'gpt-5.4-mini': 'gpt-4o-mini',
    'gpt-5.4': 'gpt-5-mini',
    'claude-sonnet-4-6': 'claude-sonnet-4-5',
    'claude-opus-4-6': 'claude-opus-4-1',
  }
  return aliases[model] ?? model
}
