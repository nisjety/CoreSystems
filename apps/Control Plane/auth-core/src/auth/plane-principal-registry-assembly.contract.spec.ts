import { spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  authorizePlaneServicePrincipal,
  loadPlaneServicePrincipalRegistry,
} from './plane-service-principal';

// The registry Auth Core loads is ASSEMBLED from the committed policy by one jq
// program (apps/Control Plane/scripts/plane-principal-registry.jq), which both the
// launcher and the central bootstrap run. scripts/tests/
// principal-registry-parity.test.sh proves those two writers use it and agree
// with the policy; this spec proves the OTHER half: that what the assembler emits
// is accepted, and acted on correctly, by Auth Core's REAL registry loader and
// REAL authorizer, in both profiles. A shape the loader rejects (an explicit null,
// an unknown binding) invalidates the whole registry on every request, which a
// shell comparison of two files cannot see.
//
//   production   the policy as stated. Loaded in production file mode, which
//                refuses allowAnyOrg, so this is the profile a deployment runs.
//   development  `tenancy: "directory"` rows as the legacy allowAnyOrg the
//                development stack has always run on. Loaded the way the dev
//                container loads it (raw JSON, NODE_ENV=development), and refused
//                in production file mode, so the production check is not vacuous.
//
// The policy, the assembler and jq sit outside this service's Docker build
// context, and they are the system under test, so they cannot be vendored: where
// any is absent the spec skips, EXCEPT under REQUIRE_CONTRACT, where the absence
// fails the suite. A contract that quietly skips is not a contract.
const policyPath = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);
const assemblerPath = join(
  __dirname,
  '../../../scripts/plane-principal-registry.jq',
);
const jqVersion = spawnSync('jq', ['--version'], { encoding: 'utf8' });
const jqAvailable = jqVersion.status === 0;
const available =
  existsSync(policyPath) && existsSync(assemblerPath) && jqAvailable;

if (!available && process.env.REQUIRE_CONTRACT) {
  throw new Error(
    `REQUIRE_CONTRACT is set but the principal policy (${policyPath}), the assembler (${assemblerPath}) or jq is missing; run from a full checkout with jq installed`,
  );
}
const describeIfAvailable = available ? describe : describe.skip;

type Row = {
  audiences?: string[];
  orgIds?: string[];
  allowAnyOrg?: boolean;
  tenancy?: string;
  scopes?: string[];
  scopesByAudience?: Record<string, string[]>;
  retentionByAudience?: Record<string, string>;
};

