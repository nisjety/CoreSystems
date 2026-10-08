import {
  ForbiddenException,
  Logger,
  ServiceUnavailableException,
} from '@nestjs/common';

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import { ModelPlaneTokenController } from './model-plane-token.controller';
import { PlaneTokenController } from './plane-token.controller';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';

// Plan W0-3, controller half. service-organization-binding.spec.ts proves what
// `assertServiceOrganizationBinding` DOES; plane-service-principal.spec.ts proves
// the registry reports a `directory` binding. Neither proves that the two token
// controllers CALL it, before anything is signed, and a controller that forgot
// (or called it after signing) passed every other spec: the org-core fake in the
// contract specs only ever holds the fixture organization, so the refusal path
// was never driven through a controller. These cases run the REAL controllers
// over the REAL registry loader, the REAL resolver and the REAL cache; only
// org-core (over HTTP) and the NATS audit publisher are faked.
const ORG_CORE_TOKEN = 'controller-spec-org-core-service-token-0123456789';
const CREDENTIAL = 'test-only-directory-worker-key';
const issuedAt = '2026-07-15T00:00:00.000Z';
const mintedToken = [
  'header',
  Buffer.from(
    JSON.stringify({ iat: Math.floor(Date.parse(issuedAt) / 1000) }),
  ).toString('base64url'),
  'signature',
].join('.');

type OrgCore = {
  up: boolean;
  organizations: Record<string, { zdrIntent: boolean; entitled: boolean }>;
  requests: string[];
};

let orgCore: OrgCore;

function respond(status: number, body: unknown): Promise<Response> {
  return Promise.resolve({
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as unknown as Response);
}

function installOrgCore(): void {
  jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      orgCore.requests.push(url.pathname);
      if (!orgCore.up) return Promise.reject(new TypeError('fetch failed'));
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== ORG_CORE_TOKEN
      ) {
        return respond(401, { error: 'unauthorized' });
      }
      const match = /^\/internal\/orgs\/([^/]+)\/retention-posture$/.exec(
        url.pathname,
      );
      const found = match
        ? orgCore.organizations[decodeURIComponent(match[1])]
        : undefined;
      return found
        ? respond(200, found)
        : respond(404, { error: 'organization not found' });
    });
}

function registry(retention: 'persistent' | 'tenant'): string {
  return JSON.stringify({
    worker: {
      credential: CREDENTIAL,
      audiences: ['data-plane', 'model-gateway'],
      // `global` is a pseudo-organization no directory holds: fixed, no lookup.
      orgIds: ['global'],
      tenancy: 'directory',
      scopes: ['documents:write', 'runs:submit'],
      scopesByAudience: {
        'data-plane': ['documents:write'],
        'model-gateway': ['runs:submit'],
      },
      retentionByAudience: {
        'data-plane': retention,
        'model-gateway': retention,
      },
    },
  });
}

const originalEnv = { ...process.env };

beforeEach(() => {
  process.env.ORG_SERVICE_URL = 'http://org-core.controller-spec.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_TOKEN;
  process.env.PLANE_SERVICE_PRINCIPALS_JSON = registry('persistent');
  jest.spyOn(Logger.prototype, 'error').mockImplementation();
  jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  resetInteractiveRetentionCacheForTests();
  orgCore = {
    up: true,
    organizations: {
      'org-plain': { zdrIntent: false, entitled: true },
      'org-zdr': { zdrIntent: true, entitled: true },
    },
    requests: [],
  };
  installOrgCore();
});

afterEach(() => {
  jest.restoreAllMocks();
  process.env = { ...originalEnv };
});

/** The audit event the controller published, read without matchers that return any. */
function publishedEvent(audit: jest.Mock): {
  org_id: string;
  details: { org_binding: string };
} {
  const call = audit.mock.calls[0] as unknown[];
  return call[1] as { org_id: string; details: { org_binding: string } };
}

/** The retention posture the controller handed the token signer. */
function signedPosture(issue: jest.Mock): { zdr: boolean; source?: string } {
  const call = issue.mock.calls[0] as unknown[];
  const claims = call[call.length - 1] as {
    retentionPosture: { zdr: boolean; source?: string };
  };
  return claims.retentionPosture;
}

type Mint = (
  org: string,
  credential?: string,
) => Promise<unknown> & { __issue: jest.Mock; __audit: jest.Mock };

