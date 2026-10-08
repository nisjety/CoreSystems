import {
  BadRequestException,
  ForbiddenException,
  Logger,
} from '@nestjs/common';
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// The retention-posture block at the end of this file drives the real token
// controllers. Like plane-token-exchange.composition.spec.ts, it replaces only
// the two modules the plane controller pulls in for its unrelated interactive
// path (Better Auth and the membership lookup).
jest.mock('./auth', () => ({ auth: { api: { getSession: jest.fn() } } }));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: jest.fn(),
}));

import { ConvexTokenService } from './convex-token.service';
import { DataPlaneAuthorizationController } from './data-plane-authorization.controller';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';
import { ModelPlaneTokenController } from './model-plane-token.controller';
import {
  authorizePlaneServicePrincipal,
  authorizePlaneServiceTokenExchange,
  loadPlaneServicePrincipalRegistry,
} from './plane-service-principal';
import { PlaneTokenController } from './plane-token.controller';

// Deployment contract for the delegated-identity rows of the checked-in
// principal policy (apps/Control Plane/config/plane-service-principals.json),
// which the launcher turns into PLANE_SERVICE_PRINCIPALS_JSON. The launcher
// copies `tokenExchange` through as-is and Auth Core only reads it at request
// time, so a wrong row is invisible until a live run fails closed. These
// assertions make the rows a tested precondition instead of a deploy-day
// discovery (security review B-4/T-6 and T-4).
//
// The policy file lives outside this service's Docker build context. It is the
// system under test, not a golden, so it cannot be vendored: wherever it is
// absent the spec skips, EXCEPT under REQUIRE_CONTRACT, where the whole suite
// fails to run. A contract that quietly skips is not a contract.
const policyPath = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);
type PolicyEntry = {
  audiences?: string[];
  allowAnyOrg?: boolean;
  tenancy?: string;
  orgIds?: string[];
  consumerEnv?: string;
  scopes?: string[];
  tokenExchange?: Record<string, string[]>;
  retentionByAudience?: Record<string, string>;
  scopesByAudience?: Record<string, string[]>;
};
const policy: Record<string, PolicyEntry> | null = existsSync(policyPath)
  ? (JSON.parse(readFileSync(policyPath, 'utf8')) as Record<
      string,
      PolicyEntry
    >)
  : null;

if (!policy && process.env.REQUIRE_CONTRACT) {
  throw new Error(
    `REQUIRE_CONTRACT is set but ${policyPath} is absent; run from a full checkout`,
  );
}
const describeIfPolicy = policy ? describe : describe.skip;

describeIfPolicy('principal policy delegated-identity rows', () => {
  it('grants token exchange to model-execution alone, for ingestion, from a session-core subject only', () => {
    const holders = Object.entries(policy ?? {}).filter(
      ([, principal]) => principal.tokenExchange !== undefined,
    );
    expect(holders.map(([name]) => name)).toEqual(['model-execution']);
    expect(holders[0][1].tokenExchange).toEqual({
      ingestion: ['session-core'],
    });
  });

  it('accepts the policy shape once the launcher adds credentials, and authorizes the exchange', () => {
    const registry = Object.fromEntries(
      Object.entries(policy ?? {}).map(([name, principal]) => [
        name,
        { ...principal, credential: `test-only-${name}-key` },
      ]),
    );
    const request = {
      serviceId: 'model-execution',
      credential: 'test-only-model-execution-key',
      audience: 'ingestion',
      orgId: 'org-a',
      requestedScopes: ['integration:read'],
      reason: 'execution-core integration:read',
      subjectAudience: 'session-core',
    };

    expect(
      authorizePlaneServiceTokenExchange(JSON.stringify(registry), request),
    ).toMatchObject({ serviceId: 'model-execution', orgId: 'org-a' });
    // The exchange never mints a write credential, whatever the registry says.
    expect(() =>
      authorizePlaneServiceTokenExchange(JSON.stringify(registry), {
        ...request,
        requestedScopes: ['integration:write'],
      }),
    ).toThrow(/not authorized/i);
  });

  it('gives the mail-task bridge principal persistent session-core retention, or every prepare call fails closed', () => {
    // session-core refuses a zero-retention caller for a non-:read scope, and
    // Auth Core defaults an unconfigured audience to zdr. The row exists since
    // plan task J3 (still dark: nothing presents it until mail-to-task is on),
    // so this binds unconditionally now; a deleted row fails here, not live.
    const bridge = policy?.['model-gateway-mail-task-bridge'];
    expect(bridge).toBeDefined();
    if (!bridge) return;
    expect(bridge.retentionByAudience?.['session-core']).toBe('persistent');
    expect(bridge.audiences).toEqual(['session-core']);
    expect(bridge.scopesByAudience?.['session-core']).toEqual([
      'session:user-owned-thread-prepare',
    ]);
  });

  it('keeps model-gateway own session-core credential zero-retention', () => {
    // Flipping this to persistent would silently widen every credential that
    // principal already mints (terminalize, heartbeat); the bridge has its own.
    const gateway = policy?.['model-gateway'];
    if (!gateway?.retentionByAudience?.['session-core']) return;
    expect(gateway.retentionByAudience['session-core']).toBe('zdr');
  });
});

// The agent runtime's web tools (execution-core `web_search` / `web_fetch`)
// mint `search:read` and `extract:read` on the Quarry audience and quarry-edge
// checks exactly those scopes on /v1/search and /v1/extract
// (Quarry-v2 quarry-edge auth.rs `require_service_route_scope`). Without the
// grant Auth Core answers 403 and execution-core's honest `cap.tool.http`
// probe keeps web search unavailable in every Do run (A6 proof, 2026-10-01).
// The grant is deliberately one principal, one audience, two named scopes, no
// wildcard: these assertions fail if it widens or moves.
const webToolScopes = ['extract:read', 'search:read'];

function principalNames(): string[] {
  return Object.keys(policy ?? {});
}

function holdersOfScope(scope: string): string[] {
  return Object.entries(policy ?? {})
    .filter(
      ([, principal]) =>
        (principal.scopes ?? []).includes(scope) ||
        Object.values(principal.scopesByAudience ?? {}).some((scopes) =>
          scopes.includes(scope),
        ),
    )
    .map(([name]) => name)
    .sort();
}

function registryWithTestCredentials(): string {
  return JSON.stringify(
    Object.fromEntries(
      Object.entries(policy ?? {}).map(([name, principal]) => [
        name,
        { ...principal, credential: `test-only-${name}-key` },
      ]),
    ),
  );
}

function mintRequest(
  serviceId: string,
  audience: string,
  scope: string,
  orgId = 'org-a',
) {
  return {
    serviceId,
    credential: `test-only-${serviceId}-key`,
    audience,
    orgId,
    requestedScopes: [scope],
    reason: 'execution-core Quarry access',
  };
}

describeIfPolicy('principal policy Quarry web-tool grant', () => {
  it('grants search:read and extract:read to execution-core on the quarry audience, beside its existing scopes', () => {
    const principal = policy?.['execution-core'];
    expect(principal?.scopesByAudience?.quarry).toEqual([
      'browser:execute',
      'extract:read',
      'scrape:write',
      'search:read',
    ]);
    for (const scope of webToolScopes) {
      expect(principal?.scopes).toContain(scope);
    }
  });

  it('never gives execution-core the web-tool scopes on any other audience', () => {
    const principal = policy?.['execution-core'];
    const others = Object.entries(principal?.scopesByAudience ?? {}).filter(
      ([audience]) => audience !== 'quarry',
    );
    expect(others.length).toBeGreaterThan(0);
    for (const [audience, scopes] of others) {
      for (const scope of webToolScopes) {
        expect({ audience, has: scopes.includes(scope) }).toEqual({
          audience,
          has: false,
        });
      }
    }
  });

  it('leaves extract:read with execution-core alone and search:read with execution-core and model-gateway alone', () => {
    expect(holdersOfScope('extract:read')).toEqual(['execution-core']);
    expect(holdersOfScope('search:read')).toEqual([
      'execution-core',
      'model-gateway',
    ]);
  });

  it('only ever places the web-tool scopes under the quarry audience, for every principal', () => {
    for (const name of principalNames()) {
      for (const [audience, scopes] of Object.entries(
        policy?.[name]?.scopesByAudience ?? {},
      )) {
        if (audience === 'quarry') continue;
        for (const scope of webToolScopes) {
          expect({ name, audience, has: scopes.includes(scope) }).toEqual({
            name,
            audience,
            has: false,
          });
        }
      }
    }
  });

  it('carries no wildcard scope anywhere, and keeps execution-core zero-retention for Quarry', () => {
    for (const name of principalNames()) {
      const principal = policy?.[name];
      const everyScope = [
        ...(principal?.scopes ?? []),
        ...Object.values(principal?.scopesByAudience ?? {}).flat(),
      ];
      for (const scope of everyScope) {
        expect({ name, scope, wildcard: scope.includes('*') }).toEqual({
          name,
          scope,
          wildcard: false,
        });
      }
    }
    // Quarry reads the zdr posture from the request body, not the token; the
    // principal's own posture stays zero-retention so the grant adds no
    // persistence authority.
    expect(policy?.['execution-core']?.retentionByAudience?.quarry).toBe('zdr');
  });

  it('lets Auth Core mint both scopes for execution-core on quarry, for the readiness org and for a run org', () => {
    // The health probe mints with org "global", a pseudo-organization no
    // directory holds, so it is a fixed entry; a run mints with its own org, which
    // the directory vouches for. Both must work, so pin them with the behaviour.
    expect(policy?.['execution-core']?.tenancy).toBe('directory');
    expect(policy?.['execution-core']?.orgIds).toContain('global');
    expect(policy?.['execution-core']?.allowAnyOrg).toBeUndefined();
    const registry = registryWithTestCredentials();
    for (const [orgId, orgBinding] of [
      ['global', 'fixed'],
      ['org-run-1234', 'directory'],
    ] as const) {
      for (const scope of webToolScopes) {
        expect(
          authorizePlaneServicePrincipal(
            registry,
            mintRequest('execution-core', 'quarry', scope, orgId),
          ),
        ).toMatchObject({
          serviceId: 'execution-core',
          orgId,
          orgBinding,
          scopes: [scope],
          zdr: true,
        });
      }
    }
  });

  it('refuses every other principal and audience for extract:read, and every other principal for search:read bar model-gateway', () => {
    const registry = registryWithTestCredentials();
    const allowed = new Set([
      'execution-core|quarry|extract:read',
      'execution-core|quarry|search:read',
      'model-gateway|quarry|search:read',
    ]);
    const audiences = new Set<string>(['quarry', 'data-plane', 'session-core']);
    for (const name of principalNames()) {
      for (const audience of policy?.[name]?.audiences ?? []) {
        audiences.add(audience);
      }
    }
    let refused = 0;
    for (const name of principalNames()) {
      for (const audience of audiences) {
        for (const scope of webToolScopes) {
          const permitted = allowed.has(`${name}|${audience}|${scope}`);
          const attempt = () =>
            authorizePlaneServicePrincipal(
              registry,
              mintRequest(name, audience, scope),
            );
          if (permitted) {
            expect(attempt).not.toThrow();
          } else {
            expect(attempt).toThrow(/not authorized/i);
            refused += 1;
          }
        }
      }
    }
    // Every (principal, audience, scope) combination outside the allow-list
    // was exercised, not skipped.
    expect(refused).toBe(
      principalNames().length * audiences.size * webToolScopes.length -
        allowed.size,
    );
  });
});

