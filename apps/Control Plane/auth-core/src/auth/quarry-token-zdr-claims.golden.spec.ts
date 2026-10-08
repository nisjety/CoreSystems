import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Golden for the claims Quarry (quarry-edge) RECEIVES on `aud=quarry` tokens,
// minted by the real Auth Core code paths and not by hand:
//
//   * interactive user tokens: the real PlaneTokenController.getToken over the
//     real ConvexTokenService, with only the Better Auth session, the
//     membership lookup and the org-core posture read stubbed (they are I/O);
//   * service tokens: the real PlaneTokenController.issueInternalToken over the
//     real ConvexTokenService and the REAL checked-in principal policy
//     (apps/Control Plane/config/plane-service-principals.json), so the `zdr`
//     a service token carries is whatever the registry row says, not a value a
//     test author typed.
//
// Why it exists (Z-08, "Quarry takes the token zdr claim as a floor"): Quarry
// must lift its ZDR posture from the token claim for USER principals only. The
// service principals that call Quarry on behalf of a chat run (model-gateway,
// execution-core) hold `retentionByAudience.quarry = zdr`, so their token says
// `zdr:true` for EVERY org on EVERY call. A Quarry that read the claim for
// service tokens would silently force ZDR on all web search. The Rust contract
// test (quarry-edge tests/zdr_claim_floor.rs) re-signs these exact claim sets
// and drives the real router; this spec keeps them honest against the mint.
//
// Each consuming tree holds its own copy, so each side runs from its own Docker
// build context and reads nothing outside it:
//   - canonical copy: src/auth/fixtures/quarry-token-claims.golden.json (+ the
//     sibling .sha256 pin), here, written by this spec.
//   - vendored copy: Quarry-v2/crates/quarry-edge/tests/fixtures/auth-core/ (+
//     .sha256), inside Quarry's build context, where the Rust test pins it.
// Regenerate both with
//   UPDATE_GOLDENS=1 npx jest src/auth/quarry-token-zdr-claims.golden.spec.ts
// and commit the two .json files, the two .sha256 files and nothing else. The
// repo-level drift check (scripts/tests/zdr-contract-sync.test.sh) pins both.
//
// The principal policy and the Quarry tree sit outside this service's Docker
// build context. Wherever either is absent the spec skips, EXCEPT under
// REQUIRE_CONTRACT or CI, where the absence fails: a contract that quietly
// skips is not a contract.

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
const mockResolveInteractiveRetentionPosture = jest.fn();
// Only the INTERACTIVE posture is replaced. The service mints run the real
// organization-binding check against an org-core double that enforces service
// authentication, because the real policy rows are directory-bound.
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import { ConvexTokenService } from './convex-token.service';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';
import { PlaneTokenController } from './plane-token.controller';

const GOLDEN_NAME = 'quarry-token-claims.golden.json';
const SHA_NAME = 'quarry-token-claims.golden.sha256';
const goldenPath = join(__dirname, 'fixtures', GOLDEN_NAME);
const shaPath = join(__dirname, 'fixtures', SHA_NAME);
const quarryFixtures = join(
  __dirname,
  '../../../../Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/fixtures/auth-core',
);
const ORG_CORE_TOKEN = 'quarry-golden-org-core-token-0123456789abcdef';
const policyPath = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);

const requireContract = Boolean(process.env.REQUIRE_CONTRACT || process.env.CI);
const updating = process.env.UPDATE_GOLDENS === '1';
const available = existsSync(policyPath) && existsSync(goldenPath);
if (!available && requireContract) {
  throw new Error(
    'REQUIRE_CONTRACT/CI is set but the Quarry token golden or the principal policy is missing',
  );
}
const describeIfAvailable = available ? describe : describe.skip;

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

type Claims = Record<string, unknown>;

function payloadOf(token: string): Claims {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Claims;
}

/**
 * The time claims are the only per-mint values. The golden keeps the rule that
 * relates them (nbf = iat - 5, exp = iat + ttl) and drops the instants, so the
 * Rust test can stamp fresh ones and still sign a token the real validator
 * accepts.
 */
