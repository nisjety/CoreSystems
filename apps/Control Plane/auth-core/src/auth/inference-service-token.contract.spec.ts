import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { HttpException } from '@nestjs/common';

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import { ConvexTokenService } from './convex-token.service';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';
import { PlaneTokenController } from './plane-token.controller';

/**
 * Cross-language contract: what Auth Core REALLY answers when a Data Plane
 * background worker asks for an `aud=inference-core` service token, and what the
 * worker's principal row looks like. The Data Plane inference client
 * (services/inference-client-rs, tests/token_contract.rs and tests/real_policy.rs)
 * replays every case through its real response validation, its real bearer cache
 * and its real status handling.
 *
 * Why it exists: the two copies of that client were tested with claims typed by
 * hand. The posture assertion they make (`zdr` equals the configured posture)
 * once killed embedding silently when the registry and the client disagreed, and
 * the new `tenant` posture (the token's `zdr` is the ORGANIZATION's setting,
 * lifetime capped at 60 s, 503 when the posture cannot be resolved) has three
 * outcomes the client must tell apart. A hand-written token cannot show that the
 * client agrees with the issuer.
 *
 * Every case is written by Auth Core's real mint path: the real registry rows for
 * embedding-engine and graph-index from config/plane-service-principals.json, the
 * real authorizePlaneServicePrincipal, the real organization-binding check, the
 * real tenant posture resolver (org-core is the only double: it enforces service
 * authentication exactly as the contract in
 * fixtures/auth-core-org-core-retention-posture.contract.json says) and the real
 * ConvexTokenService.issuePlaneToken that signs the RS256 JWT. The tokens in the
 * golden are those real compact JWTs (the header and payload segments exactly as
 * the mint produced them) with the signature segment replaced by a fixed marker.
 * The signing key is ephemeral, so a real signature would change on every run and
 * bury every real change in noise; the client does not verify signatures
 * (inference-core does), and the claims are what it judges.
 *
 * Time is frozen so the claims are reproducible. The Rust side injects a clock
 * set relative to `frozenNowSeconds`.
 *
 * Each case names the registry value it was minted under in `registryRetention`.
 * The cases cover every value a row can carry, whatever the committed row says
 * (the committed rows are in `registry`, and are checked against the policy file):
 *   - `persistent`: the row as it was before the enablement step, and what a
 *     deployment that has not replaced its registry still mints.
 *   - `zdr`: the posture a principal gets when the row is `zdr`.
 *   - `tenant`: the row of the enablement step: the token's `zdr` is the
 *     organization's own setting, lifetime capped at 60 s. The Data Plane client
 *     must work against both sides of the flip, because the clients ship first.
 *
 * HTTP status of a success is the one Nest gives a `@Post` handler without
 * `@HttpCode` (201); an error carries the exception's own status and body.
 *
 * Regenerate after an intentional change (also vendors the copy and pins):
 *   UPDATE_CONTRACT_GOLDEN=1 npx jest inference-service-token.contract
 *
 * Files and pins:
 *   - canonical copy: src/auth/fixtures/inference-service-tokens.golden.json
 *     (+ .sha256 pin), inside this service's Docker build context.
 *   - vendored copy: Data Plane v2/services/inference-client-rs/tests/fixtures/
 *     auth-core/ (+ .sha256 pin), inside the client's own tree. Both pins are the
 *     sha256 of the file with CRLF normalised to LF; this spec fails when the
 *     copies differ.
 *
 * The policy file and the Data Plane tree sit outside this service's Docker build
 * context. Without REQUIRE_CONTRACT (or CI) a missing one skips the part that
 * needs it; with either set a missing file FAILS, so a pipeline cannot go green
 * by not finding the contract.
 */
const POLICY_PATH = join(
  __dirname,
  '../../../config/plane-service-principals.json',
);
const AUTH_FIXTURES = join(__dirname, 'fixtures');
const DATA_PLANE_FIXTURES = join(
  __dirname,
  '../../../../Data Plane v2/services/inference-client-rs/tests/fixtures/auth-core',
);
const GOLDEN = 'inference-service-tokens.golden.json';