// Mail-to-task (plan task J3), prepared dark. Two Auth Core principals exist
// so a later switch-on needs no policy change, and neither can reach anything
// else: the thread-prepare bridge mints one session-core scope, the gateway
// BFF mints one model-gateway scope. A third credential, the integration
// worker's, lives in a DIFFERENT registry (user-core's inline
// USER_CORE_SERVICE_CREDENTIALS in docker-compose.yml) and is pinned below by
// reading that file. These assertions are what keeps "dark" meaning "narrow".
const mailTaskBridge = 'model-gateway-mail-task-bridge';
const mailTaskGateway = 'verevon-gateway';
const mailTaskScopeOf: Record<string, string> = {
  [mailTaskBridge]: 'session:user-owned-thread-prepare',
  [mailTaskGateway]: 'mail-task:prepare-thread',
};
const composePath = join(__dirname, '../../../docker-compose.yml');

function everyScopeOf(name: string): string[] {
  const principal = policy?.[name];
  return [
    ...(principal?.scopes ?? []),
    ...Object.values(principal?.scopesByAudience ?? {}).flat(),
  ];
}

describeIfPolicy('principal policy mail-to-task principals (dark)', () => {
  it('registers the thread-prepare bridge for session-core alone, persistent, with its one scope', () => {
    const principal = policy?.[mailTaskBridge];
    expect(principal).toMatchObject({
      tenancy: 'directory',
      orgIds: [],
      audiences: ['session-core'],
      consumerEnv: 'MAIL_TASK_BRIDGE_SERVICE_API_KEY',
      scopes: ['session:user-owned-thread-prepare'],
      scopesByAudience: {
        'session-core': ['session:user-owned-thread-prepare'],
      },
      retentionByAudience: { 'session-core': 'persistent' },
    });
    expect(principal?.tokenExchange).toBeUndefined();
  });

  it('registers the gateway BFF for model-gateway alone, zero-retention, with its one scope', () => {
    // model-gateway's prepare-thread route reads neither the token's retention
    // nor its org beyond equality with the request; the thread itself is
    // persisted by the bridge's own persistent credential and a signed Control
    // decision. zdr keeps this credential from carrying any persistence
    // authority it does not use.
    const principal = policy?.[mailTaskGateway];
    expect(principal).toMatchObject({
      tenancy: 'directory',
      orgIds: [],
      audiences: ['model-gateway'],
      consumerEnv: 'MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY',
      scopes: ['mail-task:prepare-thread'],
      scopesByAudience: { 'model-gateway': ['mail-task:prepare-thread'] },
      retentionByAudience: { 'model-gateway': 'zdr' },
    });
    expect(principal?.tokenExchange).toBeUndefined();
  });

  it('never lets either principal hold integration:write, a browser grant, or any scope beyond the one named', () => {
    for (const name of [mailTaskBridge, mailTaskGateway]) {
      const scopes = everyScopeOf(name);
      expect({ name, scopes: [...new Set(scopes)] }).toEqual({
        name,
        scopes: [mailTaskScopeOf[name]],
      });
      for (const scope of scopes) {
        expect({
          name,
          scope,
          forbidden: /^(integration:|browser:)/.test(scope),
        }).toEqual({ name, scope, forbidden: false });
      }
      expect(Object.keys(policy?.[name]?.scopesByAudience ?? {})).toHaveLength(
        1,
      );
    }
    // The gateway principal in particular holds no session scope at all, and
    // the bridge holds no scope on any audience but session-core.
    expect(
      everyScopeOf(mailTaskGateway).filter((scope) =>
        scope.startsWith('session:'),
      ),
    ).toEqual([]);
    expect(
      Object.keys(policy?.[mailTaskBridge]?.scopesByAudience ?? {}),
    ).toEqual(['session-core']);
  });

  it('mints the one named scope for each principal and refuses everything else', () => {
    const registry = registryWithTestCredentials();
    const cases: Array<[string, string, string, boolean]> = [
      [
        mailTaskBridge,
        'session-core',
        'session:user-owned-thread-prepare',
        false,
      ],
      [mailTaskGateway, 'model-gateway', 'mail-task:prepare-thread', true],
    ];
    for (const [name, audience, scope, zdr] of cases) {
      expect(
        authorizePlaneServicePrincipal(registry, {
          ...mintRequest(name, audience, scope, 'org-a'),
          reason: 'mail-task thread bridge',
        }),
      ).toMatchObject({
        serviceId: name,
        orgId: 'org-a',
        scopes: [scope],
        zdr,
      });
    }
    const refusals: Array<[string, string, string]> = [];
    for (const name of [mailTaskBridge, mailTaskGateway]) {
      for (const audience of [
        'session-core',
        'model-gateway',
        'quarry',
        'ingestion',
        'data-plane',
        'capability-core',
        'sandbox-manager',
      ]) {
        for (const scope of [
          'session:write',
          'session:heartbeat',
          'session:terminalize',
          'session:user-owned-thread-prepare',
          'mail-task:prepare-thread',
          'models:invoke',
          'integration:write',
          'integration:read',
          'browser:execute',
          'scrape:write',
          'search:read',
          'extract:read',
          'documents:write',
        ]) {
          if (
            (name === mailTaskBridge &&
              audience === 'session-core' &&
              scope === mailTaskScopeOf[name]) ||
            (name === mailTaskGateway &&
              audience === 'model-gateway' &&
              scope === mailTaskScopeOf[name])
          ) {
            continue;
          }
          refusals.push([name, audience, scope]);
        }
      }
    }
    expect(refusals.length).toBeGreaterThan(150);
    for (const [name, audience, scope] of refusals) {
      expect(() =>
        authorizePlaneServicePrincipal(
          registry,
          mintRequest(name, audience, scope),
        ),
      ).toThrow(/not authorized/i);
    }
  });

  it('gives every principal its own consumer variable, so no two credentials share a published name', () => {
    const names = principalNames().map((name) => policy?.[name]?.consumerEnv);
    expect(
      names.every((value) => typeof value === 'string' && value !== ''),
    ).toBe(true);
    expect(new Set(names).size).toBe(names.length);
  });

  it('keeps the integration worker credential in user-core own registry, scoped to mail-task:resolve alone', () => {
    // A different registry from Auth Core's `verevon-gateway` row above: this
    // one is user-core's inline USER_CORE_SERVICE_CREDENTIALS, validated by
    // user-core's service_auth.go (token of 32+ characters, scopes enforced per
    // route, `mail-task:resolve` for /internal/mail-task/resolve only).
    if (!existsSync(composePath)) {
      if (process.env.REQUIRE_CONTRACT) {
        throw new Error(
          `REQUIRE_CONTRACT is set but ${composePath} is absent; run from a full checkout`,
        );
      }
      return;
    }
    const lines = readFileSync(composePath, 'utf8').split(/\r?\n/);
    const keyIndex = lines.findIndex((line) =>
      /^\s*USER_CORE_SERVICE_CREDENTIALS:\s*>-\s*$/.test(line),
    );
    expect(keyIndex).toBeGreaterThan(-1);
    const rawRegistry = lines[keyIndex + 1].trim();
    const placeholder = 'x'.repeat(40);
    const credentials = JSON.parse(
      rawRegistry.replace(/\$\{[^}]*\}/g, placeholder),
    ) as Array<{
      principal: string;
      audience: string;
      token: string;
      scopes: string[];
    }>;
    const workers = credentials.filter(
      (credential) => credential.principal === 'integration-mail-task-worker',
    );
    expect(workers).toEqual([
      {
        principal: 'integration-mail-task-worker',
        audience: 'user-core',
        token: placeholder,
        scopes: ['mail-task:resolve'],
      },
    ]);
    // Required (`:?`), so user-core can never start with a short or empty token
    // for this entry, and a single named variable carries it.
    expect(rawRegistry).toMatch(
      /"principal":"integration-mail-task-worker","audience":"user-core","token":"\$\{USER_CORE_MAIL_TASK_WORKER_TOKEN:\?[^}]+\}"/,
    );
    // No other credential may resolve mail-task aliases, and the gateway
    // principal user-core requires exactly once is untouched.
    expect(
      credentials
        .filter((credential) => credential.scopes.includes('mail-task:resolve'))
        .map((credential) => credential.principal),
    ).toEqual(['integration-mail-task-worker']);
    expect(
      credentials.filter(
        (credential) => credential.principal === 'verevon-gateway',
      ),
    ).toHaveLength(1);
  });
});

