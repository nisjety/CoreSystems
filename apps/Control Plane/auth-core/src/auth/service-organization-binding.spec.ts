import { ForbiddenException, Logger } from '@nestjs/common';

import {
  assertServiceOrganizationBinding,
  OrganizationDirectoryUnavailableError,
  RETENTION_STALE_NON_ZDR_MAX_AGE_MS,
  RETENTION_STALE_ZDR_MAX_AGE_MS,
  resetInteractiveRetentionCacheForTests,
  resolveInteractiveRetentionPosture,
  resolveOrganizationDirectoryStatus,
} from './interactive-retention-policy';

// The directory half of the production tenancy model (plan W0-3): what Auth Core
// does, at mint, for a principal whose registry row says `tenancy: 'directory'`.
// org-core is faked only in the way the resolver meets it: service
// authentication on every route (no credential, no answer), the retention-posture
// route, and org-core's own 404 "organization not found". Everything else is the
// real resolver, cache and stale rules.
const ORG_CORE_TOKEN = 'directory-spec-org-core-service-token-0123456789';

type OrgCore = {
  up: boolean;
  organizations: Record<string, { zdrIntent: boolean; entitled: boolean }>;
  /** Answer every posture read with this status/body instead (a proxy, a wrong URL). */
  override?: { status: number; body: unknown };
  requests: string[];
};

let orgCore: OrgCore;
let now = 1_700_000_000_000;

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
      if (orgCore.override) {
        return respond(orgCore.override.status, orgCore.override.body);
      }
      const match = /^\/internal\/orgs\/([^/]+)\/retention-posture$/.exec(
        url.pathname,
      );
      if (!match) return respond(404, { error: 'page not found' });
      const found = orgCore.organizations[decodeURIComponent(match[1])];
      return found
        ? respond(200, found)
        : respond(404, { error: 'organization not found' });
    });
}

const originalEnv = { ...process.env };