function withoutTimes(claims: Claims): {
  claims: Claims;
  ttlSeconds: number;
} {
  const { iat, nbf, exp, ...rest } = claims as Claims & {
    iat: number;
    nbf: number;
    exp: number;
  };
  expect(nbf).toBe(iat - 5);
  return { claims: rest, ttlSeconds: exp - iat };
}

describeIfAvailable('Quarry-audience token claims, real mint', () => {
  const originalEnv = { ...process.env };
  let controller: PlaneTokenController;
  let tokens: ConvexTokenService;

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    delete process.env.QUARRY_AUTH_AUDIENCE;
    delete process.env.PLANE_TOKEN_TTL_QUARRY_SECONDS;
    // The deployed issuer: the same default the compose files resolve for
    // PLANE_TOKEN_ISSUER / AUTH_CORE_ISSUER, so the golden pins what Quarry's
    // AUTH_CORE_ISSUER is compared against.
    process.env.PLANE_TOKEN_ISSUER = 'http://localhost:3011/api/convex-auth';
    const policy = JSON.parse(readFileSync(policyPath, 'utf8')) as Record<
      string,
      Record<string, unknown>
    >;
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify(
      Object.fromEntries(
        Object.entries(policy).map(([name, principal]) => [
          name,
          { ...principal, credential: `test-only-${name}-credential-key` },
        ]),
      ),
    );
    tokens = new ConvexTokenService();
    controller = new PlaneTokenController(tokens, {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    } as never);
    mockGetSession.mockReset();
    mockResolveCanonicalTokenContext.mockReset();
    mockResolveInteractiveRetentionPosture.mockReset();
    // org-core as the binding check meets it: service authentication on every
    // route, and a directory that holds every organization these cases use.
    process.env.ORG_SERVICE_URL = 'http://org-core.quarry-golden.test:8080';
    delete process.env.ORG_CORE_URL;
    process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_TOKEN;
    resetInteractiveRetentionCacheForTests();
    jest
      .spyOn(global, 'fetch')
      .mockImplementation((input: unknown, init?: RequestInit) => {
        const headers = new Headers(init?.headers);
        const authenticated =
          headers.get('x-service-id') === 'auth-core' &&
          headers.get('x-service-token') === ORG_CORE_TOKEN &&
          /^\/internal\/orgs\/[^/]+\/retention-posture$/.test(
            new URL(String(input)).pathname,
          );
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
  });

  afterEach(() => {
    jest.restoreAllMocks();
    resetInteractiveRetentionCacheForTests();
    process.env = { ...originalEnv };
  });

  async function userToken(orgId: string, zdr: boolean): Promise<string> {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-golden', email: 'member@example.test' },
      session: { activeOrganizationId: orgId },
    });
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId,
      role: 'member',
    });
    mockResolveInteractiveRetentionPosture.mockResolvedValue({
      zdr,
      authority: 'interactive-org-retention-policy',
    });
    const bundle = await controller.getToken('quarry', {
      headers: { cookie: 'session=golden' },
    } as never);
    return (bundle as { token: string }).token;
  }

  async function serviceToken(
    serviceId: string,
    scopes: string[],
    reason: string,
  ): Promise<string> {
    const bundle = await controller.issueInternalToken(
      'quarry',
      serviceId,
      `test-only-${serviceId}-credential-key`,
      { orgId: 'org-standard-golden', scopes, reason },
    );
    return (bundle as { token: string }).token;
  }

  async function regenerate(): Promise<string> {
    const cases = {
      user_in_zdr_org: withoutTimes(
        payloadOf(await userToken('org-zdr-golden', true)),
      ),
      user_in_standard_org: withoutTimes(
        payloadOf(await userToken('org-standard-golden', false)),
      ),
      service_model_gateway: withoutTimes(
        payloadOf(
          await serviceToken(
            'model-gateway',
            ['scrape:read', 'search:read'],
            'chat web search',
          ),
        ),
      ),
      service_execution_core: withoutTimes(
        payloadOf(
          await serviceToken(
            'execution-core',
            ['browser:execute', 'extract:read', 'scrape:write', 'search:read'],
            'agent web tools',
          ),
        ),
      ),
    };
    const document = {
      comment:
        'Claims of aud=quarry tokens as minted by Auth Core (real PlaneTokenController over the real ConvexTokenService and the checked-in principal policy). Time claims are stripped: the consumer stamps iat, nbf = iat - 5 and exp = iat + ttl_seconds. Regenerate with UPDATE_GOLDENS=1 from apps/Control Plane/auth-core/src/auth/quarry-token-zdr-claims.golden.spec.ts.',
      cases: Object.fromEntries(
        Object.entries(cases).map(([name, value]) => [
          name,
          { ttl_seconds: value.ttlSeconds, claims: value.claims },
        ]),
      ),
    };
    return `${JSON.stringify(document, null, 2)}\n`;
  }

  it('keeps the committed golden equal to what the real mint produces', async () => {
    const fresh = await regenerate();
    if (updating) {
      const pin = `${createHash('sha256').update(fresh).digest('hex')}  ${GOLDEN_NAME}\n`;
      writeFileSync(goldenPath, fresh);
      writeFileSync(shaPath, pin);
      // Vendor into Quarry's tree when it is present, so the two sides cannot be
      // regenerated apart.
      if (existsSync(join(quarryFixtures, '..', '..', '..'))) {
        mkdirSync(quarryFixtures, { recursive: true });
        writeFileSync(join(quarryFixtures, GOLDEN_NAME), fresh);
        writeFileSync(join(quarryFixtures, SHA_NAME), pin);
      }
    }
    expect(readFileSync(goldenPath, 'utf8')).toBe(fresh);
  });

  it("is byte-identical to Quarry's vendored copy, pin included", () => {
    if (!existsSync(quarryFixtures)) {
      if (requireContract) {
        throw new Error(
          `REQUIRE_CONTRACT/CI is set but Quarry's fixtures are absent: ${quarryFixtures}`,
        );
      }
      console.warn(
        `Quarry tree absent, skipping the vendored comparison: ${quarryFixtures}`,
      );
      return;
    }
    for (const name of [GOLDEN_NAME, SHA_NAME]) {
      const vendored = join(quarryFixtures, name);
      expect(existsSync(vendored)).toBe(true);
      expect(readFileSync(vendored, 'utf8')).toBe(
        readFileSync(join(__dirname, 'fixtures', name), 'utf8'),
      );
    }
  });

  it('pins the golden with the sha256 the Rust contract test checks', () => {
    const digest = createHash('sha256')
      .update(readFileSync(goldenPath))
      .digest('hex');
    expect(readFileSync(shaPath, 'utf8').trim().split(/\s+/)[0]).toBe(digest);
  });

  it('says what Quarry depends on: user posture follows the org, service posture follows the registry row', async () => {
    const zdrUser = payloadOf(await userToken('org-zdr-golden', true));
    const standardUser = payloadOf(
      await userToken('org-standard-golden', false),
    );
    expect(zdrUser).toMatchObject({
      aud: 'quarry',
      principal_type: 'user',
      user_id: 'user-golden',
      org_id: 'org-zdr-golden',
      zdr: true,
    });
    expect(standardUser).toMatchObject({
      principal_type: 'user',
      org_id: 'org-standard-golden',
      zdr: false,
    });

    // The trap Z-08 must not walk into: both chat-run callers carry zdr:true
    // regardless of the org's own posture, because the registry rows say so.
    const gateway = payloadOf(
      await serviceToken('model-gateway', ['search:read'], 'chat web search'),
    );
    const execution = payloadOf(
      await serviceToken('execution-core', ['search:read'], 'agent web tools'),
    );
    for (const claims of [gateway, execution]) {
      expect(claims).toMatchObject({
        aud: 'quarry',
        principal_type: 'service',
        org_id: 'org-standard-golden',
        zdr: true,
      });
      expect(claims.user_id).toBeUndefined();
    }
    expect(gateway.service_id).toBe('service:model-gateway');
    expect(execution.service_id).toBe('service:execution-core');
  });
});
