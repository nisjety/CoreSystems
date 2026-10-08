import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
const mockResolveInteractiveRetentionPosture = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
// Only the INTERACTIVE posture is replaced (the interactive tokens here carry a
// fixed posture on purpose). The service mint runs the real organization-binding
// check against an org-core double that enforces service authentication, because
// the real policy row for quarry-edge is directory-bound.
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import { db } from '../db';
import { ConvexTokenService } from './convex-token.service';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';
import {
  buildDataPlaneDecision,
  DataPlaneAuthorizationController,
  type DataPlaneDecision,
  type MembershipRecord,
} from './data-plane-authorization.controller';
import { PlaneTokenController } from './plane-token.controller';

/**
 * Cross-language contract: what Auth Core REALLY emits to the Data Plane for a
 * service caller, and what it REALLY answers when the Data Plane asks whether
 * that service caller is an organization member. The Data Plane retrieval
 * engine (services/retrieval-engine-rs, tests/service_principal_auth.rs) replays
 * both goldens through its real JWT validation, its real route table and its
 * real HttpPolicyClient.
 *
 * Why it exists (live finding, 2026-10-01): quarry-edge's retrieve calls all
 * answered 403. Auth Core signed a perfectly valid service token
 * (sub=service:quarry-edge, principal_type=service, no user_id) and the engine
 * pushed that subject through the USER membership lookup, which has no row for
 * a service subject: `data_plane_authorization_decision ... allowed=false
 * decisionReason=not_member`. The engine's own tests used a permissive
 * membership double, so nothing could have noticed.
 *
 * The claims golden is written by Auth Core's real mint path, not typed by
 * hand: PlaneTokenController.issueInternalToken (real registry row for
 * quarry-edge from config/plane-service-principals.json, real
 * authorizePlaneServicePrincipal, real ConvexTokenService.issuePlaneToken) for
 * the service token and PlaneTokenController.getToken for the interactive
 * tokens. The decision golden is written by the real
 * DataPlaneAuthorizationController.decide, driven by a real control-policy
 * token and an empty `member` table (the database is the only double: Better
 * Auth keys `member` rows by user id, so no row exists for `service:*`).
 *
 * Time claims are frozen so the goldens are reproducible; the Rust side
 * re-bases iat/nbf/exp on its own clock and keeps the offsets.
 *
 * Regenerate after an intentional change (also vendors the copies and pins):
 *   UPDATE_CONTRACT_GOLDEN=1 npx jest data-plane-service-principal.contract
 *
 * Files and pins:
 *   - canonical copy: src/auth/fixtures/<name>.golden.json (+ .sha256 pin),
 *     inside this service's Docker build context.
 *   - vendored copy: Data Plane v2/services/retrieval-engine-rs/tests/fixtures/
 *     auth-core/<name>.golden.json (+ .sha256 pin), inside the retrieval
 *     engine's build context. Both sides pin the sha256 of the file with CRLF
 *     normalised to LF, and this spec fails when the copies differ.
 *
 * The policy file and the Data Plane tree sit outside this service's Docker
 * build context. Without REQUIRE_CONTRACT (or CI) a missing one skips the part
 * that needs it; with either set a missing file FAILS, so a pipeline cannot go
 * green by not finding the contract.
 */
const POLICY_PATH = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);
const AUTH_FIXTURES = join(__dirname, 'fixtures');
const DATA_PLANE_FIXTURES = join(
  __dirname,
  '../../../../Data Plane v2/services/retrieval-engine-rs/tests/fixtures/auth-core',
);
const CLAIMS_GOLDEN = 'data-plane-service-token-claims.golden.json';
const DECISION_GOLDEN = 'data-plane-authorization-decision.golden.json';