describeIfAvailable('assembled principal registry, loaded by Auth Core', () => {
  let directory: string;
  let policy: Record<string, Row>;

  beforeAll(() => {
    policy = JSON.parse(readFileSync(policyPath, 'utf8')) as Record<
      string,
      Row
    >;
  });

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'plane-principal-assembly-'));
  });

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true });
  });

  const credentialOf = (name: string) =>
    `test-only-${name}-credential-0123456789`;

  function assemble(profile: string): string {
    const credentials = JSON.stringify(
      Object.fromEntries(Object.keys(policy).map((n) => [n, credentialOf(n)])),
    );
    const run = spawnSync(
      'jq',
      [
        '-c',
        '-n',
        '--slurpfile',
        'pol',
        policyPath,
        '--argjson',
        'creds',
        credentials,
        '--arg',
        'profile',
        profile,
        '-f',
        assemblerPath,
      ],
      { encoding: 'utf8' },
    );
    if (run.status !== 0) {
      throw new Error(`assembler failed for ${profile}: ${run.stderr}`);
    }
    // jq on Windows ends its output with CRLF.
    return run.stdout.trim();
  }

  function loadForProduction(raw: string): string {
    const file = join(directory, 'registry.json');
    writeFileSync(file, raw, { mode: 0o600 });
    return loadPlaneServicePrincipalRegistry({
      NODE_ENV: 'production',
      PLANE_SERVICE_PRINCIPALS_FILE: file,
    });
  }

  /** Every (principal, audience) pair of the policy, with what a mint asks for. */
  function lanes(): Array<{
    name: string;
    audience: string;
    scopes: string[];
    orgId: string;
    fixedOnly: boolean;
  }> {
    return Object.entries(policy).flatMap(([name, row]) =>
      (row.audiences ?? []).map((audience) => {
        const fixedOnly =
          row.tenancy !== 'directory' && row.allowAnyOrg !== true;
        return {
          name,
          audience,
          scopes: row.scopesByAudience?.[audience] ?? row.scopes ?? [],
          orgId: fixedOnly ? (row.orgIds ?? [])[0] : 'org-a',
          fixedOnly,
        };
      }),
    );
  }

  function authorizeAll(
    raw: string,
    bindingOf: (lane: { fixedOnly: boolean }) => string,
  ): number {
    const all = lanes();
    for (const lane of all) {
      expect(
        authorizePlaneServicePrincipal(raw, {
          serviceId: lane.name,
          credential: credentialOf(lane.name),
          audience: lane.audience,
          orgId: lane.orgId,
          requestedScopes: lane.scopes,
          reason: 'registry assembly contract',
        }),
      ).toMatchObject({
        serviceId: lane.name,
        orgId: lane.orgId,
        orgBinding: bindingOf(lane),
      });
    }
    return all.length;
  }

  it('production profile: Auth Core loads it in production file mode, and authorizes every lane with the binding the policy states', () => {
    const loaded = loadForProduction(assemble('production'));
    expect(Object.keys(JSON.parse(loaded) as object).sort()).toEqual(
      Object.keys(policy).sort(),
    );
    expect(
      authorizeAll(loaded, (lane) => (lane.fixedOnly ? 'fixed' : 'directory')),
    ).toBeGreaterThan(20);
  });

  it('development profile: Auth Core loads it the way the development container does, and every directory row runs unchecked, as it always has', () => {
    const raw = assemble('development');
    expect(
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'development',
        PLANE_SERVICE_PRINCIPALS_JSON: raw,
      }),
    ).toBe(raw);
    expect(
      authorizeAll(raw, (lane) => (lane.fixedOnly ? 'fixed' : 'any')),
    ).toBeGreaterThan(20);
  });

  it('development profile: is exactly what production refuses, so the production check above is not vacuous', () => {
    expect(() => loadForProduction(assemble('development'))).toThrow(
      /fixed organization allowlists/,
    );
  });

  it('carries no explicit null in either profile (Auth Core would reject the whole registry)', () => {
    for (const profile of ['development', 'production']) {
      const nulls: string[] = [];
      JSON.parse(assemble(profile), (key: string, value: unknown): unknown => {
        if (value === null) nulls.push(key);
        return value;
      });
      expect({ profile, nulls }).toEqual({ profile, nulls: [] });
    }
  });

  it('refuses to assemble a production registry from a policy row that carries allowAnyOrg: true', () => {
    const unchecked = join(directory, 'unchecked-policy.json');
    writeFileSync(
      unchecked,
      JSON.stringify({
        ...policy,
        'quarry-edge': {
          ...policy['quarry-edge'],
          tenancy: undefined,
          allowAnyOrg: true,
        },
      }),
    );
    const run = spawnSync(
      'jq',
      [
        '-c',
        '-n',
        '--slurpfile',
        'pol',
        unchecked,
        '--argjson',
        'creds',
        JSON.stringify(
          Object.fromEntries(
            Object.keys(policy).map((n) => [n, credentialOf(n)]),
          ),
        ),
        '--arg',
        'profile',
        'production',
        '-f',
        assemblerPath,
      ],
      { encoding: 'utf8' },
    );
    expect(run.status).not.toBe(0);
    expect(run.stderr).toMatch(
      /allowAnyOrg is refused in the production profile/,
    );
  });

  // A policy row is edited by hand. The assembler must never turn a missing key
  // into an explicit null: Auth Core re-parses the whole registry on every
  // request and rejects the entire registry for one bad entry, so a null would
  // 503 every principal's mint fleet wide while /health stays green.
  function assembleFrom(
    rows: Record<string, unknown>,
    profile: string,
  ): ReturnType<typeof spawnSync> {
    const file = join(directory, 'edited-policy.json');
    writeFileSync(file, JSON.stringify(rows));
    return spawnSync(
      'jq',
      [
        '-c',
        '-n',
        '--slurpfile',
        'pol',
        file,
        '--argjson',
        'creds',
        JSON.stringify(
          Object.fromEntries(
            Object.keys(rows).map((n) => [n, credentialOf(n)]),
          ),
        ),
        '--arg',
        'profile',
        profile,
        '-f',
        assemblerPath,
      ],
      { encoding: 'utf8' },
    );
  }

  it.each(['development', 'production'])(
    '%s profile: a directory row that omits orgIds is emitted with an empty list, not null, and Auth Core accepts it',
    (profile) => {
      const run = assembleFrom(
        {
          'edited-worker': {
            tenancy: 'directory',
            audiences: ['inference-core'],
            scopes: ['inference:invoke'],
          },
        },
        profile,
      );
      expect(run.status).toBe(0);
      const raw = String(run.stdout).trim();
      expect(
        (JSON.parse(raw) as Record<string, { orgIds: unknown }>)[
          'edited-worker'
        ].orgIds,
      ).toEqual([]);
      expect(
        authorizePlaneServicePrincipal(raw, {
          serviceId: 'edited-worker',
          credential: credentialOf('edited-worker'),
          audience: 'inference-core',
          orgId: 'org-a',
          requestedScopes: ['inference:invoke'],
          reason: 'registry assembly contract',
        }),
      ).toMatchObject({ serviceId: 'edited-worker', orgId: 'org-a' });
    },
  );

  it.each([
    ['audiences', { tenancy: 'directory', scopes: ['inference:invoke'] }],
    ['scopes', { tenancy: 'directory', audiences: ['inference-core'] }],
    [
      'orgIds',
      {
        tenancy: 'directory',
        orgIds: 'org-a',
        audiences: ['inference-core'],
        scopes: ['inference:invoke'],
      },
    ],
  ])(
    'refuses to assemble a row whose %s is missing or malformed instead of emitting a registry Auth Core would reject whole',
    (key, row) => {
      for (const profile of ['development', 'production']) {
        const run = assembleFrom({ 'edited-worker': row }, profile);
        expect({ profile, key, status: run.status }).not.toEqual({
          profile,
          key,
          status: 0,
        });
        expect(String(run.stderr)).toContain(`"${key}" must be`);
      }
    },
  );

  // Auth Core checks every entry of the registry on EVERY request and refuses the
  // whole registry for one violation, so a hand edit that the assembler lets
  // through is a fleet-wide 503 with a green /health. These rows are the edits
  // that do it (the commonest: adding an audience to a row and forgetting its
  // entry in scopesByAudience or retentionByAudience). Each one must stop at the
  // assembler, in both profiles; the two valid rows beside them keep the table
  // honest (the assembler is not simply refusing everything) and are loaded by
  // the real loader.
  const entry = (override: Record<string, unknown>) => ({
    tenancy: 'directory',
    audiences: ['inference-core', 'session-core'],
    orgIds: ['global'],
    scopes: ['inference:invoke', 'session:heartbeat'],
    scopesByAudience: {
      'inference-core': ['inference:invoke'],
      'session-core': ['session:heartbeat'],
    },
    retentionByAudience: {
      'inference-core': 'persistent',
      'session-core': 'zdr',
    },
    tokenExchange: { 'session-core': ['inference-core'] },
    ...override,
  });

  it.each(['development', 'production'])(
    '%s profile: a well-formed row, with and without its per-audience maps, assembles and Auth Core loads it',
    (profile) => {
      for (const row of [
        entry({}),
        entry({
          scopesByAudience: undefined,
          retentionByAudience: undefined,
          tokenExchange: undefined,
        }),
      ]) {
        const run = assembleFrom({ 'edited-worker': row }, profile);
        expect(run.status).toBe(0);
        expect(() =>
          authorizePlaneServicePrincipal(String(run.stdout).trim(), {
            serviceId: 'edited-worker',
            credential: credentialOf('edited-worker'),
            audience: 'inference-core',
            orgId: 'org-a',
            requestedScopes: ['inference:invoke'],
            reason: 'registry assembly contract',
          }),
        ).not.toThrow();
      }
    },
  );

  it.each([
    [
      'an audience with no scopesByAudience entry',
      entry({ audiences: ['inference-core', 'session-core', 'letta-bridge'] }),
    ],
    [
      'an audience with no retentionByAudience entry',
      entry({
        retentionByAudience: { 'inference-core': 'persistent' },
      }),
    ],
    [
      'a scopesByAudience entry for something that is not an audience',
      entry({
        scopesByAudience: {
          'inference-core': ['inference:invoke'],
          'session-core': ['session:heartbeat'],
          'letta-bridge': ['memory:read'],
        },
        scopes: ['inference:invoke', 'session:heartbeat', 'memory:read'],
      }),
    ],
    [
      'a scope in scopes that no audience grants',
      entry({ scopes: ['inference:invoke', 'session:heartbeat', 'extra:one'] }),
    ],
    [
      'a granted scope that is not in scopes',
      entry({
        scopesByAudience: {
          'inference-core': ['inference:invoke', 'ghost:scope'],
          'session-core': ['session:heartbeat'],
        },
      }),
    ],
    [
      'a retention posture that is not zdr, persistent or tenant',
      entry({
        retentionByAudience: {
          'inference-core': 'maybe',
          'session-core': 'zdr',
        },
      }),
    ],
    [
      'an empty scopesByAudience entry',
      entry({
        scopesByAudience: {
          'inference-core': [],
          'session-core': ['session:heartbeat'],
        },
      }),
    ],
    [
      'a tokenExchange target that is not an audience',
      entry({ tokenExchange: { 'letta-bridge': ['inference-core'] } }),
    ],
    [
      'an empty tokenExchange entry',
      entry({ tokenExchange: { 'session-core': [] } }),
    ],
    [
      'a misspelled scopesByAudience, which Auth Core would read as every scope on every audience',
      entry({
        scopesByAudience: undefined,
        scopesByAudiance: {
          'inference-core': ['inference:invoke'],
          'session-core': ['session:heartbeat'],
        },
      }),
    ],
    [
      'the forbidden allowPersistentData key',
      entry({ allowPersistentData: true }),
    ],
    [
      'an allowAnyOrg that is not a boolean',
      entry({ tenancy: undefined, allowAnyOrg: 'maybe' }),
    ],
    [
      'an allowAnyOrg that is an explicit null',
      entry({ tenancy: undefined, allowAnyOrg: null }),
    ],
    ['a blank audience', entry({ audiences: ['inference-core', '  '] })],
    ['a non-string scope', entry({ scopes: ['inference:invoke', 7] })],
  ])(
    'refuses to assemble %s, instead of emitting a registry Auth Core rejects whole or reads more widely than the policy says',
    (_label, row) => {
      for (const profile of ['development', 'production']) {
        const run = assembleFrom({ 'edited-worker': row }, profile);
        expect({ profile, status: run.status }).not.toEqual({
          profile,
          status: 0,
        });
        expect(String(run.stderr)).toContain('plane-service-principals.json');
        expect(String(run.stderr)).toContain('edited-worker');
      }
    },
  );
});
