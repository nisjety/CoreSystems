/**
 * Boot-time validation of velion's internal-API-key environment.
 *
 * G39 (see `velion-gap.md` §10): two production incidents have now been
 * caused by velion shipping a placeholder internal API key and the
 * mismatch only surfacing the first time a user clicked something —
 * G30 (`INTERNAL_API_KEY=placeholder`) and §8.23 (`AUTH_CORE_INTERNAL_API_KEY=test`).
 * In both cases a one-line boot-time assertion would have surfaced the
 * problem in <1s at container start, instead of at first user click.
 *
 * This module is invoked from `instrumentation.ts:register()` so the
 * check runs once per Next.js process start. It does NOT make any
 * network calls — keep this fast and dependency-free. The cross-service
 * handshake (G39 Phase 2) is a separate concern.
 *
 * Policy:
 *   - In production (`NODE_ENV=production`), any failure throws and the
 *     process exits via Next.js's startup error path.
 *   - In dev / test, failures are logged at WARN level and the process
 *     continues — local stacks often run with weak keys for convenience.
 */

const PLACEHOLDER_PREFIXES = ['test', 'placeholder', 'change-me', 'your-', 'replace-me']
const MIN_KEY_LENGTH = 32

interface KeyCheck {
  /** Env var name(s); first non-empty value wins. */
  envVars: string[]
  /** Free-form description for log lines. */
  purpose: string
  /** When true, missing keys are a failure. False → optional. */
  required: boolean
}

const CHECKS: KeyCheck[] = [
  {
    envVars: ['INTERNAL_API_KEY', 'INTERNAL_SERVICE_SECRET'],
    purpose: 'velion → Control Plane services (user-core, auth-core, billing-core, session-core)',
    required: true,
  },
  {
    envVars: ['AUTH_CORE_INTERNAL_API_KEY', 'INTEGRATION_CORE_INTERNAL_API_KEY'],
    purpose: 'velion → integration-core (connect sessions, OAuth provider catalogue)',
    required: true,
  },
]

interface KeyProblem {
  kind: 'missing' | 'placeholder' | 'too_short'
  envVarNames: string[]
  purpose: string
  detail: string
}

function classify(value: string): { ok: true } | { ok: false; problem: Omit<KeyProblem, 'envVarNames' | 'purpose'> } {
  const lowered = value.toLowerCase()
  for (const prefix of PLACEHOLDER_PREFIXES) {
    if (lowered === prefix || lowered.startsWith(`${prefix}-`) || lowered.startsWith(`${prefix}_`)) {
      return {
        ok: false,
        problem: {
          kind: 'placeholder',
          detail: `value looks like a placeholder ("${value.slice(0, 16)}…"). Set the real cluster-wide secret.`,
        },
      }
    }
  }
  if (value.length < MIN_KEY_LENGTH) {
    return {
      ok: false,
      problem: {
        kind: 'too_short',
        detail: `value is only ${value.length} chars; the canonical secret is 64-char hex (min ${MIN_KEY_LENGTH}).`,
      },
    }
  }
  return { ok: true }
}

export interface AssertionResult {
  ok: boolean
  problems: KeyProblem[]
  /** Names of env vars that resolved successfully (for log breadcrumbs). */
  resolved: string[]
}

export function checkInternalApiKeys(env: NodeJS.ProcessEnv = process.env): AssertionResult {
  const problems: KeyProblem[] = []
  const resolved: string[] = []

  for (const check of CHECKS) {
    // Re-implement pickValue against the supplied env so the function is
    // unit-testable without mutating process.env.
    let picked: { name: string; value: string } | null = null
    for (const name of check.envVars) {
      const raw = env[name]
      if (raw && raw.trim().length > 0) {
        picked = { name, value: raw.trim() }
        break
      }
    }

    if (!picked) {
      if (check.required) {
        problems.push({
          kind: 'missing',
          envVarNames: check.envVars,
          purpose: check.purpose,
          detail: `none of [${check.envVars.join(', ')}] are set. Configure the cluster-wide secret.`,
        })
      }
      continue
    }

    const verdict = classify(picked.value)
    if (verdict.ok) {
      resolved.push(picked.name)
    } else {
      problems.push({
        envVarNames: [picked.name],
        purpose: check.purpose,
        ...verdict.problem,
      })
    }
  }

  return { ok: problems.length === 0, problems, resolved }
}

/**
 * Run the assertion and either throw (in production) or warn (in dev).
 * Safe to call multiple times — the check is idempotent and pure.
 */
export function assertInternalApiKeys(env: NodeJS.ProcessEnv = process.env): void {
  // Skip during static-analysis / type-check passes. Next.js sets
  // `NEXT_PHASE=phase-production-build` while bundling; we don't want a
  // production build to fail just because the build host doesn't have the
  // production secrets injected.
  if (env.NEXT_PHASE === 'phase-production-build') {
    return
  }
  // Avoid making `next build` brittle in test runners that don't inject env.
  if (env.NODE_ENV === 'test' && !env.VELION_ASSERT_KEYS_IN_TEST) {
    return
  }

  const result = checkInternalApiKeys(env)

  if (result.ok) {
    if (env.NODE_ENV === 'production') {
      // eslint-disable-next-line no-console
      console.log(
        `[velion startup] internal API keys OK (${result.resolved.join(', ')})`,
      )
    }
    return
  }

  const formatted = result.problems
    .map((p) => `  - [${p.kind}] ${p.envVarNames.join(' | ')}: ${p.detail} (for: ${p.purpose})`)
    .join('\n')

  const isProduction = env.NODE_ENV === 'production'
  const header = isProduction
    ? `[velion startup] FATAL: internal API key validation failed (NODE_ENV=production)`
    : `[velion startup] WARN: internal API key validation failed (NODE_ENV=${env.NODE_ENV ?? 'unset'} — continuing because not production)`

  // eslint-disable-next-line no-console
  console[isProduction ? 'error' : 'warn'](`${header}\n${formatted}`)

  if (isProduction) {
    throw new Error(
      `velion: refusing to start with invalid internal API key(s). ${result.problems.length} problem(s) detected.`,
    )
  }
}
