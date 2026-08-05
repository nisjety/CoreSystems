#!/usr/bin/env node
/**
 * G39 boot-time gate. Runs before `next dev` / `next start`.
 *
 * Next.js 16's `instrumentation.ts:register()` hook is not invoked reliably
 * in webpack dev mode, so we run the format-validation AND the cross-service
 * handshake here as synchronous gates on the process.
 *
 * Behaviour matches the two TS modules under `src/lib/server/`:
 *   - Production + bad key/401 → process.exit(1)
 *   - Anything else            → console.warn, continue
 *
 * Disable the handshake (e.g. when bringing up a cluster from cold without
 * integration-api yet) by setting `VEREVON_INTERNAL_KEY_HANDSHAKE=skip`.
 *
 * Keep this file dependency-free pure-JS — it must run on bare node before
 * any project deps are loaded.
 */

const PLACEHOLDER_PREFIXES = ['test', 'placeholder', 'change-me', 'your-', 'replace-me'];
const MIN_KEY_LENGTH = 32;

const CHECKS = [
  {
    envVars: ['INTERNAL_API_KEY', 'INTERNAL_SERVICE_SECRET'],
    purpose: 'verevon → Control Plane services (user-core, auth-core, billing-core, session-core)',
    required: true,
  },
  {
    envVars: ['AUTH_CORE_INTERNAL_API_KEY', 'INTEGRATION_CORE_INTERNAL_API_KEY'],
    purpose: 'verevon → integration-core (connect sessions, OAuth provider catalogue)',
    required: true,
  },
];

function classify(value) {
  const lowered = value.toLowerCase();
  for (const prefix of PLACEHOLDER_PREFIXES) {
    if (lowered === prefix || lowered.startsWith(`${prefix}-`) || lowered.startsWith(`${prefix}_`)) {
      return {
        ok: false,
        kind: 'placeholder',
        detail: `value looks like a placeholder ("${value.slice(0, 16)}…"). Set the real cluster-wide secret.`,
      };
    }
  }
  if (value.length < MIN_KEY_LENGTH) {
    return {
      ok: false,
      kind: 'too_short',
      detail: `value is only ${value.length} chars; the canonical secret is 64-char hex (min ${MIN_KEY_LENGTH}).`,
    };
  }
  return { ok: true };
}

function check(env) {
  const problems = [];
  const resolved = [];

  for (const check of CHECKS) {
    let picked = null;
    for (const name of check.envVars) {
      const raw = env[name];
      if (raw && raw.trim().length > 0) {
        picked = { name, value: raw.trim() };
        break;
      }
    }

    if (!picked) {
      if (check.required) {
        problems.push({
          kind: 'missing',
          envVarNames: check.envVars,
          purpose: check.purpose,
          detail: `none of [${check.envVars.join(', ')}] are set. Configure the cluster-wide secret.`,
        });
      }
      continue;
    }

    const verdict = classify(picked.value);
    if (verdict.ok) {
      resolved.push(picked.name);
    } else {
      problems.push({
        kind: verdict.kind,
        envVarNames: [picked.name],
        purpose: check.purpose,
        detail: verdict.detail,
      });
    }
  }

  return { ok: problems.length === 0, problems, resolved };
}

// ─── Phase 2: cross-service handshake ─────────────────────────────────────────

const HANDSHAKE_TARGETS = [
  {
    serviceName: 'integration-core',
    urlEnv: 'INTEGRATION_CORE_URL',
    defaultUrl: 'http://integration-api:3026',
    keyEnvs: ['AUTH_CORE_INTERNAL_API_KEY', 'INTEGRATION_CORE_INTERNAL_API_KEY'],
    path: '/api/v1/internal/whoami',
  },
];

function pickKey(env, names) {
  for (const name of names) {
    const v = env[name];
    if (v && v.trim().length > 0) {
      return v.trim();
    }
  }
  return null;
}

