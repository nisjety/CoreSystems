import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { authorizePlaneServicePrincipal } from './plane-service-principal';
import { planeScopesForRole } from './plane-token-scopes';

// W3-EP-14-S6-grant: inference-core writes EP-14 request snapshots to
// session-core with `session:request-snapshots:write` (inference-core
// request_capture.rs WRITE_SCOPE; session-core request_snapshots.rs checks it).
// The grant adds that one scope to the inference-core row, on session-core only,
// and makes the session-core lane persistent (session-core refuses a durable
// write from a zero-retention token). The policy file is the system under test;
// it is outside the Docker build context, so the pin runs where it exists.
const POLICY_PATH = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);
type Row = {
  tenancy?: string;
  audiences?: string[];
  consumerEnv?: string;
  orgIds?: string[];
  scopes?: string[];
  scopesByAudience?: Record<string, string[]>;
  retentionByAudience?: Record<string, string>;
  tokenExchange?: unknown;
  apiKeyExchange?: unknown;
};
const policy: Record<string, Row> | null = existsSync(POLICY_PATH)
  ? (JSON.parse(readFileSync(POLICY_PATH, 'utf8')) as Record<string, Row>)
  : null;
const describeIfPolicy = policy ? describe : describe.skip;

const SCOPE = 'session:request-snapshots:write';
const CREDENTIAL = 'test-only-inference-core-credential-0001';

function scopesOf(row: Row): string[] {
  return [
    ...(row.scopes ?? []),
    ...Object.values(row.scopesByAudience ?? {}).flat(),
  ];
}

describeIfPolicy(
  'inference-core request-snapshot grant (W3-EP-14-S6-grant)',
  () => {
    const row = policy?.['inference-core'] as Row;

    it('the inference-core row is exactly routing:read plus the snapshot write, on session-core alone, persistent', () => {
      expect(row).toMatchObject({
        tenancy: 'directory',
        audiences: ['session-core'],
        consumerEnv: 'INFERENCE_ROUTING_SERVICE_API_KEY',
        orgIds: [],
        scopes: ['routing:read', SCOPE],
        scopesByAudience: { 'session-core': ['routing:read', SCOPE] },
        retentionByAudience: { 'session-core': 'persistent' },
      });
      expect(Object.keys(row.scopesByAudience ?? {})).toEqual(['session-core']);
      expect(row.tokenExchange).toBeUndefined();
      expect(row.apiKeyExchange).toBeUndefined();
    });

    it('no other principal holds the snapshot write scope on any audience', () => {
      const holders = Object.entries(policy ?? {})
        .filter(
          ([, entry]) =>
            entry &&
            typeof entry === 'object' &&
            scopesOf(entry).includes(SCOPE),
        )
        .map(([name]) => name);
      expect(holders).toEqual(['inference-core']);
    });

    it('the real authorizer mints the snapshot write for session-core as a persistent (zdr false) token', () => {
      const registry = JSON.stringify({
        'inference-core': { ...row, credential: CREDENTIAL },
      });
      const authorized = authorizePlaneServicePrincipal(registry, {
        serviceId: 'inference-core',
        credential: CREDENTIAL,
        audience: 'session-core',
        orgId: 'org-a',
        requestedScopes: [SCOPE],
        reason: 'request snapshot capture',
      });
      expect(authorized.scopes).toEqual([SCOPE]);
      expect(authorized.retention).toBe('persistent');
      expect(authorized.zdr).toBe(false);
    });

    it('the real authorizer refuses the snapshot write on any other audience', () => {
      const registry = JSON.stringify({
        'inference-core': { ...row, credential: CREDENTIAL },
      });
      for (const audience of [
        'inference-core',
        'data-plane',
        'model-gateway',
        'execution-core',
        'cost-core',
      ]) {
        expect(() =>
          authorizePlaneServicePrincipal(registry, {
            serviceId: 'inference-core',
            credential: CREDENTIAL,
            audience,
            orgId: 'org-a',
            requestedScopes: [SCOPE],
            reason: 'request snapshot capture',
          }),
        ).toThrow();
      }
    });

    it('no interactive role ever derives the snapshot write scope', () => {
      for (const role of ['owner', 'admin', 'member', 'viewer', null]) {
        for (const audience of [
          'session-core',
          'inference-core',
          'execution-core',
          'data-plane',
        ]) {
          expect(planeScopesForRole(role, audience as never)).not.toContain(
            SCOPE,
          );
        }
      }
    });
  },
);