const ISSUER = 'https://control.contract.test/api/convex-auth';
const ORG_PLAIN = 'org-contract-plain';
const ORG_ZDR = 'org-contract-zdr';
const ORG_UNKNOWN = 'org-contract-unknown';
// 2026-10-06T12:00:00Z
const FROZEN_NOW_SECONDS = Date.UTC(2026, 9, 6, 12, 0, 0) / 1000;
const INFERENCE_SCOPES = ['inference:invoke'];
const ORG_CORE_FIXTURE_TOKEN = 'inference-contract-org-core-token-0123456789';
const AUDIENCE = 'inference-core';
// Longest a token minted for a `tenant` principal may live
// (TENANT_PRINCIPAL_TOKEN_TTL_SECONDS in convex-token.service.ts).
const TENANT_TTL_SECONDS = 60;
const DEFAULT_TTL_SECONDS = 300;
// Stands where the signature segment of every token in the golden was.
const SIGNATURE_MARKER = 'signature-omitted-from-golden';

// The vocabulary of `reason` the Data Plane client sends per feature. The
// client's `Reason` enum (src/token.rs) is the other half of this list, and its
// test fails when the two differ.
const REASONS = {
  documentEmbedding: 'document-embedding',
  contextualRetrieval: 'contextual-retrieval',
  videoCaption: 'video-caption',
  graphExtraction: 'graph-extraction',
  graphCommunitySummary: 'graph-community-summary',
} as const;

const SERVICES = ['embedding-engine', 'graph-index'] as const;
type ServiceId = (typeof SERVICES)[number];

const requireContract = Boolean(process.env.REQUIRE_CONTRACT || process.env.CI);
const updateGolden = process.env.UPDATE_CONTRACT_GOLDEN === '1';

type PolicyRow = Record<string, unknown>;
const policy: Record<string, PolicyRow> | null = existsSync(POLICY_PATH)
  ? (JSON.parse(readFileSync(POLICY_PATH, 'utf8')) as Record<string, PolicyRow>)
  : null;

type RegistryRetention = 'persistent' | 'zdr' | 'tenant';
type OrgCorePosture = 'plain' | 'zdr' | 'unknown' | 'down';
type Tenancy = 'directory' | 'fixed';

type CaseSpec = {
  serviceId: ServiceId;
  orgId: string;
  reason: string;
  registryRetention: RegistryRetention;
  tenancy: Tenancy;
  orgCore: OrgCorePosture;
  credential?: string;
  /** Where `registryRetention` and `tenancy` come from, for the reader. */
  note: string;
};

type CaseGolden = {
  note: string;
  request: {
    route: string;
    serviceId: string;
    orgId: string;
    scopes: string[];
    reason: string;
    registryRetention: RegistryRetention;
    tenancy: Tenancy;
    orgCore: OrgCorePosture;
  };
  response: { status: number; body: unknown };
  header?: Record<string, unknown>;
  claims?: Record<string, unknown>;
};

type Golden = {
  contract: string;
  version: number;
  audience: string;
  issuer: string;
  frozenNowSeconds: number;
  tenantTtlSeconds: number;
  defaultTtlSeconds: number;
  reasons: string[];
  registry: Record<string, PolicyRow>;
  cases: Record<string, CaseGolden>;
};