async function probeOne(target, env, timeoutMs) {
  const base = env[target.urlEnv]?.trim() || target.defaultUrl;
  const url = `${base.replace(/\/+$/, '')}${target.path}`;
  const key = pickKey(env, target.keyEnvs);
  if (!key) return null;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      method: 'GET',
      headers: { 'x-internal-api-key': key, accept: 'application/json' },
      signal: controller.signal,
    });
    if (response.status === 200) return null;
    if (response.status === 401) {
      return { serviceName: target.serviceName, url, kind: 'unauthorized', detail: `${target.serviceName} rejected the internal API key (401).` };
    }
    if (response.status === 403) {
      return { serviceName: target.serviceName, url, kind: 'forbidden', detail: `${target.serviceName} returned 403.` };
    }
    return { serviceName: target.serviceName, url, kind: 'unexpected_status', detail: `unexpected HTTP ${response.status}.` };
  } catch (err) {
    return { serviceName: target.serviceName, url, kind: 'transport_error', detail: `could not reach ${url}: ${err?.message ?? err}` };
  } finally {
    clearTimeout(timer);
  }
}

async function performHandshake(env) {
  const problems = [];
  const succeeded = [];
  for (const target of HANDSHAKE_TARGETS) {
    const problem = await probeOne(target, env, 3000);
    if (problem) problems.push(problem);
    else succeeded.push(target.serviceName);
  }
  return { ok: problems.length === 0, problems, succeeded };
}

async function runHandshake(env) {
  const mode = (env.VEREVON_INTERNAL_KEY_HANDSHAKE ?? '').toLowerCase();
  if (mode === 'skip') return;
  if (env.NEXT_PHASE === 'phase-production-build') return;
  if (env.NODE_ENV === 'test' && !env.VEREVON_ASSERT_KEYS_IN_TEST) return;

  const report = await performHandshake(env);
  if (report.ok) {
    console.log(`[verevon startup] internal API key handshake OK (${report.succeeded.join(', ')})`);
    return;
  }

  const strict = env.NODE_ENV === 'production' || mode === 'strict';
  const hasAuthFailure = report.problems.some((p) => p.kind === 'unauthorized' || p.kind === 'forbidden');
  const fatal = strict && hasAuthFailure;

  const formatted = report.problems
    .map((p) => `  - [${p.kind}] ${p.serviceName} ${p.url}: ${p.detail}`)
    .join('\n');

  const header = fatal
    ? `[verevon startup] FATAL: internal API key handshake failed (NODE_ENV=${env.NODE_ENV ?? 'unset'}, mode=${mode || 'default'})`
    : `[verevon startup] WARN: internal API key handshake reported problems`;

  console[fatal ? 'error' : 'warn'](`${header}\n${formatted}`);

  if (fatal) {
    process.exit(1);
  }
}

// ─── Entry ────────────────────────────────────────────────────────────────────

async function main() {
  const env = process.env;

  // Skip on build / test passes.
  if (env.NEXT_PHASE === 'phase-production-build') return;
  if (env.NODE_ENV === 'test' && !env.VEREVON_ASSERT_KEYS_IN_TEST) return;

  // Phase 1: format validation (synchronous, decisive).
  const result = check(env);
  if (result.ok) {
    console.log(`[verevon startup] internal API keys OK (${result.resolved.join(', ')})`);
  } else {
    const formatted = result.problems
      .map((p) => `  - [${p.kind}] ${p.envVarNames.join(' | ')}: ${p.detail} (for: ${p.purpose})`)
      .join('\n');
    const isProduction = env.NODE_ENV === 'production';
    const header = isProduction
      ? `[verevon startup] FATAL: internal API key validation failed (NODE_ENV=production)`
      : `[verevon startup] WARN: internal API key validation failed (NODE_ENV=${env.NODE_ENV ?? 'unset'} — continuing because not production)`;
    console[isProduction ? 'error' : 'warn'](`${header}\n${formatted}`);
    if (isProduction) {
      process.exit(1);
    }
  }

  // Phase 2: cross-service handshake (async, best-effort).
  await runHandshake(env);
}

main().catch((err) => {
  console.error('[verevon startup] check-internal-api-keys: unexpected error:', err?.message ?? err);
  // Don't let an internal bug here block dev startup; only exit on prod.
  if (process.env.NODE_ENV === 'production') {
    process.exit(1);
  }
});