// Retention posture of service tokens (plan W0-2, the `tenant` posture).
//
// A principal's `retentionByAudience` row is one of three values:
//   zdr         every token is zero-retention                      (default)
//   persistent  every token is persistent
//   tenant      the token's `zdr` claim is the ORG's own ZDR setting, read from
//               org-core at mint time; tokens live at most 60 seconds
//
// What can go wrong is not in the row but in what the controllers do with it:
// resolve a posture for a principal that has none to resolve (a `persistent`
// worker starts following the org, or a `zdr` one starts blocking on org-core),
// or fail to resolve one that must (a `tenant` worker silently minting the safe
// default, which is wrong in the other direction: a non-ZDR org's background
// work stalls or, with the old `zdr: retention === 'zdr'`, a ZDR org's content
// is persisted). So these cases do not read the row and assume; they mint
// through the REAL token controllers, the REAL resolver and REAL RS256 signing
// with the REAL checked-in policy, and compare what came out with what the row
// says must come out. org-core is faked only in the way the resolver meets it:
// the request shape pinned by the retention-posture wire contract
// (src/auth/fixtures, vendored into org-core) with service authentication
// enforced.
const retentionVocabulary = ['persistent', 'tenant', 'zdr'];
// Plan W0-4 flips exactly these two lanes to `tenant`; any other lane doing so
// is a retention decision nobody has made.
const tenantLaneAllowList = [
  'embedding-engine|inference-core',
  'graph-index|inference-core',
];
// The golden is vendored inside this tree (src/auth/fixtures, sha256-pinned and
// checked against org-core's copy by interactive-retention-policy.org-core-
// contract.spec.ts), so reading it needs nothing outside the service.
const retentionGoldenPath = join(
  __dirname,
  'fixtures/auth-core-org-core-retention-posture.contract.json',
);
const orgCoreServiceToken = 'contract-spec-org-core-service-token-0123456789';

type RetentionPolicy = Record<string, PolicyEntry>;

type RetentionLane = {
  key: string;
  principal: string;
  audience: string;
  orgId: string;
  scopes: string[];
  retention: string;
  /**
   * How the organization this lane mints for is bound: `directory` rows are
   * checked against org-core at every mint, fixed lists and the legacy form are
   * not.
   */
  binding: 'fixed' | 'directory' | 'any';
};

/**
 * Every (principal, audience) pair of a policy, with what a mint would ask for:
 * a real organization (`org-a`, which org-core holds in the scenarios below) for
 * a directory-bound or legacy row, the row's first fixed entry otherwise.
 */
function retentionLanes(source: RetentionPolicy): RetentionLane[] {
  return Object.entries(source).flatMap(([principal, entry]) => {
    const binding: RetentionLane['binding'] =
      entry.tenancy === 'directory'
        ? 'directory'
        : entry.allowAnyOrg === true
          ? 'any'
          : 'fixed';
    return (entry.audiences ?? []).map((audience) => ({
      key: `${principal}|${audience}`,
      principal,
      audience,
      orgId: binding === 'fixed' ? (entry.orgIds ?? [])[0] : 'org-a',
      scopes: entry.scopesByAudience?.[audience] ?? entry.scopes ?? [],
      retention: entry.retentionByAudience?.[audience] ?? 'zdr',
      binding,
    }));
  });
}

/** Whether a mint on this lane asks org-core anything at all. */
function asksOrgCore(lane: RetentionLane): boolean {
  return lane.retention === 'tenant' || lane.binding === 'directory';
}

/** The policy as checked in, with the named lanes switched to `tenant`. */
function withTenantLanes(
  source: RetentionPolicy,
  lanes: readonly string[],
): RetentionPolicy {
  const flipped = JSON.parse(JSON.stringify(source)) as RetentionPolicy;
  for (const lane of lanes) {
    const [principal, audience] = lane.split('|');
    const entry = flipped[principal];
    if (!entry?.audiences?.includes(audience)) {
      throw new Error(`policy has no ${lane} lane to switch to tenant`);
    }
    entry.retentionByAudience = {
      ...(entry.retentionByAudience ?? {}),
      [audience]: 'tenant',
    };
  }
  return flipped;
}

function registryFrom(source: RetentionPolicy): string {
  return JSON.stringify(
    Object.fromEntries(
      Object.entries(source).map(([name, principal]) => [
        name,
        { ...principal, credential: `test-only-${name}-key` },
      ]),
    ),
  );
}

/**
 * What org-core says about the organization a mint is for: it holds it with ZDR
 * on (`org-zdr`), holds it with ZDR off (`org-plain`), has no record of it
 * (`org-unknown`, the 404 "organization not found"), or cannot be reached
 * (`org-core-down`).
 */
type OrgScenario = 'org-zdr' | 'org-plain' | 'org-core-down' | 'org-unknown';
const orgScenarios: readonly OrgScenario[] = [
  'org-zdr',
  'org-plain',
  'org-core-down',
  'org-unknown',
];

type Observation = {
  lane: string;
  scenario: OrgScenario;
  /**
   * `refused` is a 403 (the directory does not hold the organization),
   * `unavailable` a 503 (org-core could not be read): neither issues a token.
   */
  outcome: 'minted' | 'unavailable' | 'refused';
  /** The token's zdr claim; null when no token was issued. */
  zdr: boolean | null;
  /** How many times Auth Core asked org-core. */
  orgCoreCalls: number;
  /** null when no token was issued. */
  lifetime: 'capped' | 'default' | null;
  audited: boolean;
};

function expectedObservation(
  lane: RetentionLane,
  scenario: OrgScenario,
): Observation {
  const base = { lane: lane.key, scenario };
  const tenant = lane.retention === 'tenant';
  if (!asksOrgCore(lane)) {
    // A fixed list, or the legacy unchecked form on a zdr/persistent row: org-core
    // is never asked, so no org-core answer can change the outcome.
    return {
      ...base,
      outcome: 'minted',
      zdr: lane.retention === 'zdr',
      orgCoreCalls: 0,
      lifetime: 'default',
      audited: true,
    };
  }
  if (scenario === 'org-core-down') {
    return {
      ...base,
      outcome: 'unavailable',
      zdr: null,
      orgCoreCalls: 1,
      lifetime: null,
      audited: false,
    };
  }
  if (scenario === 'org-unknown' && lane.binding === 'directory') {
    // The directory does not hold the organization: no token, and the posture is
    // never read (one org-core round trip, the existence check itself).
    return {
      ...base,
      outcome: 'refused',
      zdr: null,
      orgCoreCalls: 1,
      lifetime: null,
      audited: false,
    };
  }
  // org-core answered, so the mint proceeds. Exactly one read serves both the
  // directory check and a tenant posture (they share the cache). A tenant lane
  // that is not directory-bound and meets an unknown organization keeps the
  // posture resolver's rule: no record means no ZDR intent, so zdr:false.
  return {
    ...base,
    outcome: 'minted',
    zdr: tenant ? scenario === 'org-zdr' : lane.retention === 'zdr',
    orgCoreCalls: 1,
    lifetime: tenant ? 'capped' : 'default',
    audited: true,
  };
}