function plane(): Mint {
  return (org, credential = CREDENTIAL) => {
    const issue = jest.fn().mockReturnValue({ token: mintedToken });
    const audit = jest.fn().mockResolvedValue({ stream: 's', seq: 1 });
    const controller = new PlaneTokenController(
      {
        isKnownPlaneAudience: jest.fn().mockReturnValue(true),
        issuePlaneToken: issue,
      } as never,
      { publishAuditDurable: audit } as never,
    );
    return Object.assign(
      controller.issueInternalToken('data-plane', 'worker', credential, {
        orgId: org,
        scopes: ['documents:write'],
        reason: 'directory binding controller spec',
      }),
      { __issue: issue, __audit: audit },
    );
  };
}

function model(): Mint {
  return (org, credential = CREDENTIAL) => {
    const issue = jest.fn().mockReturnValue({ token: mintedToken });
    const audit = jest.fn().mockResolvedValue({ stream: 's', seq: 1 });
    const controller = new ModelPlaneTokenController(
      { issueModelPlaneToken: issue } as never,
      { publishAuditDurable: audit } as never,
    );
    return Object.assign(
      controller.issueInternalToken('worker', credential, {
        orgId: org,
        scopes: ['runs:submit'],
        reason: 'directory binding controller spec',
      }),
      { __issue: issue, __audit: audit },
    );
  };
}

describe.each([
  ['PlaneTokenController.issueInternalToken', plane],
  ['ModelPlaneTokenController.issueInternalToken', model],
] as const)('%s with a directory-bound principal', (_name, makeMint) => {
  const mint = makeMint();

  it('signs and audits for an organization org-core holds, recording how the organization was bound', async () => {
    const call = mint('org-plain');
    await expect(call).resolves.toEqual({ token: mintedToken });
    expect(call.__issue).toHaveBeenCalledTimes(1);
    expect(publishedEvent(call.__audit)).toMatchObject({
      org_id: 'org-plain',
      details: { org_binding: 'directory' },
    });
    expect(orgCore.requests).toEqual([
      '/internal/orgs/org-plain/retention-posture',
    ]);
  });

  it('answers 403, signs nothing and audits nothing for an organization org-core does not hold, and says nothing about which exist', async () => {
    const call = mint('org-fabricated');
    const failure = await call.catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ForbiddenException);
    expect((failure as ForbiddenException).message).toBe(
      'Service principal is not authorized',
    );
    expect(
      JSON.stringify((failure as ForbiddenException).getResponse()),
    ).not.toContain('org-fabricated');
    expect(call.__issue).not.toHaveBeenCalled();
    expect(call.__audit).not.toHaveBeenCalled();
  });

  it('answers 503 organization_directory_unavailable, signs nothing and audits nothing while org-core cannot be read', async () => {
    orgCore.up = false;
    const call = mint('org-plain');
    const failure = await call.catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ServiceUnavailableException);
    expect(
      (failure as ServiceUnavailableException).getResponse(),
    ).toMatchObject({
      statusCode: 503,
      error: 'organization_directory_unavailable',
    });
    expect(call.__issue).not.toHaveBeenCalled();
    expect(call.__audit).not.toHaveBeenCalled();
  });

  it('mints for a fixed pseudo-organization without asking org-core, even while it is down', async () => {
    orgCore.up = false;
    const call = mint('global');
    await expect(call).resolves.toEqual({ token: mintedToken });
    expect(publishedEvent(call.__audit).details.org_binding).toBe('fixed');
    expect(orgCore.requests).toEqual([]);
  });

  it('does not let a caller without the credential make Auth Core query org-core', async () => {
    const call = mint('org-plain', 'not-the-credential-0123456789');
    await expect(call).rejects.toBeInstanceOf(ForbiddenException);
    expect(call.__issue).not.toHaveBeenCalled();
    expect(orgCore.requests).toEqual([]);
  });

  it('never hands a tenant principal a posture for an organization the directory does not hold (org-core answers 404 there, which the posture resolver alone would read as "not ZDR")', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = registry('tenant');
    const call = mint('org-fabricated');
    await expect(call).rejects.toBeInstanceOf(ForbiddenException);
    expect(call.__issue).not.toHaveBeenCalled();
    expect(call.__audit).not.toHaveBeenCalled();
  });

  it('still stamps a tenant principal with the ORG posture once the directory vouches for it, from one org-core read', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = registry('tenant');
    const call = mint('org-zdr');
    await expect(call).resolves.toEqual({ token: mintedToken });
    expect(signedPosture(call.__issue)).toMatchObject({
      zdr: true,
      source: 'tenant-policy',
    });
    expect(orgCore.requests).toHaveLength(1);
  });
});