const ISSUER = 'https://control.contract.test/api/convex-auth';
const ORG_ID = 'org-contract-fixture';
const FROZEN_NOW_SECONDS = Date.UTC(2026, 9, 1) / 1000;
// The exact reason and scope quarry-edge's DataPlaneVectorIndex requests
// (Quarry-v2 quarry-runtime/src/vector_index.rs: ServiceTokenRequest::data_plane).
const QUARRY_RETRIEVAL_SCOPES = ['data:read'];
const QUARRY_RETRIEVAL_REASON = 'retrieve verified organization evidence';
const ORG_CORE_FIXTURE_TOKEN = 'data-plane-contract-org-core-token-0123456789';
const MEMBER_USER_ID = 'user-contract-member';
const ADMIN_USER_ID = 'user-contract-admin';
const MEMBER_CREATED_AT = new Date('2026-01-01T00:00:00.000Z');

const requireContract = Boolean(process.env.REQUIRE_CONTRACT || process.env.CI);
const updateGolden = process.env.UPDATE_CONTRACT_GOLDEN === '1';

type PolicyRow = Record<string, unknown>;
const policy: Record<string, PolicyRow> | null = existsSync(POLICY_PATH)
  ? (JSON.parse(readFileSync(POLICY_PATH, 'utf8')) as Record<string, PolicyRow>)
  : null;

type TokenGolden = {
  request: Record<string, unknown>;
  header: Record<string, unknown>;
  claims: Record<string, unknown>;
  claimKeys: string[];
};
type ClaimsGolden = {
  contract: string;
  version: number;
  audience: string;
  issuer: string;
  orgId: string;
  frozenNowSeconds: number;
  tokens: Record<string, TokenGolden>;
};
type DecisionGolden = {
  contract: string;
  version: number;
  orgId: string;
  decisions: Record<
    string,
    {
      request: { userId: string; orgId: string; action: string };
      response: DataPlaneDecision;
    }
  >;
};

function sha256Lf(bytes: Buffer): string {
  return createHash('sha256')
    .update(bytes.toString('utf8').replace(/\r\n/g, '\n'))
    .digest('hex');
}

function pinLine(name: string, bytes: Buffer): string {
  return `${sha256Lf(bytes)}  ${name}\n`;
}

function pinnedHash(path: string): string {
  return readFileSync(path, 'utf8').trim().split(/\s+/)[0];
}

function decodeJwt(token: string): {
  header: Record<string, unknown>;
  claims: Record<string, unknown>;
} {
  const [header, payload] = token.split('.');
  return {
    header: JSON.parse(
      Buffer.from(header, 'base64url').toString('utf8'),
    ) as Record<string, unknown>,
    claims: JSON.parse(
      Buffer.from(payload, 'base64url').toString('utf8'),
    ) as Record<string, unknown>,
  };
}

function tokenGolden(
  request: Record<string, unknown>,
  token: string,
): TokenGolden {
  const { header, claims } = decodeJwt(token);
  return { request, header, claims, claimKeys: Object.keys(claims).sort() };
}

function registryWithTestCredentials(): string {
  return JSON.stringify(
    Object.fromEntries(
      Object.entries(policy ?? {}).map(([name, row]) => [
        name,
        { ...row, credential: `test-only-${name}-key` },
      ]),
    ),
  );
}

/** Runs `body` with the environment the real mint reads, restored afterwards. */
async function withContractEnvironment<T>(body: () => Promise<T>): Promise<T> {
  const saved = { ...process.env };
  for (const name of [
    'CONVEX_AUTH_PRIVATE_KEY_FILE',
    'CONVEX_AUTH_PRIVATE_KEY_PEM',
    'CONVEX_AUTH_PUBLIC_KEY_FILE',
    'CONVEX_AUTH_PUBLIC_KEY_PEM',
    'DATA_PLANE_AUTH_AUDIENCE',
    'PLANE_TOKEN_TTL_DATA_PLANE_SECONDS',
    'PLANE_SERVICE_PRINCIPALS_FILE',
    'DATA_PLANE_POLICY_AUTH_AUDIENCE',
    'DATA_PLANE_POLICY_CALLER_SERVICE_ID',
  ]) {
    delete process.env[name];
  }
  process.env.PLANE_TOKEN_ISSUER = ISSUER;
  process.env.PLANE_SERVICE_PRINCIPALS_JSON = registryWithTestCredentials();
  // org-core as the binding check meets it: service authentication on every
  // route (no credential, no answer) and a directory that holds the fixture org.
  process.env.ORG_SERVICE_URL = 'http://org-core.contract.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_FIXTURE_TOKEN;
  resetInteractiveRetentionCacheForTests();
  const orgCore = jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      const answer = (status: number, body: unknown) =>
        Promise.resolve({
          ok: status === 200,
          status,
          json: () => Promise.resolve(body),
        } as never);
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== ORG_CORE_FIXTURE_TOKEN
      ) {
        return answer(401, { error: 'unauthorized' });
      }
      return url.pathname === `/internal/orgs/${ORG_ID}/retention-posture`
        ? answer(200, { zdrIntent: false, entitled: true })
        : answer(404, { error: 'organization not found' });
    });
  const now = jest
    .spyOn(Date, 'now')
    .mockReturnValue(FROZEN_NOW_SECONDS * 1000);
  try {
    return await body();
  } finally {
    now.mockRestore();
    orgCore.mockRestore();
    resetInteractiveRetentionCacheForTests();
    process.env = saved;
  }
}

