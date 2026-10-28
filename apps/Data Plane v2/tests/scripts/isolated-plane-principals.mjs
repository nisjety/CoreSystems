#!/usr/bin/env node
// QB-T08 — the quarry-edge row of the isolated real-authority registry, GENERATED from the committed
// principal policy (apps/Control Plane/config/plane-service-principals.json), never typed by hand.
//
// Why: the isolated runner (run-isolated-real-authority.sh) builds the registry Auth Core loads, and a
// hand-typed copy of a principal drifts from the committed policy. This generator copies every
// authority field of the committed row (audiences, tenancy, orgIds, retentionByAudience, scopes,
// scopesByAudience) and adds only a disposable credential. The one field it drops is `consumerEnv`,
// which names the launcher variable that carries the credential in production, not an Auth Core
// authority. A committed row that carries `allowAnyOrg: true` is refused, so the copy can never be a
// permissive one.
//
// Usage (the registry JSON is read from REAL_AUTHORITY_PLANE_PRINCIPALS_JSON, the credential from the
// environment variable NAMED by the third argument, and the merged JSON is written to stdout):
//   node isolated-plane-principals.mjs merge <committed-policy.json> <principal-id> <credential-env-name>
//
// The credential and the registry are secrets: this script prints nothing except the merged JSON on
// stdout, and its error messages name fields, never values.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MIN_CREDENTIAL_CHARS = 32;

/** The committed row as Auth Core reads it: every authority field, the launcher key removed, the credential added. */
export function generatePrincipalRow(policy, principalId, credential) {
  const row = policy?.[principalId];
  if (row === null || typeof row !== 'object' || Array.isArray(row)) {
    throw new Error(`the committed policy has no "${principalId}" row`);
  }
  if (row.allowAnyOrg === true) {
    throw new Error(`"${principalId}" is allowAnyOrg in the committed policy; a test registry must not copy an unchecked row`);
  }
  if (typeof credential !== 'string' || credential.length < MIN_CREDENTIAL_CHARS) {
    throw new Error(`the credential for "${principalId}" is missing or shorter than ${MIN_CREDENTIAL_CHARS} characters`);
  }
  // Drop the launcher key by name; every other field is copied as committed.
  const { consumerEnv: _launcherKey, ...authority } = row;
  return { ...structuredClone(authority), credential };
}

/** The registry with the generated row added. A row already present is refused: it is generated, not typed. */
export function mergeRegistry(registry, principalId, row) {
  if (registry === null || typeof registry !== 'object' || Array.isArray(registry)) {
    throw new Error('the isolated registry is not a JSON object');
  }
  if (Object.prototype.hasOwnProperty.call(registry, principalId)) {
    throw new Error(`"${principalId}" is already in the isolated registry; it must come from the generator only`);
  }
  return { ...registry, [principalId]: row };
}

function main(argv) {
  const [command, policyPath, principalId, credentialEnv] = argv;
  if (command !== 'merge' || !policyPath || !principalId || !credentialEnv) {
    process.stderr.write('usage: isolated-plane-principals.mjs merge <committed-policy.json> <principal-id> <credential-env-name>\n');
    return 2;
  }
  const policy = JSON.parse(readFileSync(policyPath, 'utf8'));
  const registry = JSON.parse(process.env.REAL_AUTHORITY_PLANE_PRINCIPALS_JSON ?? '{}');
  const row = generatePrincipalRow(policy, principalId, process.env[credentialEnv] ?? '');
  process.stdout.write(JSON.stringify(mergeRegistry(registry, principalId, row)));
  return 0;
}

// Run only when executed directly, so the parity test can import the functions.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`isolated-plane-principals: ${error.message}\n`);
    process.exitCode = 1;
  }
}
