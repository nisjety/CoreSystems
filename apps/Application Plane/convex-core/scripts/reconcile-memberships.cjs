#!/usr/bin/env node

const { createHmac, randomUUID } = require('node:crypto');

function parseOptions(argv) {
  let orgId = '';
  let confirmation = '';
  let apply = false;
  for (let index = 0; index < argv.length; index += 1) {
    switch (argv[index]) {
      case '--org':
        orgId = String(argv[index + 1] ?? '').trim();
        index += 1;
        break;
      case '--confirm-org':
        confirmation = String(argv[index + 1] ?? '').trim();
        index += 1;
        break;
      case '--apply':
        apply = true;
        break;
      default:
        throw new Error(`Unknown option: ${argv[index]}`);
    }
  }
  if (!orgId) throw new Error('--org <organization-id> is required');
  if (apply && confirmation !== orgId) {
    throw new Error('--apply requires --confirm-org with the exact organization id');
  }
  return { apply, orgId };
}

function normalizeAuthorityResponse(payload) {
  if (!payload || !Array.isArray(payload.members)) {
    throw new Error('Control Plane membership response is malformed');
  }
  return payload.members
    .filter((member) => member?.status === 'active')
    .filter((member) => typeof member?.user_id === 'string' && member.user_id.trim())
    .map((member) => ({
      userId: member.user_id.trim(),
      role: String(member.role ?? '').trim(),
    }));
}

function requireEnvironment(environment, name) {
  const value = String(environment[name] ?? '').trim();
  if (!value) throw new Error(`${name} must be configured`);
  return value;
}

async function run(argv = process.argv.slice(2), environment = process.env, fetchImpl = fetch) {
  const options = parseOptions(argv);
  const authBaseUrl = requireEnvironment(environment, 'CONTROL_PLANE_AUTH_CORE_URL').replace(/\/$/, '');
  const convexBaseUrl = requireEnvironment(environment, 'CONVEX_HTTP_ACTIONS_URL').replace(/\/$/, '');
  const authToken = requireEnvironment(environment, 'APPLICATION_RECONCILER_AUTH_TOKEN');
  const convexKey = requireEnvironment(environment, 'CONVEX_RECONCILIATION_KEY');
  const signal = AbortSignal.timeout(10_000);

  const authorityResponse = await fetchImpl(
    `${authBaseUrl}/api/v1/internal/membership/organizations/${encodeURIComponent(options.orgId)}/members`,
    {
      headers: {
        'X-Service-Id': 'application-reconciler',
        'X-Service-Token': authToken,
      },
      redirect: 'manual',
      signal,
    },
  );
  if (!authorityResponse.ok) {
    throw new Error(`Control Plane membership read failed with HTTP ${authorityResponse.status}`);
  }
  const authoritativeMembers = normalizeAuthorityResponse(await authorityResponse.json());
  const observedAt = Date.now();

  const requestPath = '/api/operator/reconcile-memberships';
  const nonce = randomUUID();
  const body = JSON.stringify({
    externalOrgId: options.orgId,
    authoritativeMembers,
    observedAt,
    apply: options.apply,
    confirmOrgId: options.apply ? options.orgId : undefined,
  });
  const canonical = `${observedAt}\nPOST\n${requestPath}\n${options.orgId}\n${nonce}\n${body}`;
  const signature = createHmac('sha256', convexKey).update(canonical).digest('hex');
  const convexResponse = await fetchImpl(
    `${convexBaseUrl}${requestPath}`,
    {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Reconciliation-Timestamp': String(observedAt),
        'X-Reconciliation-Nonce': nonce,
        'X-Reconciliation-Org': options.orgId,
        'X-Reconciliation-Signature': signature,
      },
      body,
      redirect: 'manual',
      signal,
    },
  );
  if (!convexResponse.ok) {
    throw new Error(`Convex reconciliation failed with HTTP ${convexResponse.status}`);
  }
  return await convexResponse.json();
}

async function main() {
  try {
    const result = await run();
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`Membership reconciliation failed: ${error instanceof Error ? error.message : 'unknown error'}\n`);
    process.exitCode = 1;
  }
}

if (require.main === module) void main();

module.exports = {
  normalizeAuthorityResponse,
  parseOptions,
  run,
};