const SUCCESS_CASES: Record<string, CaseSpec> = {
  embeddingEnginePersistent: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.documentEmbedding,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row as it was before the enablement step; what an unflipped registry mints',
  },
  embeddingEngineContextualRetrievalPersistent: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row as it was before the enablement step; what an unflipped registry mints',
  },
  embeddingEngineVideoCaptionPersistent: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.videoCaption,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row as it was before the enablement step; what an unflipped registry mints',
  },
  embeddingEngineZdrRegistry: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.documentEmbedding,
    registryRetention: 'zdr',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row flipped to zdr: the posture is the registry, not the org',
  },
  embeddingEngineTenantOrgPlain: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row of the enablement step; the org has no ZDR',
  },
  embeddingEngineTenantOrgZdr: {
    serviceId: 'embedding-engine',
    orgId: ORG_ZDR,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'zdr',
    note: 'the row of the enablement step; the org has ZDR on',
  },
  graphIndexPersistent: {
    serviceId: 'graph-index',
    orgId: ORG_PLAIN,
    reason: REASONS.graphExtraction,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row as it was before the enablement step; what an unflipped registry mints',
  },
  graphIndexCommunitySummaryPersistent: {
    serviceId: 'graph-index',
    orgId: ORG_PLAIN,
    reason: REASONS.graphCommunitySummary,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row as it was before the enablement step; what an unflipped registry mints',
  },
  graphIndexTenantOrgPlain: {
    serviceId: 'graph-index',
    orgId: ORG_PLAIN,
    reason: REASONS.graphExtraction,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'plain',
    note: 'the row of the enablement step; the org has no ZDR',
  },
  graphIndexTenantOrgZdr: {
    serviceId: 'graph-index',
    orgId: ORG_ZDR,
    reason: REASONS.graphExtraction,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'zdr',
    note: 'the row of the enablement step; the org has ZDR on',
  },
};

const ERROR_CASES: Record<string, CaseSpec> = {
  tenantDirectoryUnavailable: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'down',
    note: 'org-core unreachable for a directory-bound tenant principal: the directory check answers first',
  },
  tenantPostureUnavailable: {
    serviceId: 'embedding-engine',
    orgId: ORG_PLAIN,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'tenant',
    tenancy: 'fixed',
    orgCore: 'down',
    note: 'org-core unreachable for a principal bound to a fixed org: only the posture read fails',
  },
  organizationNotInDirectory: {
    serviceId: 'embedding-engine',
    orgId: ORG_UNKNOWN,
    reason: REASONS.contextualRetrieval,
    registryRetention: 'tenant',
    tenancy: 'directory',
    orgCore: 'unknown',
    note: 'org-core has no record of the organization: refused with the same 403 as any unauthorised caller',
  },
  wrongCredential: {
    serviceId: 'graph-index',
    orgId: ORG_PLAIN,
    reason: REASONS.graphExtraction,
    registryRetention: 'persistent',
    tenancy: 'directory',
    orgCore: 'plain',
    credential: 'not-the-registered-credential',
    note: 'the credential does not match the registry',
  },
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

function credentialFor(serviceId: string): string {
  return `test-only-${serviceId}-key`;
}

/** The committed rows of the two principals, credentials left out. */
function committedRegistrySlice(): Record<string, PolicyRow> {
  if (!policy) throw new Error(`policy file absent: ${POLICY_PATH}`);
  const slice: Record<string, PolicyRow> = {};
  for (const id of SERVICES) {
    const row = policy[id];
    if (!row) throw new Error(`policy file has no principal ${id}`);
    slice[id] = JSON.parse(JSON.stringify(row)) as PolicyRow;
  }
  return slice;
}

function registryFor(spec: CaseSpec): string {
  const registry: Record<string, PolicyRow> = {};
  for (const [id, row] of Object.entries(committedRegistrySlice())) {
    const next: PolicyRow = {
      ...row,
      credential: credentialFor(id),
    };
    if (id === spec.serviceId) {
      next.retentionByAudience = {
        ...(row.retentionByAudience as Record<string, string>),
        [AUDIENCE]: spec.registryRetention,
      };
      if (spec.tenancy === 'fixed') {
        delete next.tenancy;
        next.orgIds = [spec.orgId];
      }
    }
    registry[id] = next;
  }
  return JSON.stringify(registry);
}

type OrgCoreDouble = { state: OrgCorePosture; plain: string; zdr: string };