function jwtPayload(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

/**
 * Mint one token per (lane, org scenario) through the real controllers and
 * report what each mint did.
 */
async function mintEveryLane(source: RetentionPolicy): Promise<Observation[]> {
  const originalEnv = { ...process.env };
  const orgCoreCalls: string[] = [];
  let scenario: OrgScenario = 'org-zdr';
  jest.spyOn(Logger.prototype, 'error').mockImplementation();
  jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  jest.spyOn(Logger.prototype, 'log').mockImplementation();
  jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      orgCoreCalls.push(url.pathname);
      if (scenario === 'org-core-down') {
        return Promise.reject(new TypeError('fetch failed'));
      }
      const authenticated =
        headers.get('x-service-id') === 'auth-core' &&
        headers.get('x-service-token') === orgCoreServiceToken &&
        /^\/internal\/orgs\/[^/]+\/retention-posture$/.test(url.pathname) &&
        (init?.method ?? 'GET').toUpperCase() === 'GET';
      const unknown = authenticated && scenario === 'org-unknown';
      const status = !authenticated ? 401 : unknown ? 404 : 200;
      return Promise.resolve({
        ok: status === 200,
        status,
        json: () =>
          Promise.resolve(
            !authenticated
              ? { error: 'unauthorized' }
              : unknown
                ? { error: 'organization not found' }
                : { zdrIntent: scenario === 'org-zdr', entitled: true },
          ),
      } as never);
    });

  try {
    process.env.NODE_ENV = 'test';
    for (const name of [
      'CONVEX_AUTH_PRIVATE_KEY_FILE',
      'CONVEX_AUTH_PUBLIC_KEY_FILE',
      'CONVEX_AUTH_PRIVATE_KEY_PEM',
      'CONVEX_AUTH_PUBLIC_KEY_PEM',
    ]) {
      delete process.env[name];
    }
    process.env.ORG_SERVICE_URL = 'http://org-core.contract-spec.test:8080';
    delete process.env.ORG_CORE_URL;
    process.env.ORG_CORE_SERVICE_TOKEN = orgCoreServiceToken;
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = registryFrom(source);

    const tokens = new ConvexTokenService();
    const published: unknown[] = [];
    const auditPublisher = {
      publishAuditDurable: jest.fn().mockImplementation((_subject, event) => {
        published.push(event);
        return Promise.resolve({
          stream: 'VEREVON_CONTROL_OBSERVABILITY',
          seq: 1,
        });
      }),
    };
    const planeController = new PlaneTokenController(
      tokens,
      auditPublisher as never,
    );
    const modelController = new ModelPlaneTokenController(
      tokens,
      auditPublisher as never,
    );
    const observations: Observation[] = [];

    for (const lane of retentionLanes(source)) {
      const isModelAudience = lane.audience === 'model-gateway';
      if (!isModelAudience && !tokens.isKnownPlaneAudience(lane.audience)) {
        throw new Error(
          `${lane.key}: the policy names an audience Auth Core cannot mint a token for`,
        );
      }
      // What an ordinary registry-sourced token for this audience lives:
      // anything shorter than this for a non-tenant lane would be a cap that
      // leaked beyond tenant principals.
      const defaultLifetime = (
        isModelAudience
          ? tokens.issueModelPlaneToken({
              userId: 'service:baseline',
              orgId: lane.orgId,
              principalType: 'service',
              serviceId: 'baseline',
              reason: 'baseline lifetime',
              retentionPosture: {
                zdr: true,
                authority: 'service-principal-policy',
              },
            })
          : tokens.issuePlaneToken(lane.audience as never, {
              userId: 'service:baseline',
              orgId: lane.orgId,
              principalType: 'service',
              serviceId: 'baseline',
              reason: 'baseline lifetime',
              retentionPosture: {
                zdr: true,
                authority: 'service-principal-policy',
              },
            })
      ).expiresInSeconds;

      for (scenario of orgScenarios) {
        resetInteractiveRetentionCacheForTests();
        orgCoreCalls.length = 0;
        published.length = 0;
        const request = {
          orgId: lane.orgId,
          scopes: lane.scopes,
          reason: 'retention posture contract mint',
        };
        let bundle: { token: string; expiresInSeconds: number } | undefined;
        let refused: unknown;
        try {
          bundle = isModelAudience
            ? ((await modelController.issueInternalToken(
                lane.principal,
                `test-only-${lane.principal}-key`,
                request,
              )) as never)
            : ((await planeController.issueInternalToken(
                lane.audience,
                lane.principal,
                `test-only-${lane.principal}-key`,
                request,
              )) as never);
        } catch (error) {
          refused = error;
        }
        let outcome: Observation['outcome'] = 'minted';
        if (refused !== undefined) {
          const response = (refused as { response?: { error?: string } })
            .response;
          if (
            response?.error === 'retention_posture_unavailable' ||
            response?.error === 'organization_directory_unavailable'
          ) {
            outcome = 'unavailable';
          } else if (refused instanceof ForbiddenException) {
            outcome = 'refused';
          } else {
            // Anything else (400, 404, a crash) means the real policy could not
            // mint this lane at all, which is its own defect.
            throw new Error(
              `${lane.key} (${scenario}) was refused: ${String(
                (refused as Error)?.message ?? refused,
              )}`,
            );
          }
        }
        observations.push({
          lane: lane.key,
          scenario,
          outcome,
          zdr: bundle ? (jwtPayload(bundle.token).zdr as boolean) : null,
          orgCoreCalls: orgCoreCalls.length,
          lifetime: bundle
            ? bundle.expiresInSeconds <= 60 && defaultLifetime > 60
              ? 'capped'
              : bundle.expiresInSeconds === defaultLifetime
                ? 'default'
                : (`odd:${bundle.expiresInSeconds}` as never)
            : null,
          audited: published.length > 0,
        });
      }
    }
    return observations;
  } finally {
    process.env = originalEnv;
    jest.restoreAllMocks();
  }
}

describeIfPolicy(
  'principal policy retention posture, minted through the real controllers',
  () => {
    it('uses only the retention vocabulary zdr, persistent, tenant, and only for audiences the principal can address', () => {
      for (const name of principalNames()) {
        const principal = policy?.[name];
        for (const [audience, posture] of Object.entries(
          principal?.retentionByAudience ?? {},
        )) {
          expect({
            name,
            audience,
            known: retentionVocabulary.includes(posture),
            addressable: principal?.audiences?.includes(audience) === true,
          }).toEqual({ name, audience, known: true, addressable: true });
        }
      }
    });

    it('keeps the quarry audience zero-retention for every principal: the run-level mode travels in the request body, never in a claim an org setting could change', () => {
      const quarryLanes = retentionLanes(policy ?? {}).filter(
        (lane) => lane.audience === 'quarry',
      );
      // Not vacuous. Who holds the Quarry web-tool scopes is pinned by the
      // grant block above; this one holds the retention of EVERY lane on the
      // audience, so a principal added to it later is held to it too.
      expect(quarryLanes.length).toBeGreaterThan(0);
      for (const lane of quarryLanes) {
        expect({ lane: lane.key, retention: lane.retention }).toEqual({
          lane: lane.key,
          retention: 'zdr',
        });
      }
    });

    it('assigns the tenant posture only to the background-worker lanes the plan names', () => {
      const tenantLanes = retentionLanes(policy ?? {})
        .filter((lane) => lane.retention === 'tenant')
        .map((lane) => lane.key);
      for (const lane of tenantLanes) {
        expect({ lane, allowed: tenantLaneAllowList.includes(lane) }).toEqual({
          lane,
          allowed: true,
        });
      }
    });

    it('mints, for every lane of the real policy, exactly what its rows say: org-core asked only for tenant and directory-bound lanes, a 403 for an unknown organization, a 503 and no token in an outage, caps only on tenant lanes', async () => {
      const lanes = retentionLanes(policy ?? {});
      // Every audience of every principal, so a lane cannot go untested by
      // being skipped; the floor only guards against an emptied policy.
      expect(lanes.length).toBe(
        Object.values(policy ?? {}).reduce(
          (total, principal) => total + (principal.audiences ?? []).length,
          0,
        ),
      );
      expect(lanes.length).toBeGreaterThan(20);

      const observed = await mintEveryLane(policy ?? {});

      expect(observed).toEqual(
        lanes.flatMap((lane) =>
          orgScenarios.map((scenario) => expectedObservation(lane, scenario)),
        ),
      );
    });

    it('does the same when the plan lanes are switched to tenant: they follow the org, fail closed with no token and no audit event during an org-core outage, and no other lane changes', async () => {
      // The two plan lanes are minted by the plane controller. The Model Plane
      // controller is a second, separate mint path with its own resolve call and
      // no real lane switched yet, so one model-gateway lane is switched here
      // too, in this derived copy only, to hold that path to the same rules.
      const switched = [
        ...tenantLaneAllowList,
        'verevon-gateway|model-gateway',
      ];
      const tenantPolicy = withTenantLanes(policy ?? {}, switched);
      const lanes = retentionLanes(tenantPolicy);
      expect(
        lanes
          .filter((lane) => lane.retention === 'tenant')
          .map((lane) => lane.key),
      ).toEqual(switched);

      const observed = await mintEveryLane(tenantPolicy);

      expect(observed).toEqual(
        lanes.flatMap((lane) =>
          orgScenarios.map((scenario) => expectedObservation(lane, scenario)),
        ),
      );
      const outage = observed.filter(
        (item) => item.scenario === 'org-core-down',
      );
      // An org-core outage blocks exactly the lanes that ask org-core (tenant
      // posture or directory binding), and no token or audit event leaks out of
      // any of them.
      const asking = lanes.filter(asksOrgCore).map((lane) => lane.key);
      expect(
        outage
          .filter((item) => item.outcome === 'unavailable')
          .map((item) => item.lane),
      ).toEqual(asking);
      for (const lane of switched) expect(asking).toContain(lane);
      // Every other lane (a fixed list) is untouched by the outage.
      expect(outage.filter((item) => item.outcome === 'minted').length).toBe(
        lanes.length - asking.length,
      );
      expect(
        outage.every(
          (item) =>
            (item.outcome === 'minted') === (item.orgCoreCalls === 0) &&
            (item.outcome !== 'minted') ===
              (item.zdr === null && !item.audited),
        ),
      ).toBe(true);
    });
  },
);

