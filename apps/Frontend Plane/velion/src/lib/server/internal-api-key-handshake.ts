/**
 * G39 Phase 2: cross-service startup handshake.
 *
 * The format-only validation (`assertInternalApiKeys`) catches obvious
 * problems — placeholders, missing keys, too-short keys — without leaving
 * the process. This module goes one step further: hit a counterpart
 * service's auth-gated endpoint with the configured key and confirm a 200.
 * That's the only way to prove key DRIFT — both sides have well-formed
 * keys that nonetheless don't match.
 *
 * Design rules:
 *   - **Best effort.** The probe must not block boot indefinitely. We use
 *     a short timeout (3s default) with no retries. A network glitch is
 *     not a key problem.
 *   - **Opt-in in dev, on by default in production.** Set
 *     `VEREVON_INTERNAL_KEY_HANDSHAKE=skip` to disable, or `=strict` to
 *     promote dev warnings to throws.
 *   - **Never throws on transport errors.** If `integration-api` is
 *     simply not running (network unreachable / DNS fail), log a
 *     `transport_error` and continue — that's an ops problem, not a key
 *     problem. We only throw on a definitive **401 / 403** response.
 */

interface HandshakeTarget {
  serviceName: string
  /** Base URL env var name. */
  urlEnv: string
  /** Default URL if env var is unset (in-cluster name). */
  defaultUrl: string
  /** Env var names from which to read the api key. First non-empty wins. */
  keyEnvs: string[]
  /** Relative path of the handshake endpoint. */
  path: string
}

const TARGETS: HandshakeTarget[] = [
  {
    serviceName: 'integration-core',
    urlEnv: 'INTEGRATION_CORE_URL',
    defaultUrl: 'http://integration-api:3026',
    keyEnvs: ['AUTH_CORE_INTERNAL_API_KEY', 'INTEGRATION_CORE_INTERNAL_API_KEY'],
    path: '/api/v1/internal/whoami',
  },
]

export interface HandshakeProblem {
  serviceName: string
  url: string
  kind: 'unauthorized' | 'forbidden' | 'transport_error' | 'unexpected_status'
  detail: string
}

export interface HandshakeReport {
  ok: boolean
  problems: HandshakeProblem[]
  succeeded: string[]
}

function pickKey(env: NodeJS.ProcessEnv, names: string[]): string | null {
  for (const name of names) {
    const v = env[name]
    if (v && v.trim().length > 0) {
      return v.trim()
    }
  }
  return null
}

async function probeOne(target: HandshakeTarget, env: NodeJS.ProcessEnv, timeoutMs: number): Promise<HandshakeProblem | null> {
  const base = env[target.urlEnv]?.trim() || target.defaultUrl
  const url = `${base.replace(/\/+$/, '')}${target.path}`
  const key = pickKey(env, target.keyEnvs)

  if (!key) {
    // No key configured for this target — the format assertion already
    // surfaced this as a `[missing]` problem. Skip here to avoid
    // double-reporting.
    return null
  }

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const response = await fetch(url, {
      method: 'GET',
      headers: {
        'x-internal-api-key': key,
        accept: 'application/json',
      },
      signal: controller.signal,
    })

    if (response.status === 200) {
      return null
    }
    if (response.status === 401) {
      return {
        serviceName: target.serviceName,
        url,
        kind: 'unauthorized',
        detail: `${target.serviceName} rejected the internal API key (401). The key in [${target.keyEnvs.join(' | ')}] does not match ${target.serviceName}'s cluster secret.`,
      }
    }
    if (response.status === 403) {
      return {
        serviceName: target.serviceName,
        url,
        kind: 'forbidden',
        detail: `${target.serviceName} returned 403. The key is accepted but lacks the required role.`,
      }
    }
    return {
      serviceName: target.serviceName,
      url,
      kind: 'unexpected_status',
      detail: `unexpected HTTP ${response.status} from ${url}.`,
    }
  } catch (err) {
    return {
      serviceName: target.serviceName,
      url,
      kind: 'transport_error',
      detail: `could not reach ${url}: ${err instanceof Error ? err.message : String(err)}`,
    }
  } finally {
    clearTimeout(timer)
  }
}

export async function performInternalApiKeyHandshake(
  env: NodeJS.ProcessEnv = process.env,
  timeoutMs = 3000,
): Promise<HandshakeReport> {
  const problems: HandshakeProblem[] = []
  const succeeded: string[] = []

  for (const target of TARGETS) {
    const problem = await probeOne(target, env, timeoutMs)
    if (problem) {
      problems.push(problem)
    } else {
      succeeded.push(target.serviceName)
    }
  }

  return { ok: problems.length === 0, problems, succeeded }
}

/**
 * Run the handshake and decide whether to surface errors. Called from
 * `instrumentation.ts:register()` after the synchronous format assertion.
 *
 * Behaviour matrix:
 *   - `VEREVON_INTERNAL_KEY_HANDSHAKE=skip`  → no-op
 *   - `NODE_ENV=production`                 → fatal on `unauthorized`/`forbidden`
 *   - `VEREVON_INTERNAL_KEY_HANDSHAKE=strict`→ fatal on `unauthorized`/`forbidden`
 *   - Other transport errors / dev mode     → log warn, continue
 */
export async function assertInternalApiKeyHandshake(
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const mode = (env.VEREVON_INTERNAL_KEY_HANDSHAKE ?? '').toLowerCase()
  if (mode === 'skip') {
    return
  }
  // Tests / build phases — defer to format assertion only.
  if (env.NEXT_PHASE === 'phase-production-build') {
    return
  }
  if (env.NODE_ENV === 'test' && !env.VEREVON_ASSERT_KEYS_IN_TEST) {
    return
  }

  const report = await performInternalApiKeyHandshake(env)
  if (report.ok) {
    // eslint-disable-next-line no-console
    console.log(`[verevon startup] internal API key handshake OK (${report.succeeded.join(', ')})`)
    return
  }

  const strict = env.NODE_ENV === 'production' || mode === 'strict'
  const formatted = report.problems
    .map((p) => `  - [${p.kind}] ${p.serviceName} ${p.url}: ${p.detail}`)
    .join('\n')

  // Distinguish: a key-mismatch is fatal in strict mode; a transport
  // failure is never fatal (might just mean the upstream isn't ready yet).
  const hasAuthFailure = report.problems.some(
    (p) => p.kind === 'unauthorized' || p.kind === 'forbidden',
  )

  const fatal = strict && hasAuthFailure
  const header = fatal
    ? `[verevon startup] FATAL: internal API key handshake failed (NODE_ENV=${env.NODE_ENV ?? 'unset'}, mode=${mode || 'default'})`
    : `[verevon startup] WARN: internal API key handshake reported problems`

  // eslint-disable-next-line no-console
  console[fatal ? 'error' : 'warn'](`${header}\n${formatted}`)

  if (fatal) {
    throw new Error(
      `verevon: refusing to start with mismatched internal API key(s). ${report.problems.length} probe failure(s); ${hasAuthFailure ? 'at least one was a 401/403.' : ''}`,
    )
  }
}
