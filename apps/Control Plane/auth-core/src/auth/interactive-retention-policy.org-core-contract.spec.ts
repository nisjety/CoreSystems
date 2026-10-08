import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  resetInteractiveRetentionCacheForTests,
  resolveInteractiveRetentionPosture,
} from './interactive-retention-policy';

/**
 * Cross-language contract: the EXACT request Auth Core's retention resolver
 * sends to org-core, recorded in a golden file that org-core's Go test
 * (`internal/http/retention_posture_contract_test.go`) replays through org-core's
 * REAL service-auth middleware, REAL route table and REAL compose credential
 * block.
 *
 * Why this exists: the resolver used to call org-core with no service headers.
 * org-core answered 401 to every lookup, the resolver swallowed that as an
 * "outage", and every org resolved to `zdr:false` — a ZDR org's setting never
 * reached a token, and no test noticed because the unit spec mocked `fetch`
 * and enforced no authentication. This spec drives the real resolver, records
 * what it really puts on the wire, and fails when that differs from the
 * golden. The Go side then fails when org-core would not accept the golden.
 * A header or path change on either side now breaks a test instead of
 * silently returning `zdr:false`.
 *
 * Regenerate after an intentional client change:
 *   UPDATE_CONTRACT_GOLDEN=1 npx jest interactive-retention-policy.org-core-contract
 * and then run org-core's Go contract test.
 *
 * Files and pins (each consuming tree holds its own copy, so each side runs
 * from its own Docker build context and reads nothing outside it):
 *   - canonical copy: src/auth/fixtures/<name> (+ .sha256 pin), here.
 *   - vendored copy: org-core/internal/http/testdata/<name> (+ .sha256 pin),
 *     inside org-core's build context. Both pins are the sha256 of the file with
 *     CRLF normalised to LF, and this spec fails when the copies differ.
 * The repo-level drift check (scripts/tests/zdr-contract-sync.test.sh) pins both.
 *
 * Only the org-core copy sits outside this service's tree. When that tree is
 * absent the comparison is skipped, EXCEPT under REQUIRE_CONTRACT, where the
 * absence fails: a contract that quietly skips is not a contract.
 */
const GOLDEN_NAME = 'auth-core-org-core-retention-posture.contract.json';
const AUTH_FIXTURES = join(__dirname, 'fixtures');
const ORG_CORE_FIXTURES = join(
  __dirname,
  '../../../org-core/internal/http/testdata',
);
const GOLDEN_PATH = join(AUTH_FIXTURES, GOLDEN_NAME);
const TOKEN_PLACEHOLDER = '<ORG_CORE_SERVICE_TOKEN>';
const FIXTURE_TOKEN = 'contract-fixture-service-token-0123456789abcdef';
const FIXTURE_ORG_ID = 'org-contract-fixture';
const FIXTURE_BASE_URL = 'http://org-core.contract.test:8080';
const requireContract = Boolean(process.env.REQUIRE_CONTRACT);
const updateGolden = process.env.UPDATE_CONTRACT_GOLDEN === '1';

type RecordedRequest = {
  method: string;
  path: string;
  headers: Record<string, string>;
};

type Golden = {
  contract: string;
  version: number;
  organizationId: string;
  tokenPlaceholder: string;
  requests: RecordedRequest[];
  response: { status: number; body: Record<string, unknown> };
  notFound: { status: number; body: Record<string, unknown> };
};

const RESPONSE_EXAMPLE = {
  status: 200,
  body: { zdrIntent: true, entitled: true },
};

// org-core's answer for an organization it has no active record of. It is the
// only 404 the resolver accepts as "no such org"; see fetchPosture.
const NOT_FOUND_EXAMPLE = {
  status: 404,
  body: { error: 'organization not found' },
};

function normalizeRequest(input: unknown, init?: RequestInit): RecordedRequest {
  const url = new URL(String(input));
  const headers = new Headers(init?.headers);
  const normalized: Record<string, string> = {};
  for (const [name, value] of [...headers.entries()].sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    normalized[name] = value === FIXTURE_TOKEN ? TOKEN_PLACEHOLDER : value;
  }
  return {
    method: (init?.method ?? 'GET').toUpperCase(),
    path: `${url.pathname}${url.search}`,
    headers: normalized,
  };
}

function sha256Lf(bytes: Buffer): string {
  return createHash('sha256')
    .update(bytes.toString('utf8').replace(/\r\n/g, '\n'))
    .digest('hex');
}

function pinnedHash(pinPath: string): string {
  return readFileSync(pinPath, 'utf8').trim().split(/\s+/)[0];
}

function writeWithPin(dir: string, bytes: Buffer): void {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, GOLDEN_NAME), bytes);
  writeFileSync(
    join(dir, `${GOLDEN_NAME}.sha256`),
    `${sha256Lf(bytes)}  ${GOLDEN_NAME}\n`,
  );
}