/** Runs `body` with the environment the real mint reads, restored afterwards. */
async function withContractEnvironment<T>(
  orgCore: OrgCoreDouble,
  body: () => Promise<T>,
): Promise<T> {
  const saved = { ...process.env };
  for (const name of Object.keys(process.env)) {
    if (name.startsWith('PLANE_TOKEN_TTL_')) delete process.env[name];
  }
  for (const name of [
    'CONVEX_AUTH_PRIVATE_KEY_FILE',
    'CONVEX_AUTH_PRIVATE_KEY_PEM',
    'CONVEX_AUTH_PUBLIC_KEY_FILE',
    'CONVEX_AUTH_PUBLIC_KEY_PEM',
    'PLANE_SERVICE_PRINCIPALS_FILE',
  ]) {
    delete process.env[name];
  }
  process.env.NODE_ENV = 'test';
  process.env.PLANE_TOKEN_ISSUER = ISSUER;
  // org-core as the binding check and the posture resolver meet it: service
  // authentication on every route (no credential, no answer), the posture
  // route, and its own "organization not found" 404.
  process.env.ORG_SERVICE_URL = 'http://org-core.contract.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_FIXTURE_TOKEN;
  resetInteractiveRetentionCacheForTests();
  const fetchSpy = jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      if (orgCore.state === 'down') {
        return Promise.reject(new TypeError('fetch failed'));
      }
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      const answer = (status: number, payload: unknown) =>
        Promise.resolve({
          ok: status === 200,
          status,
          json: () => Promise.resolve(payload),
        } as never);
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== ORG_CORE_FIXTURE_TOKEN
      ) {
        return answer(401, { error: 'unauthorized' });
      }
      if (
        url.pathname === `/internal/orgs/${orgCore.plain}/retention-posture`
      ) {
        return answer(200, { zdrIntent: false, entitled: true });
      }
      if (url.pathname === `/internal/orgs/${orgCore.zdr}/retention-posture`) {
        return answer(200, { zdrIntent: true, entitled: true });
      }
      return answer(404, { error: 'organization not found' });
    });
  const now = jest
    .spyOn(Date, 'now')
    .mockReturnValue(FROZEN_NOW_SECONDS * 1000);
  try {
    return await body();
  } finally {
    now.mockRestore();
    fetchSpy.mockRestore();
    resetInteractiveRetentionCacheForTests();
    process.env = saved;
  }
}

/** One case through the real controller. Call inside withContractEnvironment. */
async function mintCase(
  tokens: ConvexTokenService,
  orgCore: OrgCoreDouble,
  spec: CaseSpec,
): Promise<CaseGolden> {
  orgCore.state = spec.orgCore;
  // Every case meets a cold resolver: nothing remembered from an earlier case.
  resetInteractiveRetentionCacheForTests();
  process.env.PLANE_SERVICE_PRINCIPALS_JSON = registryFor(spec);
  const audit = {
    publishAuditDurable: jest
      .fn()
      .mockResolvedValue({ stream: 'contract', seq: 1 }),
  };
  const controller = new PlaneTokenController(tokens, audit as never);
  const request = {
    route: `POST /api/${AUDIENCE}/internal-token`,
    serviceId: spec.serviceId,
    orgId: spec.orgId,
    scopes: INFERENCE_SCOPES,
    reason: spec.reason,
    registryRetention: spec.registryRetention,
    tenancy: spec.tenancy,
    orgCore: spec.orgCore,
  };
  try {
    const body = await controller.issueInternalToken(
      AUDIENCE,
      spec.serviceId,
      spec.credential ?? credentialFor(spec.serviceId),
      {
        orgId: spec.orgId,
        scopes: INFERENCE_SCOPES,
        reason: spec.reason,
      },
    );
    const { header, claims } = decodeJwt(body.token);
    return {
      note: spec.note,
      request,
      response: { status: 201, body: { ...body, token: unsigned(body.token) } },
      header,
      claims,
    };
  } catch (error) {
    if (error instanceof HttpException) {
      return {
        note: spec.note,
        request,
        response: { status: error.getStatus(), body: error.getResponse() },
      };
    }
    throw error;
  }
}

