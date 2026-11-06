/**
 * Replay of a saved run with new variables (slice 10).
 *
 * A saved run is a sequence of typed steps (navigate / click / type / read).
 * Replaying substitutes `{{variable}}` placeholders with caller-supplied
 * values and returns the steps to re-issue. It fails closed at the first
 * step whose action, selector or URL does not match the recorded shape, or
 * whose variable is missing — never silently skipping or guessing. This is
 * pure composition over the existing task-start endpoints; no model call is
 * made per step.
 */

export type ReplayStep = {
  kind: 'navigate' | 'click' | 'type' | 'read'
  /** For navigate: the recorded URL. For type: the recorded text with {{vars}}. */
  value?: string
  /** For click/type: the recorded element selector. */
  selector?: string
}

export type ReplayPlan = {
  steps: ReplayStep[]
}

export type ReplayVariables = Record<string, string>

const variablePattern = /\{\{([A-Za-z][A-Za-z0-9_]*)\}\}/g

function substitute(text: string, variables: ReplayVariables): { ok: true; value: string } | { ok: false; missing: string } {
  let missing = ''
  const value = text.replace(variablePattern, (_match, name: string) => {
    const supplied = variables[name]
    if (supplied === undefined) {
      if (!missing) missing = name
      return ''
    }
    return supplied
  })
  return missing ? { ok: false, missing } : { ok: true, value }
}

export type ReplayResult =
  | { ok: true; steps: ReplayStep[] }
  | { ok: false; stepIndex: number; reason: string }

/**
 * Build the steps to re-issue. Every recorded step is checked for shape
 * (known kind + required fields) before substitution; the first mismatch
 * stops the replay.
 */
export function buildReplay(plan: ReplayPlan, variables: ReplayVariables): ReplayResult {
  const steps: ReplayStep[] = []
  for (let index = 0; index < plan.steps.length; index++) {
    const step = plan.steps[index]
    if (!step || !['navigate', 'click', 'type', 'read'].includes(step.kind)) {
      return { ok: false, stepIndex: index, reason: 'unknown_step_kind' }
    }
    if ((step.kind === 'click' || step.kind === 'type') && !step.selector?.trim()) {
      return { ok: false, stepIndex: index, reason: 'selector_required' }
    }
    if (step.kind === 'navigate' && !step.value?.trim()) {
      return { ok: false, stepIndex: index, reason: 'url_required' }
    }
    const substituted: ReplayStep = { kind: step.kind }
    if (step.selector) substituted.selector = step.selector
    if (step.value !== undefined) {
      const result = substitute(step.value, variables)
      if (!result.ok) return { ok: false, stepIndex: index, reason: `missing_variable:${result.missing}` }
      // A substituted navigate target must still be a plain http(s) URL.
      if (step.kind === 'navigate') {
        try {
          const url = new URL(result.value)
          if (url.protocol !== 'http:' && url.protocol !== 'https:') {
            return { ok: false, stepIndex: index, reason: 'unsafe_url' }
          }
        } catch {
          return { ok: false, stepIndex: index, reason: 'unsafe_url' }
        }
      }
      substituted.value = result.value
    }
    steps.push(substituted)
  }
  return { ok: true, steps }
}