async function buildClaimsGolden(): Promise<ClaimsGolden> {
  return withContractEnvironment(async () => {
    const tokens = new ConvexTokenService();
    const audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'contract', seq: 1 }),
    };
    const controller = new PlaneTokenController(tokens, audit as never);

    // The real service mint: the registry row decides audience, tenant, the
    // scope ceiling and the retention posture; the request can only narrow.
    const service = await controller.issueInternalToken(
      'data-plane',
      'quarry-edge',
      'test-only-quarry-edge-key',
      {
        orgId: ORG_ID,
        scopes: QUARRY_RETRIEVAL_SCOPES,
        reason: QUARRY_RETRIEVAL_REASON,
      },
    );

    // The real interactive mint (session cookie path).
    const interactive = async (userId: string, role: string) => {
      mockGetSession.mockResolvedValue({
        user: { id: userId, email: `${userId}@contract.test` },
        session: { activeOrganizationId: ORG_ID },
      });
      mockResolveCanonicalTokenContext.mockResolvedValue({
        orgId: ORG_ID,
        role,
      });
      mockResolveInteractiveRetentionPosture.mockResolvedValue({
        zdr: false,
        authority: 'interactive-org-retention-policy',
      });
      return controller.getToken('data-plane', { headers: {} } as never);
    };
    const member = await interactive(MEMBER_USER_ID, 'member');
    const admin = await interactive(ADMIN_USER_ID, 'admin');

    return {
      contract:
        'auth-core issuePlaneToken (data-plane) -> retrieval-engine admission',
      version: 1,
      audience: 'data-plane',
      issuer: ISSUER,
      orgId: ORG_ID,
      frozenNowSeconds: FROZEN_NOW_SECONDS,
      tokens: {
        serviceQuarryEdgeDataRead: tokenGolden(
          {
            route: 'POST /api/data-plane/internal-token',
            serviceId: 'quarry-edge',
            scopes: QUARRY_RETRIEVAL_SCOPES,
            reason: QUARRY_RETRIEVAL_REASON,
          },
          service.token,
        ),
        interactiveMember: tokenGolden(
          { route: 'GET /api/data-plane/token', role: 'member' },
          member.token,
        ),
        interactiveAdmin: tokenGolden(
          { route: 'GET /api/data-plane/token', role: 'admin' },
          admin.token,
        ),
      },
    };
  });
}