describeIfPolicy(
  'retention-posture wire contract the resolver is held to here',
  () => {
    it('is the request recorded in the golden: GET, the service headers and the org in the path', () => {
      expect(existsSync(retentionGoldenPath)).toBe(true);
      const golden = JSON.parse(readFileSync(retentionGoldenPath, 'utf8')) as {
        organizationId: string;
        tokenPlaceholder: string;
        requests: Array<{
          method: string;
          path: string;
          headers: Record<string, string>;
        }>;
      };
      expect(golden.requests).toEqual([
        {
          method: 'GET',
          path: `/internal/orgs/${golden.organizationId}/retention-posture`,
          headers: {
            accept: 'application/json',
            'x-service-id': 'auth-core',
            'x-service-token': golden.tokenPlaceholder,
          },
        },
      ]);
    });
  },
);

// Production tenancy (plan W0-3). The production loader refuses the unchecked
// `allowAnyOrg` form, and the committed policy used to be nothing but that form,
// so the real principal set could not load in production at all. These cases run
// the REAL production loader (NODE_ENV=production, a private registry file, the
// same code Auth Core runs) over the REAL policy with credentials added, and then
// authorize every lane of what it loaded, so "production-valid" is a tested
// property of the file rather than a claim about it.
//
// What the policy may say about organizations:
//   tenancy: 'directory'   any organization org-core holds, verified at mint
//   orgIds: [...]          a fixed list, with no lookup. Only the pseudo-
//                          organization `global` (which no directory holds) and
//                          the dev-eval corpus organizations belong here.
// Nothing else, and never `allowAnyOrg: true`.
const fixedOrgAllowList: Record<string, readonly string[]> = {
  'corpus-seeder': ['org-corpus-baseline', 'org-corpus-contextual'],
  'execution-core': ['global'],
};

describeIfPolicy(
  'principal policy production tenancy, over the real file',
  () => {
    let directory: string;
    let registryFile: string;

    beforeEach(() => {
      directory = mkdtempSync(join(tmpdir(), 'plane-principals-production-'));
      registryFile = join(directory, 'registry.json');
    });

    afterEach(() => {
      rmSync(directory, { recursive: true, force: true });
    });

    const loadInProduction = (raw: string, runtime = 'production') => {
      writeFileSync(registryFile, raw, { mode: 0o600 });
      return loadPlaneServicePrincipalRegistry({
        NODE_ENV: runtime,
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
      });
    };

    it('has no allowAnyOrg: true on any principal', () => {
      const unchecked = principalNames().filter(
        (name) => policy?.[name]?.allowAnyOrg === true,
      );
      expect(unchecked).toEqual([]);
    });

    it('binds every principal by exactly the sources the production model allows, and fixes only the organizations the allow-list names', () => {
      for (const name of principalNames()) {
        const row = policy?.[name] as PolicyEntry;
        const fixed = row.orgIds ?? [];
        const directoryBound = row.tenancy === 'directory';
        expect({
          name,
          bound: directoryBound || fixed.length > 0,
          tenancyOk: row.tenancy === undefined || directoryBound,
          unexpectedFixed: fixed.filter(
            (orgId) => !(fixedOrgAllowList[name] ?? []).includes(orgId),
          ),
        }).toEqual({
          name,
          bound: true,
          tenancyOk: true,
          unexpectedFixed: [],
        });
      }
      // Not vacuous: the multi-tenant workers are directory-bound, not listed.
      for (const name of [
        'embedding-engine',
        'graph-index',
        'quarry-edge',
        'retrieval-engine',
        'session-core',
      ]) {
        expect(policy?.[name]?.tenancy).toBe('directory');
      }
    });

    it('loads in production as it stands, and Auth Core can then authorize every lane of what it loaded', () => {
      const loaded = loadInProduction(registryWithTestCredentials());
      expect(Object.keys(JSON.parse(loaded) as object).sort()).toEqual(
        principalNames().sort(),
      );

      let lanes = 0;
      for (const lane of retentionLanes(policy ?? {})) {
        const authorized = authorizePlaneServicePrincipal(loaded, {
          serviceId: lane.principal,
          credential: `test-only-${lane.principal}-key`,
          audience: lane.audience,
          orgId: lane.orgId,
          requestedScopes: lane.scopes,
          reason: 'production tenancy contract',
        });
        expect(authorized).toMatchObject({
          serviceId: lane.principal,
          orgId: lane.orgId,
          orgBinding: lane.binding === 'any' ? 'directory' : lane.binding,
        });
        lanes += 1;
      }
      expect(lanes).toBeGreaterThan(20);
    });

    it('loads in every non-development runtime a deployment may name', () => {
      for (const runtime of ['production', 'staging', 'prod', 'qa']) {
        expect(() =>
          loadInProduction(registryWithTestCredentials(), runtime),
        ).not.toThrow();
      }
    });

    it('would refuse the same policy in the development shape (directory rows read as allowAnyOrg), so the check above is not vacuous', () => {
      const developmentShape = JSON.stringify(
        Object.fromEntries(
          principalNames().map((name) => {
            const { tenancy, ...rest } = policy?.[name] as PolicyEntry;
            return [
              name,
              {
                ...rest,
                ...(tenancy === 'directory' ? { allowAnyOrg: true } : {}),
                credential: `test-only-${name}-key`,
              },
            ];
          }),
        ),
      );
      expect(() => loadInProduction(developmentShape)).toThrow(
        /fixed organization allowlists/,
      );
      // ...and it is exactly what development still accepts.
      expect(() =>
        loadInProduction(developmentShape, 'development'),
      ).not.toThrow();
    });

    it('lets production mint for the pseudo-organization global without asking anyone, and for a real organization only through the directory', () => {
      const registry = loadInProduction(registryWithTestCredentials());
      const global = authorizePlaneServicePrincipal(
        registry,
        mintRequest(
          'execution-core',
          'capability-core',
          'capability:read',
          'global',
        ),
      );
      expect(global).toMatchObject({ orgId: 'global', orgBinding: 'fixed' });
      const run = authorizePlaneServicePrincipal(
        registry,
        mintRequest(
          'execution-core',
          'capability-core',
          'capability:read',
          'org-a',
        ),
      );
      expect(run).toMatchObject({ orgId: 'org-a', orgBinding: 'directory' });
      // A principal whose list is fixed has no way to reach another organization.
      expect(() =>
        authorizePlaneServicePrincipal(
          registry,
          mintRequest('corpus-seeder', 'data-plane', 'data:read', 'org-a'),
        ),
      ).toThrow(/not authorized/i);
    });
  },
);

// The Control Plane principal bundle (plan W0-4, "P1"): the grants the Data
// Plane work needs, in one change set, each pinned so that no other principal
// can hold it and no other lane can carry it.
//
//   data:rerank           quarry-edge, on data-plane, alone. Quarry's web rerank:
//                         POST /v1/rerank on the Data Plane retrieval engine
//                         (paid Azure Cohere; never the Model Plane, never a
//                         subscription route).
//   cost:write            data-orchestrator, on cost-core, alone. The Data Plane's
//                         usage forwarder: POST /api/v1/cost/record, which
//                         cost-core pins to the token's own organization.
//   retention:zdr-decide  verevon-gateway-retention-decision, on control-policy,
//                         alone. What the BFF presents to the later Auth Core
//                         waiver decision endpoint. DARK: nothing presents it and
//                         nothing verifies it yet.
//   tenant retention      embedding-engine and graph-index, on inference-core.
//
// Everything below runs the REAL checked-in policy through the REAL authorizer,
// and for the cases that cross a controller boundary through the real token
// controllers and the real RS256 signer; org-core is faked only in the way the
// directory check meets it (service authentication enforced, no answer without a
// credential).
const rerankScope = 'data:rerank';
const costWriteScope = 'cost:write';
const retentionDecideScope = 'retention:zdr-decide';
const costWriter = 'data-orchestrator';
const retentionDecider = 'verevon-gateway-retention-decision';
const bundleScopes = [rerankScope, costWriteScope, retentionDecideScope];
// Every audience a principal could be asked to mint for, whether or not any row
// names it: a refusal matrix over only the audiences the policy lists would miss
// a grant that lands on one it does not.
const everyPlaneAudience = [
  'application-plane',
  'bridge-core',
  'browser-broker',
  'capability-core',
  'control-plane',
  'control-policy',
  'cost-core',
  'data-plane',
  'execution-core',
  'inference-core',
  'ingestion',
  'letta-bridge',
  'model-gateway',
  'orchestrator-core',
  'quarry',
  'sandbox-manager',
  'session-core',
];