beforeEach(() => {
  process.env.ORG_SERVICE_URL = 'http://org-core.directory-spec.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_TOKEN;
  jest.spyOn(Logger.prototype, 'error').mockImplementation();
  jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  now = 1_700_000_000_000;
  jest.spyOn(Date, 'now').mockImplementation(() => now);
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

describe('resolveOrganizationDirectoryStatus', () => {
  it('says active for an organization org-core holds, after one authenticated read', async () => {
    await expect(resolveOrganizationDirectoryStatus('org-plain')).resolves.toBe(
      'active',
    );
    expect(orgCore.requests).toEqual([
      '/internal/orgs/org-plain/retention-posture',
    ]);
  });

  it('says unknown for an organization org-core has no record of, and never remembers that answer', async () => {
    await expect(resolveOrganizationDirectoryStatus('org-new')).resolves.toBe(
      'unknown',
    );
    // The record can appear moments later (Auth projects organizations
    // asynchronously): the next ask goes back to org-core and sees it.
    orgCore.organizations['org-new'] = { zdrIntent: false, entitled: true };
    await expect(resolveOrganizationDirectoryStatus('org-new')).resolves.toBe(
      'active',
    );
    expect(orgCore.requests).toHaveLength(2);
  });

  it('answers a positive result from memory for 45 seconds, then asks again', async () => {
    await resolveOrganizationDirectoryStatus('org-plain');
    now += 44_000;
    await resolveOrganizationDirectoryStatus('org-plain');
    expect(orgCore.requests).toHaveLength(1);
    now += 2_000;
    await resolveOrganizationDirectoryStatus('org-plain');
    expect(orgCore.requests).toHaveLength(2);
  });

  it('shares one read between concurrent asks for one organization, and with the posture', async () => {
    await Promise.all([
      resolveOrganizationDirectoryStatus('org-zdr'),
      resolveOrganizationDirectoryStatus('org-zdr'),
      resolveInteractiveRetentionPosture('org-zdr'),
    ]);
    expect(orgCore.requests).toHaveLength(1);
    // And the posture that read cached is the real one, not a guess.
    await expect(
      resolveInteractiveRetentionPosture('org-zdr'),
    ).resolves.toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
    expect(orgCore.requests).toHaveLength(1);
  });

  it.each(['', '../org-plain', 'org plain', 'x'.repeat(129), 'org/plain'])(
    'says unknown for an id that cannot be an organization, without asking org-core: %p',
    async (organizationId) => {
      await expect(
        resolveOrganizationDirectoryStatus(organizationId),
      ).resolves.toBe('unknown');
      expect(orgCore.requests).toEqual([]);
    },
  );

  describe('when org-core cannot answer', () => {
    it('refuses with a 503 and its own error code when nothing usable is remembered', async () => {
      orgCore.up = false;
      const failure = await resolveOrganizationDirectoryStatus(
        'org-plain',
      ).catch((error: unknown) => error);
      expect(failure).toBeInstanceOf(OrganizationDirectoryUnavailableError);
      expect(
        (failure as OrganizationDirectoryUnavailableError).getStatus(),
      ).toBe(503);
      expect(
        (failure as OrganizationDirectoryUnavailableError).getResponse(),
      ).toMatchObject({
        statusCode: 503,
        error: 'organization_directory_unavailable',
      });
      // The reason stays on the error for the log; it is not in the body.
      expect(
        JSON.stringify(
          (failure as OrganizationDirectoryUnavailableError).getResponse(),
        ),
      ).not.toContain('org_core_unreachable');
    });

    it('never reads a failure as an organization being fine, or as it being absent', async () => {
      for (const override of [
        { status: 500, body: { error: 'boom' } },
        { status: 404, body: { error: 'page not found' } },
        { status: 403, body: { error: 'forbidden' } },
        { status: 200, body: { zdrIntent: 'yes' } },
      ]) {
        resetInteractiveRetentionCacheForTests();
        orgCore.override = override;
        await expect(
          resolveOrganizationDirectoryStatus('org-plain'),
        ).rejects.toBeInstanceOf(OrganizationDirectoryUnavailableError);
      }
    });

    it('serves a remembered organization for 15 minutes when it was not ZDR, and no longer', async () => {
      await resolveOrganizationDirectoryStatus('org-plain');
      orgCore.up = false;
      now += RETENTION_STALE_NON_ZDR_MAX_AGE_MS - 1_000;
      await expect(
        resolveOrganizationDirectoryStatus('org-plain'),
      ).resolves.toBe('active');
      now += 2_000;
      await expect(
        resolveOrganizationDirectoryStatus('org-plain'),
      ).rejects.toBeInstanceOf(OrganizationDirectoryUnavailableError);
    });

    it('serves a remembered ZDR organization for 24 hours', async () => {
      await resolveOrganizationDirectoryStatus('org-zdr');
      orgCore.up = false;
      now += RETENTION_STALE_ZDR_MAX_AGE_MS - 1_000;
      await expect(resolveOrganizationDirectoryStatus('org-zdr')).resolves.toBe(
        'active',
      );
      now += 2_000;
      await expect(
        resolveOrganizationDirectoryStatus('org-zdr'),
      ).rejects.toBeInstanceOf(OrganizationDirectoryUnavailableError);
    });

    it('does not let an organization that was never seen ride on another one being remembered', async () => {
      await resolveOrganizationDirectoryStatus('org-plain');
      orgCore.up = false;
      await expect(
        resolveOrganizationDirectoryStatus('org-other'),
      ).rejects.toBeInstanceOf(OrganizationDirectoryUnavailableError);
    });
  });

  it('leaves the posture cache alone on a 404, so a known ZDR organization is not downgraded by it', async () => {
    await expect(
      resolveInteractiveRetentionPosture('org-zdr'),
    ).resolves.toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
    // Past the fresh window, org-core now says it has no record (an anomaly).
    now += 60_000;
    delete orgCore.organizations['org-zdr'];
    await expect(resolveOrganizationDirectoryStatus('org-zdr')).resolves.toBe(
      'unknown',
    );
    // The posture resolver's own rule still holds: a 404 for a known ZDR
    // organization keeps zdr:true. Had the directory check evicted the entry,
    // this would now be zdr:false.
    await expect(
      resolveInteractiveRetentionPosture('org-zdr'),
    ).resolves.toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
  });
});

describe('assertServiceOrganizationBinding', () => {
  const principal = (
    orgBinding: 'fixed' | 'directory' | 'any',
    orgId = 'org-plain',
  ) => ({ orgBinding, orgId, serviceId: 'embedding-engine' });

  it.each(['fixed', 'any'] as const)(
    'asks nobody for a %s binding',
    async (binding) => {
      orgCore.up = false;
      await expect(
        assertServiceOrganizationBinding(principal(binding, 'global')),
      ).resolves.toBeUndefined();
      expect(orgCore.requests).toEqual([]);
    },
  );

  it('lets a directory-bound principal act for an organization the directory holds', async () => {
    await expect(
      assertServiceOrganizationBinding(principal('directory')),
    ).resolves.toBeUndefined();
    expect(orgCore.requests).toHaveLength(1);
  });

  it('refuses a directory-bound principal for an organization the directory does not hold, with the generic not-authorized answer', async () => {
    const failure = await assertServiceOrganizationBinding(
      principal('directory', 'org-fabricated'),
    ).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ForbiddenException);
    // Which organizations exist is not something an error body should teach.
    expect((failure as ForbiddenException).message).toBe(
      'Service principal is not authorized',
    );
    expect(
      JSON.stringify((failure as ForbiddenException).getResponse()),
    ).not.toContain('org-fabricated');
  });

  it('propagates the 503 when org-core cannot be read: no token, never "fine"', async () => {
    orgCore.up = false;
    await expect(
      assertServiceOrganizationBinding(principal('directory')),
    ).rejects.toBeInstanceOf(OrganizationDirectoryUnavailableError);
  });

  it('refuses a mint for the pseudo-organization global when the principal does not list it, instead of treating it as an organization', async () => {
    // `global` is held by no directory: it is only ever a fixed entry.
    await expect(
      assertServiceOrganizationBinding(principal('directory', 'global')),
    ).rejects.toBeInstanceOf(ForbiddenException);
  });
});