async function buildDecisionGolden(): Promise<DecisionGolden> {
  return withContractEnvironment(async () => {
    const tokens = new ConvexTokenService();
    const controller = Reflect.construct(DataPlaneAuthorizationController, [
      tokens,
    ]) as {
      decide(
        authorization: string | undefined,
        body: { userId?: string; orgId?: string; action?: string },
      ): Promise<DataPlaneDecision>;
    };
    // The caller the retrieval engine's HttpPolicyClient really is: a
    // control-policy service token with the single decide scope.
    const policyToken = tokens.issuePlaneToken('control-policy', {
      userId: 'service:retrieval-engine',
      orgId: ORG_ID,
      scopes: ['data:authorization:decide'],
      principalType: 'service',
      serviceId: 'retrieval-engine',
      reason: 'authorize retrieval request',
    }).token;
    const authorization = `Bearer ${policyToken}`;

    const memberRows = (rows: MembershipRecord[]) => {
      jest.spyOn(db, 'select').mockReturnValue({
        from: () => ({
          where: () => ({ limit: jest.fn().mockResolvedValue(rows) }),
        }),
      } as never);
    };

    const ask = async (userId: string, rows: MembershipRecord[]) => {
      memberRows(rows);
      const request = { userId, orgId: ORG_ID, action: 'data.read' };
      const response = await controller.decide(authorization, request);
      return { request, response };
    };

    return {
      contract:
        'auth-core data-plane authorization decision -> retrieval-engine HttpPolicyClient',
      version: 1,
      orgId: ORG_ID,
      decisions: {
        // The `member` table has no row for a service subject: Better Auth
        // keys rows by user id. This is the live not_member that caused the 403.
        serviceSubjectNotMember: await ask('service:quarry-edge', []),
        memberRead: await ask(MEMBER_USER_ID, [
          { role: 'member', createdAt: MEMBER_CREATED_AT },
        ]),
        adminRead: await ask(ADMIN_USER_ID, [
          { role: 'admin', createdAt: MEMBER_CREATED_AT },
        ]),
      },
    };
  });
}

function serialise(value: unknown): Buffer {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

function writeWithPin(dir: string, name: string, bytes: Buffer): void {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, name), bytes);
  writeFileSync(join(dir, `${name}.sha256`), pinLine(name, bytes));
}

function failOrSkip(reason: string): void {
  if (requireContract) {
    throw new Error(
      `REQUIRE_CONTRACT/CI is set but ${reason}; regenerate with UPDATE_CONTRACT_GOLDEN=1`,
    );
  }
  console.warn(`data-plane service-principal contract part skipped: ${reason}`);
}