function everyAudience(): string[] {
  const audiences = new Set<string>(everyPlaneAudience);
  for (const name of principalNames()) {
    for (const audience of policy?.[name]?.audiences ?? []) {
      audiences.add(audience);
    }
  }
  return [...audiences].sort();
}

/** The `principal|audience` lanes whose ceiling names the scope. */
function lanesHoldingScope(scope: string): string[] {
  return retentionLanes(policy ?? {})
    .filter((lane) => lane.scopes.includes(scope))
    .map((lane) => lane.key)
    .sort();
}

/**
 * Asks the real authorizer for `scope` alone from EVERY principal on EVERY
 * audience: the `allowed` lanes must be authorized and every other pair refused,
 * so a grant that spread to a second principal or a second audience fails here by
 * name, and the refused count shows the whole matrix ran.
 */
function expectScopeConfinedTo(
  scope: string,
  allowed: readonly string[],
): void {
  const registry = registryWithTestCredentials();
  const audiences = everyAudience();
  let refused = 0;
  for (const name of principalNames()) {
    for (const audience of audiences) {
      const attempt = () =>
        authorizePlaneServicePrincipal(
          registry,
          mintRequest(name, audience, scope),
        );
      if (allowed.includes(`${name}|${audience}`)) {
        expect(attempt).not.toThrow();
      } else {
        expect(attempt).toThrow(/not authorized/i);
        refused += 1;
      }
    }
  }
  expect(refused).toBe(
    principalNames().length * audiences.length - allowed.length,
  );
}

type RealMint = {
  token: string;
  expiresInSeconds: number;
  claims: Record<string, unknown>;
};

/**
 * One mint through the REAL plane token controller: the real rows of `source`,
 * the real directory check (org-core holds `org-a`, ZDR off, and answers only a
 * caller that presents Auth Core's service credential) and real RS256 signing.
 * `inspect` runs while that environment is still in place, with the signing
 * service, so a minted token can be handed to a verifier built on the same key.
 */
async function mintThroughRealController<T>(
  source: RetentionPolicy,
  request: { principal: string; audience: string; scopes: string[] },
  inspect: (minted: RealMint, tokens: ConvexTokenService) => Promise<T> | T,
): Promise<T> {
  const originalEnv = { ...process.env };
  jest.spyOn(Logger.prototype, 'error').mockImplementation();
  jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  jest.spyOn(Logger.prototype, 'log').mockImplementation();
  jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      const authenticated =
        headers.get('x-service-id') === 'auth-core' &&
        headers.get('x-service-token') === orgCoreServiceToken &&
        url.pathname === '/internal/orgs/org-a/retention-posture';
      return Promise.resolve({
        ok: authenticated,
        status: authenticated ? 200 : 401,
        json: () =>
          Promise.resolve(
            authenticated
              ? { zdrIntent: false, entitled: true }
              : { error: 'unauthorized' },
          ),
      } as never);
    });
  try {
    process.env.NODE_ENV = 'test';
    for (const name of [
      'CONVEX_AUTH_PRIVATE_KEY_FILE',
      'CONVEX_AUTH_PUBLIC_KEY_FILE',
      'CONVEX_AUTH_PRIVATE_KEY_PEM',
      'CONVEX_AUTH_PUBLIC_KEY_PEM',
      'PLANE_SERVICE_PRINCIPALS_FILE',
      'DATA_PLANE_POLICY_CALLER_SERVICE_ID',
      'DATA_PLANE_POLICY_AUTH_AUDIENCE',
    ]) {
      delete process.env[name];
    }
    process.env.ORG_SERVICE_URL = 'http://org-core.contract-spec.test:8080';
    delete process.env.ORG_CORE_URL;
    process.env.ORG_CORE_SERVICE_TOKEN = orgCoreServiceToken;
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = registryFrom(source);
    resetInteractiveRetentionCacheForTests();

    const tokens = new ConvexTokenService();
    const controller = new PlaneTokenController(tokens, {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    } as never);
    const bundle = (await controller.issueInternalToken(
      request.audience,
      request.principal,
      `test-only-${request.principal}-key`,
      {
        orgId: 'org-a',
        scopes: request.scopes,
        reason: 'principal bundle contract mint',
      },
    )) as { token: string; expiresInSeconds: number };
    return await inspect(
      { ...bundle, claims: jwtPayload(bundle.token) },
      tokens,
    );
  } finally {
    process.env = originalEnv;
    resetInteractiveRetentionCacheForTests();
    jest.restoreAllMocks();
  }
}

/**
 * Presents `token` to the REAL Data Plane authorization decision endpoint with an
 * empty body: a caller the endpoint refuses is a 403 at authentication, one it
 * admits is then refused as a bad request (the body names no user), so the two
 * outcomes tell the door's answer from the harness's.
 */
function callDataPlaneDecision(
  tokens: ConvexTokenService,
  token: string,
): Promise<unknown> {
  return (
    Reflect.construct(DataPlaneAuthorizationController, [tokens]) as {
      decide(
        authorization: string | undefined,
        body: Record<string, string>,
      ): Promise<unknown>;
    }
  ).decide(`Bearer ${token}`, {});
}

describeIfPolicy('principal bundle P1: data:rerank for quarry-edge', () => {
  it('is held by quarry-edge alone, on data-plane alone, beside the scopes it already held and nothing else', () => {
    expect(holdersOfScope(rerankScope)).toEqual(['quarry-edge']);
    expect(lanesHoldingScope(rerankScope)).toEqual(['quarry-edge|data-plane']);
    const row = policy?.['quarry-edge'];
    const ceiling = [
      'data:read',
      'data:rerank',
      'documents:write',
      'org:data:read_all',
    ];
    expect(row?.scopesByAudience?.['data-plane']).toEqual(ceiling);
    expect(row?.scopes).toEqual(ceiling);
    // The grant adds no retention authority, no tenancy and no delegation.
    expect(row?.audiences).toContain('data-plane');
    expect(row?.retentionByAudience?.['data-plane']).toBe('persistent');
    expect(row?.tenancy).toBe('directory');
    expect(row?.allowAnyOrg).toBeUndefined();
    expect(row?.consumerEnv).toBe('QUARRY_SERVICE_API_KEY');
    expect(row?.tokenExchange).toBeUndefined();
  });

  it('mints data:rerank for quarry-edge on data-plane, as a persistent service token for a directory organization', () => {
    const registry = registryWithTestCredentials();
    expect(
      authorizePlaneServicePrincipal(
        registry,
        mintRequest('quarry-edge', 'data-plane', rerankScope),
      ),
    ).toMatchObject({
      serviceId: 'quarry-edge',
      subject: 'service:quarry-edge',
      orgId: 'org-a',
      orgBinding: 'directory',
      scopes: [rerankScope],
      retention: 'persistent',
      zdr: false,
    });
  });

  it('refuses data:rerank to every other principal on every audience, and to quarry-edge on every audience but data-plane', () => {
    expectScopeConfinedTo(rerankScope, ['quarry-edge|data-plane']);
  });

  it('refuses data:rerank mixed with a scope the lane does not hold, so the grant cannot ride along with another authority', () => {
    const registry = registryWithTestCredentials();
    for (const other of [
      costWriteScope,
      retentionDecideScope,
      'models:invoke',
      'inference:invoke',
      'integration:write',
      'data:admin',
    ]) {
      expect(() =>
        authorizePlaneServicePrincipal(registry, {
          ...mintRequest('quarry-edge', 'data-plane', rerankScope),
          requestedScopes: [rerankScope, other],
        }),
      ).toThrow(/not authorized/i);
    }
  });

  it('mints a real data-plane token carrying exactly data:rerank for service:quarry-edge, with no user and no ZDR', async () => {
    await mintThroughRealController(
      policy ?? {},
      {
        principal: 'quarry-edge',
        audience: 'data-plane',
        scopes: [rerankScope],
      },
      ({ claims }) => {
        expect(claims).toMatchObject({
          aud: 'data-plane',
          sub: 'service:quarry-edge',
          principal_type: 'service',
          service_id: 'service:quarry-edge',
          org_id: 'org-a',
          scopes: [rerankScope],
          zdr: false,
        });
        expect(claims).not.toHaveProperty('user_id');
        expect(claims).not.toHaveProperty('acting_user_id');
      },
    );
  });
});