async function buildGolden(): Promise<Golden> {
  const orgCore: OrgCoreDouble = {
    state: 'plain',
    plain: ORG_PLAIN,
    zdr: ORG_ZDR,
  };
  // One environment for the whole build: the token service reads its issuer and
  // audience configuration when it is constructed.
  const cases = await withContractEnvironment(orgCore, async () => {
    const tokens = new ConvexTokenService();
    const minted: Record<string, CaseGolden> = {};
    for (const [name, spec] of Object.entries({
      ...SUCCESS_CASES,
      ...ERROR_CASES,
    })) {
      minted[name] = await mintCase(tokens, orgCore, spec);
    }
    return minted;
  });
  return {
    contract:
      'auth-core POST /api/inference-core/internal-token -> Data Plane inference client',
    version: 1,
    audience: AUDIENCE,
    issuer: ISSUER,
    frozenNowSeconds: FROZEN_NOW_SECONDS,
    tenantTtlSeconds: TENANT_TTL_SECONDS,
    defaultTtlSeconds: DEFAULT_TTL_SECONDS,
    reasons: Object.values(REASONS),
    registry: committedRegistrySlice(),
    cases,
  };
}

/** The token with its (ephemeral-key) signature segment replaced by the marker. */
function unsigned(token: string): string {
  const [header, payload] = token.split('.');
  return `${header}.${payload}.${SIGNATURE_MARKER}`;
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
  console.warn(`inference service-token contract part skipped: ${reason}`);
}

const committedGolden = (): Golden =>
  JSON.parse(readFileSync(join(AUTH_FIXTURES, GOLDEN), 'utf8')) as Golden;