describe('Auth Core -> Data Plane service-principal contract', () => {
  afterEach(() => {
    jest.restoreAllMocks();
  });

  if (updateGolden) {
    it('writes both goldens, their pins and the vendored Data Plane copies (UPDATE_CONTRACT_GOLDEN=1)', async () => {
      if (!policy) {
        throw new Error(`policy file absent: ${POLICY_PATH}`);
      }
      const claims = serialise(await buildClaimsGolden());
      const decisions = serialise(await buildDecisionGolden());
      for (const dir of [AUTH_FIXTURES, DATA_PLANE_FIXTURES]) {
        writeWithPin(dir, CLAIMS_GOLDEN, claims);
        writeWithPin(dir, DECISION_GOLDEN, decisions);
      }
    });
    return;
  }

  it('keeps each committed golden and its sha256 pin consistent (Auth Core copy)', () => {
    for (const name of [CLAIMS_GOLDEN, DECISION_GOLDEN]) {
      const goldenPath = join(AUTH_FIXTURES, name);
      const pinPath = `${goldenPath}.sha256`;
      expect(existsSync(goldenPath)).toBe(true);
      expect(existsSync(pinPath)).toBe(true);
      expect(pinnedHash(pinPath)).toBe(sha256Lf(readFileSync(goldenPath)));
    }
  });

  it('is byte-identical to the vendored Data Plane copies, pins included', () => {
    if (!existsSync(DATA_PLANE_FIXTURES)) {
      failOrSkip(`the Data Plane tree is absent: ${DATA_PLANE_FIXTURES}`);
      return;
    }
    for (const name of [CLAIMS_GOLDEN, DECISION_GOLDEN]) {
      const canonical = readFileSync(join(AUTH_FIXTURES, name));
      const vendoredPath = join(DATA_PLANE_FIXTURES, name);
      expect(existsSync(vendoredPath)).toBe(true);
      const vendored = readFileSync(vendoredPath);
      expect(sha256Lf(vendored)).toBe(sha256Lf(canonical));
      expect(vendored.toString('utf8').replace(/\r\n/g, '\n')).toBe(
        canonical.toString('utf8').replace(/\r\n/g, '\n'),
      );
      expect(pinnedHash(`${vendoredPath}.sha256`)).toBe(
        pinnedHash(join(AUTH_FIXTURES, `${name}.sha256`)),
      );
    }
  });

  describe('regenerated from the real mint', () => {
    it('reproduces the committed claims golden from the real controller and registry', async () => {
      if (!policy) {
        failOrSkip(`the policy file is absent: ${POLICY_PATH}`);
        return;
      }
      const committed = JSON.parse(
        readFileSync(join(AUTH_FIXTURES, CLAIMS_GOLDEN), 'utf8'),
      ) as ClaimsGolden;
      expect(await buildClaimsGolden()).toEqual(committed);
    });

    it('reproduces the committed decision golden from the real decision controller', async () => {
      if (!policy) {
        failOrSkip(`the policy file is absent: ${POLICY_PATH}`);
        return;
      }
      const committed = JSON.parse(
        readFileSync(join(AUTH_FIXTURES, DECISION_GOLDEN), 'utf8'),
      ) as DecisionGolden;
      expect(await buildDecisionGolden()).toEqual(committed);
    });
  });

  describe('what the goldens assert about the contract', () => {
    const claimsGolden = (): ClaimsGolden =>
      JSON.parse(
        readFileSync(join(AUTH_FIXTURES, CLAIMS_GOLDEN), 'utf8'),
      ) as ClaimsGolden;
    const decisionGolden = (): DecisionGolden =>
      JSON.parse(
        readFileSync(join(AUTH_FIXTURES, DECISION_GOLDEN), 'utf8'),
      ) as DecisionGolden;

    it('records the claim key set the mint really emits for every token', () => {
      for (const token of Object.values(claimsGolden().tokens)) {
        expect(token.claimKeys).toEqual(Object.keys(token.claims).sort());
      }
    });

    it('shapes the quarry-edge service token exactly as the engine must validate it', () => {
      const { claims } = claimsGolden().tokens.serviceQuarryEdgeDataRead;
      expect(claims).toMatchObject({
        iss: ISSUER,
        aud: 'data-plane',
        sub: 'service:quarry-edge',
        service_id: 'service:quarry-edge',
        principal_type: 'service',
        org_id: ORG_ID,
        scopes: QUARRY_RETRIEVAL_SCOPES,
        reason: QUARRY_RETRIEVAL_REASON,
        // quarry-edge's data-plane retention is `persistent` in the registry.
        zdr: false,
      });
      // The two fields that make a token ambiguous are absent.
      expect(claims).not.toHaveProperty('user_id');
      expect(claims).not.toHaveProperty('acting_user_id');
      // No org-wide visibility scope was requested or granted.
      expect(claims.scopes).not.toContain('org:data:read_all');
    });

    it('shapes the interactive tokens as users with a user_id equal to sub', () => {
      const { interactiveMember, interactiveAdmin } = claimsGolden().tokens;
      expect(interactiveMember.claims).toMatchObject({
        sub: MEMBER_USER_ID,
        user_id: MEMBER_USER_ID,
        principal_type: 'user',
        org_id: ORG_ID,
        zdr: false,
      });
      expect(interactiveMember.claims).not.toHaveProperty('service_id');
      expect(interactiveAdmin.claims.scopes).toContain('org:data:read_all');
    });

    it('answers not_member for a service subject, which is the live 403', () => {
      const { serviceSubjectNotMember } = decisionGolden().decisions;
      expect(serviceSubjectNotMember.response).toEqual(
        buildDataPlaneDecision(null, 'data.read'),
      );
      expect(serviceSubjectNotMember.response).toMatchObject({
        version: 'v1',
        allowed: false,
        reason: 'not_member',
      });
    });

    it('answers member with the data:read permission for a real member', () => {
      const { memberRead } = decisionGolden().decisions;
      expect(memberRead.response).toEqual(
        buildDataPlaneDecision(
          { role: 'member', createdAt: MEMBER_CREATED_AT },
          'data.read',
        ),
      );
      expect(memberRead.response.permissions).toContain('data:read');
    });
  });
});