describeIfPolicy(
  'principal bundle P1: cost:write for data-orchestrator',
  () => {
    it('is held by data-orchestrator alone, on cost-core alone, as its only scope, with nothing else on the row', () => {
      expect(holdersOfScope(costWriteScope)).toEqual([costWriter]);
      expect(lanesHoldingScope(costWriteScope)).toEqual([
        `${costWriter}|cost-core`,
      ]);
      // Exact equality: any key added to the row (a second audience, a read scope,
      // delegation, an unchecked organization form) fails here.
      expect(policy?.[costWriter]).toEqual({
        tenancy: 'directory',
        audiences: ['cost-core'],
        consumerEnv: 'DATA_ORCHESTRATOR_COST_SERVICE_API_KEY',
        orgIds: [],
        retentionByAudience: { 'cost-core': 'persistent' },
        scopes: [costWriteScope],
        scopesByAudience: { 'cost-core': [costWriteScope] },
      });
    });

    it('mints cost:write on cost-core for a directory organization, and refuses it on inference-core and every other audience', () => {
      const registry = registryWithTestCredentials();
      expect(
        authorizePlaneServicePrincipal(
          registry,
          mintRequest(costWriter, 'cost-core', costWriteScope),
        ),
      ).toMatchObject({
        serviceId: costWriter,
        subject: `service:${costWriter}`,
        orgId: 'org-a',
        orgBinding: 'directory',
        scopes: [costWriteScope],
        retention: 'persistent',
        zdr: false,
      });
      expectScopeConfinedTo(costWriteScope, [`${costWriter}|cost-core`]);
    });

    it('refuses data-orchestrator every other scope on every audience, so it can write the ledger and read nothing', () => {
      const registry = registryWithTestCredentials();
      const audiences = everyAudience();
      const otherScopes = [
        'cost:read',
        'cost:org:read',
        'inference:invoke',
        'models:invoke',
        'data:read',
        'data:admin',
        'documents:write',
        'documents:read',
        'org:data:read_all',
        'session:write',
        'integration:write',
        rerankScope,
        retentionDecideScope,
      ];
      let refused = 0;
      for (const audience of audiences) {
        for (const scope of otherScopes) {
          expect(() =>
            authorizePlaneServicePrincipal(
              registry,
              mintRequest(costWriter, audience, scope),
            ),
          ).toThrow(/not authorized/i);
          refused += 1;
        }
      }
      expect(refused).toBe(audiences.length * otherScopes.length);
      // Not even cost:write riding with a second scope.
      expect(() =>
        authorizePlaneServicePrincipal(registry, {
          ...mintRequest(costWriter, 'cost-core', costWriteScope),
          requestedScopes: [costWriteScope, 'cost:read'],
        }),
      ).toThrow(/not authorized/i);
    });

    it('mints a real cost-core token carrying exactly cost:write for service:data-orchestrator, persistent, bound to the organization', async () => {
      await mintThroughRealController(
        policy ?? {},
        {
          principal: costWriter,
          audience: 'cost-core',
          scopes: [costWriteScope],
        },
        ({ claims }) => {
          expect(claims).toMatchObject({
            aud: 'cost-core',
            sub: `service:${costWriter}`,
            principal_type: 'service',
            service_id: `service:${costWriter}`,
            org_id: 'org-a',
            scopes: [costWriteScope],
            zdr: false,
          });
          expect(claims).not.toHaveProperty('user_id');
        },
      );
    });
  },
);

describeIfPolicy(
  'principal bundle P1: retention:zdr-decide for the BFF decision principal (dark)',
  () => {
    it('is held by the retention-decision principal alone, on control-policy alone, as its only scope, with nothing else on the row', () => {
      expect(holdersOfScope(retentionDecideScope)).toEqual([retentionDecider]);
      expect(lanesHoldingScope(retentionDecideScope)).toEqual([
        `${retentionDecider}|control-policy`,
      ]);
      expect(policy?.[retentionDecider]).toEqual({
        tenancy: 'directory',
        audiences: ['control-policy'],
        consumerEnv: 'GATEWAY_RETENTION_DECISION_SERVICE_API_KEY',
        orgIds: [],
        retentionByAudience: { 'control-policy': 'zdr' },
        scopes: [retentionDecideScope],
        scopesByAudience: { 'control-policy': [retentionDecideScope] },
      });
    });

    it('is a principal of its own: verevon-gateway, the gateway model credential, holds neither the scope nor the control-policy audience', () => {
      const gateway = policy?.['verevon-gateway'];
      expect(gateway?.audiences).toEqual(['model-gateway']);
      expect([...new Set(everyScopeOf('verevon-gateway'))]).toEqual([
        'mail-task:prepare-thread',
      ]);
      expect(gateway?.consumerEnv).toBe(
        'MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY',
      );
      // Two credentials, two variables: leaking one does not lend the other's
      // authority.
      expect(policy?.[retentionDecider]?.consumerEnv).not.toBe(
        gateway?.consumerEnv,
      );
    });

    it('shares the control-policy audience with retrieval-engine alone, and each holds its own decision scope alone', () => {
      const onControlPolicy = principalNames()
        .filter((name) =>
          (policy?.[name]?.audiences ?? []).includes('control-policy'),
        )
        .sort();
      expect(onControlPolicy).toEqual(['retrieval-engine', retentionDecider]);
      expect(holdersOfScope('data:authorization:decide')).toEqual([
        'retrieval-engine',
      ]);
      expect(
        policy?.['retrieval-engine']?.scopesByAudience?.['control-policy'],
      ).toEqual(['data:authorization:decide']);
    });

    it('mints retention:zdr-decide on control-policy for a directory organization as a zero-retention token, and refuses it on every other lane', () => {
      const registry = registryWithTestCredentials();
      expect(
        authorizePlaneServicePrincipal(
          registry,
          mintRequest(retentionDecider, 'control-policy', retentionDecideScope),
        ),
      ).toMatchObject({
        serviceId: retentionDecider,
        orgId: 'org-a',
        orgBinding: 'directory',
        scopes: [retentionDecideScope],
        retention: 'zdr',
        zdr: true,
      });
      expectScopeConfinedTo(retentionDecideScope, [
        `${retentionDecider}|control-policy`,
      ]);
    });

    it('refuses the decision principal the data-plane decision scope and every other scope on control-policy, so it cannot ask the membership question', () => {
      const registry = registryWithTestCredentials();
      for (const scope of [
        'data:authorization:decide',
        'data:read',
        'data:admin',
        'inference:invoke',
        'models:invoke',
        'mail-task:prepare-thread',
        'session:write',
        costWriteScope,
        rerankScope,
      ]) {
        expect(() =>
          authorizePlaneServicePrincipal(
            registry,
            mintRequest(retentionDecider, 'control-policy', scope),
          ),
        ).toThrow(/not authorized/i);
      }
      expect(() =>
        authorizePlaneServicePrincipal(registry, {
          ...mintRequest(
            retentionDecider,
            'control-policy',
            retentionDecideScope,
          ),
          requestedScopes: [retentionDecideScope, 'data:authorization:decide'],
        }),
      ).toThrow(/not authorized/i);
    });

    it('mints a real control-policy token for the decision principal that the data-plane decision endpoint refuses at the door, while the token of the retrieval engine gets past it', async () => {
      const decide = callDataPlaneDecision;

      await mintThroughRealController(
        policy ?? {},
        {
          principal: retentionDecider,
          audience: 'control-policy',
          scopes: [retentionDecideScope],
        },
        async ({ claims, token }, tokens) => {
          expect(claims).toMatchObject({
            aud: 'control-policy',
            sub: `service:${retentionDecider}`,
            principal_type: 'service',
            scopes: [retentionDecideScope],
            zdr: true,
          });
          // The membership oracle (is user U a member of org O?) answers the
          // retrieval engine and nobody else: a genuine control-policy token with
          // the wrong subject and scope is a 403, before any lookup.
          await expect(decide(tokens, token)).rejects.toBeInstanceOf(
            ForbiddenException,
          );
        },
      );
      await mintThroughRealController(
        policy ?? {},
        {
          principal: 'retrieval-engine',
          audience: 'control-policy',
          scopes: ['data:authorization:decide'],
        },
        async ({ token }, tokens) => {
          // The positive control: the same door lets the retrieval engine in (the
          // empty body is then refused as a bad request, which only happens after
          // authentication), so the refusal above is the principal's, not the
          // harness's.
          await expect(decide(tokens, token)).rejects.toBeInstanceOf(
            BadRequestException,
          );
        },
      );
    });

    it('keeps that door shut to the decision principal even if it were granted the data-plane decision scope too: the endpoint pins the caller subject as well as the scope', async () => {
      // A derived copy of the policy, never the checked-in one: the decision
      // principal given data:authorization:decide as well, the grant a careless edit
      // would make. Subject and scope are two locks on one door; this holds the
      // second one on its own.
      const widened = JSON.parse(
        JSON.stringify(policy ?? {}),
      ) as RetentionPolicy;
      const row = widened[retentionDecider];
      row.scopes = [...(row.scopes ?? []), 'data:authorization:decide'].sort();
      row.scopesByAudience = {
        'control-policy': [
          ...(row.scopesByAudience?.['control-policy'] ?? []),
          'data:authorization:decide',
        ].sort(),
      };

      await mintThroughRealController(
        widened,
        {
          principal: retentionDecider,
          audience: 'control-policy',
          scopes: ['data:authorization:decide'],
        },
        async ({ claims, token }, tokens) => {
          expect(claims).toMatchObject({
            sub: `service:${retentionDecider}`,
            scopes: ['data:authorization:decide'],
          });
          await expect(
            callDataPlaneDecision(tokens, token),
          ).rejects.toBeInstanceOf(ForbiddenException);
        },
      );
    });
  },
);