describe('Auth Core -> org-core retention posture wire contract', () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    resetInteractiveRetentionCacheForTests();
    process.env.ORG_SERVICE_URL = FIXTURE_BASE_URL;
    delete process.env.ORG_CORE_URL;
    process.env.ORG_CORE_SERVICE_TOKEN = FIXTURE_TOKEN;
  });

  afterEach(() => {
    jest.restoreAllMocks();
    process.env = { ...originalEnv };
  });

  async function recordResolverRequests(): Promise<RecordedRequest[]> {
    const recorded: RecordedRequest[] = [];
    jest
      .spyOn(global, 'fetch')
      .mockImplementation((input: unknown, init?: RequestInit) => {
        recorded.push(normalizeRequest(input, init));
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () => Promise.resolve(RESPONSE_EXAMPLE.body),
        } as never);
      });
    await resolveInteractiveRetentionPosture(FIXTURE_ORG_ID).catch(
      () => undefined,
    );
    return recorded;
  }

  if (updateGolden) {
    it('writes the golden from the real resolver (UPDATE_CONTRACT_GOLDEN=1)', async () => {
      const requests = await recordResolverRequests();
      const golden: Golden = {
        contract: 'auth-core -> org-core retention posture',
        version: 1,
        organizationId: FIXTURE_ORG_ID,
        tokenPlaceholder: TOKEN_PLACEHOLDER,
        requests,
        response: RESPONSE_EXAMPLE,
        notFound: NOT_FOUND_EXAMPLE,
      };
      const bytes = Buffer.from(`${JSON.stringify(golden, null, 2)}\n`, 'utf8');
      writeWithPin(AUTH_FIXTURES, bytes);
      // Vendor into org-core's tree when it is present, so the two sides cannot
      // be regenerated apart.
      if (existsSync(join(ORG_CORE_FIXTURES, '..', '..', '..'))) {
        writeWithPin(ORG_CORE_FIXTURES, bytes);
      }
      expect(requests.length).toBeGreaterThan(0);
    });
    return;
  }

  // The canonical copy is inside this tree: it is there or the repository is
  // broken, with or without REQUIRE_CONTRACT.
  it('has the canonical golden and a sha256 pin that matches it', () => {
    expect(existsSync(GOLDEN_PATH)).toBe(true);
    expect(existsSync(`${GOLDEN_PATH}.sha256`)).toBe(true);
    expect(pinnedHash(`${GOLDEN_PATH}.sha256`)).toBe(
      sha256Lf(readFileSync(GOLDEN_PATH)),
    );
  });

  it("is byte-identical to org-core's vendored copy, pin included", () => {
    if (!existsSync(ORG_CORE_FIXTURES)) {
      if (requireContract) {
        throw new Error(
          `REQUIRE_CONTRACT is set but org-core's fixtures are absent: ${ORG_CORE_FIXTURES}`,
        );
      }
      console.warn(
        `org-core tree absent, skipping the vendored comparison: ${ORG_CORE_FIXTURES}`,
      );
      return;
    }
    const vendoredPath = join(ORG_CORE_FIXTURES, GOLDEN_NAME);
    expect(existsSync(vendoredPath)).toBe(true);
    const canonical = readFileSync(GOLDEN_PATH);
    const vendored = readFileSync(vendoredPath);
    expect(sha256Lf(vendored)).toBe(sha256Lf(canonical));
    expect(vendored.toString('utf8').replace(/\r\n/g, '\n')).toBe(
      canonical.toString('utf8').replace(/\r\n/g, '\n'),
    );
    expect(pinnedHash(`${vendoredPath}.sha256`)).toBe(
      pinnedHash(`${GOLDEN_PATH}.sha256`),
    );
  });

  const golden = JSON.parse(readFileSync(GOLDEN_PATH, 'utf8')) as Golden;

  it('sends exactly the requests recorded in the golden, with no unauthenticated org-core call', async () => {
    const requests = await recordResolverRequests();

    expect(requests).toEqual(golden.requests);
    // The live defect: a request with no service credential. org-core answers
    // 401 to it, so a golden without these headers can never be accepted.
    for (const request of requests) {
      expect(request.headers['x-service-id']).toBe('auth-core');
      expect(request.headers['x-service-token']).toBe(TOKEN_PLACEHOLDER);
    }
  });

  it('reads the response body shape recorded in the golden', async () => {
    jest.spyOn(global, 'fetch').mockResolvedValue({
      ok: true,
      status: golden.response.status,
      json: () => Promise.resolve(golden.response.body),
    } as never);

    await expect(
      resolveInteractiveRetentionPosture(FIXTURE_ORG_ID),
    ).resolves.toEqual({
      zdr:
        golden.response.body.zdrIntent === true &&
        golden.response.body.entitled === true,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('treats the golden not-found answer as "no record" (not ZDR, not remembered) and no other 404 as one', async () => {
    const fetchSpy = jest.spyOn(global, 'fetch').mockResolvedValue({
      ok: false,
      status: golden.notFound.status,
      json: () => Promise.resolve(golden.notFound.body),
    } as never);

    await expect(
      resolveInteractiveRetentionPosture(FIXTURE_ORG_ID),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
    await resolveInteractiveRetentionPosture(FIXTURE_ORG_ID);
    expect(fetchSpy).toHaveBeenCalledTimes(2);

    fetchSpy.mockResolvedValue({
      ok: false,
      status: 404,
      json: () => Promise.resolve({ error: 'page not found' }),
    } as never);
    await expect(
      resolveInteractiveRetentionPosture(FIXTURE_ORG_ID),
    ).rejects.toMatchObject({ reason: 'org_core_error_status' });
  });
});