describe('Auth Core -> Data Plane inference client token contract', () => {
  afterEach(() => {
    jest.restoreAllMocks();
  });

  if (updateGolden) {
    it('writes the golden, its pin and the vendored Data Plane copy (UPDATE_CONTRACT_GOLDEN=1)', async () => {
      if (!policy) {
        throw new Error(`policy file absent: ${POLICY_PATH}`);
      }
      const bytes = serialise(await buildGolden());
      for (const dir of [AUTH_FIXTURES, DATA_PLANE_FIXTURES]) {
        writeWithPin(dir, GOLDEN, bytes);
      }
    });
    return;
  }

  it('keeps the committed golden and its sha256 pin consistent (Auth Core copy)', () => {
    const goldenPath = join(AUTH_FIXTURES, GOLDEN);
    expect(existsSync(goldenPath)).toBe(true);
    expect(existsSync(`${goldenPath}.sha256`)).toBe(true);
    expect(pinnedHash(`${goldenPath}.sha256`)).toBe(
      sha256Lf(readFileSync(goldenPath)),
    );
  });

  it('is byte-identical to the vendored Data Plane copy, pin included', () => {
    if (!existsSync(DATA_PLANE_FIXTURES)) {
      failOrSkip(`the Data Plane tree is absent: ${DATA_PLANE_FIXTURES}`);
      return;
    }
    const canonical = readFileSync(join(AUTH_FIXTURES, GOLDEN));
    const vendoredPath = join(DATA_PLANE_FIXTURES, GOLDEN);
    expect(existsSync(vendoredPath)).toBe(true);
    const vendored = readFileSync(vendoredPath);
    expect(sha256Lf(vendored)).toBe(sha256Lf(canonical));
    expect(vendored.toString('utf8').replace(/\r\n/g, '\n')).toBe(
      canonical.toString('utf8').replace(/\r\n/g, '\n'),
    );
    expect(pinnedHash(`${vendoredPath}.sha256`)).toBe(
      pinnedHash(join(AUTH_FIXTURES, `${GOLDEN}.sha256`)),
    );
  });

  describe('regenerated from the real mint', () => {
    it('reproduces the committed golden from the real controller, registry and resolver', async () => {
      if (!policy) {
        failOrSkip(`the policy file is absent: ${POLICY_PATH}`);
        return;
      }
      expect(await buildGolden()).toEqual(committedGolden());
    });

    it('commits the registry rows exactly as the policy file holds them', () => {
      if (!policy) {
        failOrSkip(`the policy file is absent: ${POLICY_PATH}`);
        return;
      }
      expect(committedGolden().registry).toEqual(committedRegistrySlice());
    });
  });

  describe('what the golden asserts about the contract', () => {
    const successCases = () =>
      Object.entries(committedGolden().cases).filter(
        ([name]) => name in SUCCESS_CASES,
      );

    it('commits every case this spec generates, and nothing else', () => {
      expect(Object.keys(committedGolden().cases).sort()).toEqual(
        [...Object.keys(SUCCESS_CASES), ...Object.keys(ERROR_CASES)].sort(),
      );
    });

    it('shapes every success exactly as the client must validate it', () => {
      for (const [name, entry] of successCases()) {
        const spec = SUCCESS_CASES[name];
        const body = entry.response.body as Record<string, unknown>;
        const claims = entry.claims as Record<string, unknown>;
        expect(entry.response.status).toBe(201);
        expect(Object.keys(body).sort()).toEqual([
          'audience',
          'expiresAt',
          'expiresInSeconds',
          'issuer',
          'token',
        ]);
        expect(body.audience).toBe(AUDIENCE);
        expect(body.issuer).toBe(ISSUER);
        expect(entry.header).toMatchObject({ alg: 'RS256', typ: 'JWT' });
        expect(claims).toMatchObject({
          iss: ISSUER,
          aud: AUDIENCE,
          sub: `service:${spec.serviceId}`,
          service_id: `service:${spec.serviceId}`,
          principal_type: 'service',
          org_id: spec.orgId,
          scopes: INFERENCE_SCOPES,
          reason: spec.reason,
        });
        expect(claims).not.toHaveProperty('user_id');
        expect(claims).not.toHaveProperty('acting_user_id');
        const ttl = (claims.exp as number) - (claims.iat as number);
        expect(body.expiresInSeconds).toBe(ttl);
        expect(body.expiresAt).toBe(
          new Date((claims.exp as number) * 1000).toISOString(),
        );
        expect(claims.iat).toBe(FROZEN_NOW_SECONDS);
        // The mint backdates `nbf` by five seconds.
        expect(claims.nbf).toBe(FROZEN_NOW_SECONDS - 5);
      }
    });

    it('carries the registry posture for persistent and zdr principals, 300 s', () => {
      for (const [name, entry] of successCases()) {
        const spec = SUCCESS_CASES[name];
        if (spec.registryRetention === 'tenant') continue;
        const claims = entry.claims as Record<string, unknown>;
        expect(claims.zdr).toBe(spec.registryRetention === 'zdr');
        expect(claims.exp).toBe(FROZEN_NOW_SECONDS + DEFAULT_TTL_SECONDS);
      }
    });

    it('carries the ORGANIZATION posture for a tenant principal, capped at 60 s', () => {
      let seen = 0;
      for (const [name, entry] of successCases()) {
        const spec = SUCCESS_CASES[name];
        if (spec.registryRetention !== 'tenant') continue;
        seen += 1;
        const claims = entry.claims as Record<string, unknown>;
        expect(claims.zdr).toBe(spec.orgCore === 'zdr');
        expect(claims.exp).toBe(FROZEN_NOW_SECONDS + TENANT_TTL_SECONDS);
      }
      expect(seen).toBe(4);
    });

    it('answers an unresolvable posture with a typed 503 and no token', () => {
      const golden = committedGolden();
      for (const [name, error] of [
        ['tenantPostureUnavailable', 'retention_posture_unavailable'],
        ['tenantDirectoryUnavailable', 'organization_directory_unavailable'],
      ] as const) {
        const entry = golden.cases[name];
        expect(entry.response.status).toBe(503);
        expect(entry.response.body).toMatchObject({
          statusCode: 503,
          error,
        });
        expect(JSON.stringify(entry.response.body)).not.toContain('eyJ');
      }
    });

    it('refuses an unknown organization and a wrong credential with the same 403', () => {
      const golden = committedGolden();
      for (const name of ['organizationNotInDirectory', 'wrongCredential']) {
        expect(golden.cases[name].response.status).toBe(403);
        expect(golden.cases[name].response.body).toMatchObject({
          statusCode: 403,
          message: 'Service principal is not authorized',
        });
      }
    });

    it('lists every reason the principals mint with', () => {
      expect(committedGolden().reasons).toEqual(Object.values(REASONS));
    });
  });
});