describeIfPolicy('principal bundle P1: tenant retention lanes', () => {
  it('puts exactly embedding-engine and graph-index on inference-core on the tenant posture, and the switch adds no authority', () => {
    const tenantLanes = retentionLanes(policy ?? {})
      .filter((lane) => lane.retention === 'tenant')
      .map((lane) => lane.key)
      .sort();
    expect(tenantLanes).toEqual([...tenantLaneAllowList].sort());
    for (const lane of tenantLaneAllowList) {
      const [principal, audience] = lane.split('|');
      const row = policy?.[principal];
      expect(row?.retentionByAudience).toEqual({ [audience]: 'tenant' });
      expect(row?.audiences).toEqual([audience]);
      expect(row?.scopes).toEqual(['inference:invoke']);
      expect(row?.scopesByAudience).toEqual({
        [audience]: ['inference:invoke'],
      });
      expect(row?.tenancy).toBe('directory');
    }
  });

  it('leaves every other inference-core lane exactly as it was: the retrieval engine, session-core, orchestrator-core and capability-core stay persistent', () => {
    const others = retentionLanes(policy ?? {}).filter(
      (lane) =>
        lane.audience === 'inference-core' &&
        !tenantLaneAllowList.includes(lane.key),
    );
    expect(others.map((lane) => lane.key).sort()).toEqual([
      'capability-core|inference-core',
      'orchestrator-core|inference-core',
      'retrieval-engine|inference-core',
      'session-core|inference-core',
    ]);
    for (const lane of others) {
      expect({ lane: lane.key, retention: lane.retention }).toEqual({
        lane: lane.key,
        retention: 'persistent',
      });
    }
  });
});

// Who holds the three new scopes, and how a scope is spelled: one place that
// every scope of the bundle passes, so a typo cannot create a second, accidental
// scope that nothing checks, and a wildcard cannot slip in as a "convenience".
describeIfPolicy('principal bundle P1: no wildcard, no stray scope', () => {
  it('spells the bundle scopes as single named scopes and carries no wildcard on any lane of any bundle principal', () => {
    for (const scope of bundleScopes) {
      expect({
        scope,
        shape: /^[a-z][a-z-]*:[a-z][a-z-]*$/.test(scope),
      }).toEqual({ scope, shape: true });
    }
    for (const name of [
      'quarry-edge',
      costWriter,
      retentionDecider,
      ...tenantLaneAllowList.map((lane) => lane.split('|')[0]),
    ]) {
      for (const scope of everyScopeOf(name)) {
        expect({
          name,
          scope,
          wildcard: /[*?]/.test(scope),
          padded: scope !== scope.trim(),
        }).toEqual({ name, scope, wildcard: false, padded: false });
      }
    }
  });

  it('keeps the three bundle scopes inside their lanes whatever else the policy grows to hold', () => {
    const lanes = retentionLanes(policy ?? {});
    const holders = (scope: string) =>
      lanes
        .filter((lane) => lane.scopes.includes(scope))
        .map((lane) => lane.key);
    expect(holders(rerankScope)).toEqual(['quarry-edge|data-plane']);
    expect(holders(costWriteScope)).toEqual([`${costWriter}|cost-core`]);
    expect(holders(retentionDecideScope)).toEqual([
      `${retentionDecider}|control-policy`,
    ]);
  });
});

// Quarry states, in one manifest, every service-token request quarry-edge makes
// and whether Auth Core's registry grants it (`granted`). The manifest is Quarry's
// (authored, not generated); a byte-for-byte copy is vendored here, pinned by the
// sha256 beside it, so this spec reads nothing outside the service's build
// context. Auth Core's half of the contract is what this block runs: the real
// authorizer must grant every row marked granted and refuse every row marked not,
// so the flag cannot say "granted" for a request the registry would refuse (the
// 403 on the web rerank, 2026-10-03) and cannot lag a grant it should record.
const manifestFixtureDirectory = join(__dirname, 'fixtures');
const manifestName = 'cross-plane-token-requests.json';
const manifestCanonicalPath = join(
  __dirname,
  '../../../../Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts',
  manifestName,
);

type ManifestRow = {
  name: string;
  audience: string;
  scopes: string[];
  reason: string;
  granted: boolean;
};

function sha256OfLf(bytes: Buffer): string {
  return createHash('sha256')
    .update(bytes.toString('utf8').replace(/\r\n/g, '\n'))
    .digest('hex');
}

describeIfPolicy(
  'principal policy against the token requests quarry-edge makes (Quarry manifest, vendored)',
  () => {
    const fixture = join(manifestFixtureDirectory, manifestName);
    const rows = (): ManifestRow[] =>
      (JSON.parse(readFileSync(fixture, 'utf8')) as { rows: ManifestRow[] })
        .rows;

    it('is the manifest Quarry owns: a vendored copy matching its pin and, in a full checkout, the original byte for byte', () => {
      expect(existsSync(fixture)).toBe(true);
      expect(existsSync(`${fixture}.sha256`)).toBe(true);
      const copy = readFileSync(fixture);
      const pinned = readFileSync(`${fixture}.sha256`, 'utf8')
        .trim()
        .split(/\s+/)[0];
      expect(sha256OfLf(copy)).toBe(pinned);
      if (!existsSync(manifestCanonicalPath)) {
        // The Quarry tree is outside this service's build context. A full-checkout
        // pipeline must not go green by not finding it.
        if (process.env.REQUIRE_CONTRACT || process.env.CI) {
          throw new Error(
            `REQUIRE_CONTRACT or CI is set but ${manifestCanonicalPath} is absent; run from a full checkout`,
          );
        }
        return;
      }
      expect(sha256OfLf(readFileSync(manifestCanonicalPath))).toBe(pinned);
      expect(readFileSync(manifestCanonicalPath, 'utf8')).toBe(
        copy.toString('utf8'),
      );
    });

    it('is a request set of quarry-edge, with more than the web rerank in it', () => {
      const names = rows().map((row) => row.name);
      expect(names).toEqual(
        expect.arrayContaining(['vector-retrieve', 'ingest', 'web-rerank']),
      );
      expect(new Set(names).size).toBe(names.length);
    });

    it('is authorized by the real policy exactly where a row says granted, and refused exactly where it does not', () => {
      const registry = registryWithTestCredentials();
      let granted = 0;
      let refused = 0;
      for (const row of rows()) {
        const attempt = () =>
          authorizePlaneServicePrincipal(registry, {
            serviceId: 'quarry-edge',
            credential: 'test-only-quarry-edge-key',
            audience: row.audience,
            orgId: 'org-a',
            requestedScopes: row.scopes,
            reason: row.reason,
          });
        if (row.granted) {
          expect({ row: row.name, outcome: attempt().scopes }).toEqual({
            row: row.name,
            outcome: row.scopes,
          });
          granted += 1;
        } else {
          expect({
            row: row.name,
            refused: (() => {
              try {
                attempt();
                return false;
              } catch {
                return true;
              }
            })(),
          }).toEqual({ row: row.name, refused: true });
          refused += 1;
        }
      }
      // Both halves ran: the manifest has granted rows and rows the registry does
      // not grant, so neither loop above was empty.
      expect(granted).toBeGreaterThanOrEqual(3);
      expect(refused).toBeGreaterThanOrEqual(1);
    });

    it('pins the web rerank row: data:rerank alone, on data-plane, never the Model Plane, and held by no other row', () => {
      const all = rows();
      const rerank = all.find((row) => row.name === 'web-rerank');
      expect(rerank).toMatchObject({
        audience: 'data-plane',
        scopes: [rerankScope],
        granted: true,
      });
      for (const row of all) {
        if (row.scopes.includes(rerankScope)) {
          expect({ row: row.name, scopes: row.scopes }).toEqual({
            row: 'web-rerank',
            scopes: [rerankScope],
          });
        }
        if (row.name === 'web-rerank') continue;
        // Rerank is a Data Plane request: no row that rides the Model Plane
        // audience is the web rerank under another name.
        if (row.audience === 'model-gateway') {
          expect(row.scopes).not.toContain(rerankScope);
        }
      }
    });
  },
);

// The release preflight (scripts/validate-release-credentials.sh) refuses a release
// whose credentials are missing, short, placeholders or shared. Each new
// principal's presenting-end credential belongs on that list, or a release could
// ship with the registry holding a credential the presenting service was never
// given. The shell test beside it (validate-release-credentials-test.sh) exercises
// the script itself; this pins the list against the policy it must cover.
const validatorPath = join(
  __dirname,
  '../../../scripts/validate-release-credentials.sh',
);

describeIfPolicy(
  'release credential preflight covers the consumerEnv of each new principal',
  () => {
    it('lists the credential variable of data-orchestrator and of the retention-decision principal, once each', () => {
      if (!existsSync(validatorPath)) {
        if (process.env.REQUIRE_CONTRACT || process.env.CI) {
          throw new Error(
            `REQUIRE_CONTRACT or CI is set but ${validatorPath} is absent; run from a full checkout`,
          );
        }
        return;
      }
      const block = /credential_names=\(([\s\S]*?)\n\)/.exec(
        readFileSync(validatorPath, 'utf8'),
      );
      expect(block).not.toBeNull();
      const names = (block?.[1] ?? '').split(/\s+/).filter(Boolean);
      expect(names.length).toBeGreaterThan(40);
      expect(new Set(names).size).toBe(names.length);
      for (const principal of [costWriter, retentionDecider]) {
        const variable = policy?.[principal]?.consumerEnv ?? '';
        expect({
          principal,
          variable,
          listed: names.includes(variable),
        }).toEqual({ principal, variable, listed: true });
      }
    });
  },
);
