import { Logger, ServiceUnavailableException } from '@nestjs/common';

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
import {
  RETENTION_POSTURE_FRESH_TTL_MS,
  resetInteractiveRetentionCacheForTests,
} from './interactive-retention-policy';

/**
 * The interactive token endpoints with the REAL retention resolver (only
 * org-core's HTTP answer and the session/membership lookups are faked). What
 * an org-core outage does to a mint is decided by the resolver and these two
 * controllers together, and a spec that mocks the resolver (as the controllers'
 * own specs do) cannot see it: the old "fails closed, never blocks login"
 * behaviour was a `zdr:false` token for every org, which is the defect.
 *
 * Contract: while org-core cannot be read, an org whose posture is not
 * remembered gets a retryable 503 `retention_posture_unavailable` and NO token;
 * an org last known to be ZDR keeps getting zdr:true tokens.
 */
describe('interactive token endpoints when org-core cannot be read', () => {
  const originalEnv = { ...process.env };
  const mintedFor = (zdr: boolean) => ({ token: `minted-zdr-${zdr}` });
  let orgCoreUp: boolean;
  let orgCorePosture: { zdrIntent: boolean; entitled: boolean };

  beforeEach(() => {
    jest.useFakeTimers({ now: new Date('2026-10-03T10:00:00.000Z') });
    resetInteractiveRetentionCacheForTests();
    jest.spyOn(Logger.prototype, 'error').mockImplementation();
    jest.spyOn(Logger.prototype, 'warn').mockImplementation();
    process.env.ORG_SERVICE_URL = 'http://org-core.spec.test:8080';
    process.env.ORG_CORE_SERVICE_TOKEN =
      'controller-spec-service-token-0123456789abcdef';
    orgCoreUp = true;
    orgCorePosture = { zdrIntent: true, entitled: true };
    jest.spyOn(global, 'fetch').mockImplementation(() => {
      if (!orgCoreUp) return Promise.reject(new Error('ECONNREFUSED'));
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.resolve(orgCorePosture),
      } as never);
    });
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a', email: 'user-a@example.test' },
      session: { activeOrganizationId: 'org-a' },
    });
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId: 'org-a',
      role: 'member',
    });
  });

  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
    process.env = { ...originalEnv };
  });

  type Harness = {
    /** Mint one interactive token through the real controller. */
    mint: () => Promise<unknown>;
    /** The token service method the controller would have called to sign. */
    issue: jest.Mock;
  };

  function planeEndpoint(): Harness {
    const issue = jest
      .fn()
      .mockImplementation(
        (_audience: string, claims: { retentionPosture: { zdr: boolean } }) =>
          mintedFor(claims.retentionPosture.zdr),
      );
    const tokens = {
      isInteractivePlaneAudience: jest.fn().mockReturnValue(true),
      issuePlaneToken: issue,
    };
    const controller = new PlaneTokenController(tokens as never, {} as never);
    return {
      issue,
      mint: () =>
        controller.getToken('inference-core', { headers: {} } as never),
    };
  }

  function modelPlaneEndpoint(): Harness {
    const issue = jest
      .fn()
      .mockImplementation((claims: { retentionPosture: { zdr: boolean } }) =>
        mintedFor(claims.retentionPosture.zdr),
      );
    const controller = new ModelPlaneTokenController(
      { issueModelPlaneToken: issue } as never,
      {} as never,
    );
    return { issue, mint: () => controller.getToken({ headers: {} } as never) };
  }

  const endpoints: Array<[string, () => Harness]> = [
    ['plane token', planeEndpoint],
    ['model-plane token', modelPlaneEndpoint],
  ];

  describe.each(endpoints)('%s endpoint', (_name, build) => {
    it('answers 503 retention_posture_unavailable and issues no token for an org it has never resolved', async () => {
      orgCoreUp = false;
      const { mint, issue } = build();

      const attempt = mint();

      await expect(attempt).rejects.toBeInstanceOf(ServiceUnavailableException);
      await expect(attempt).rejects.toMatchObject({
        response: { statusCode: 503, error: 'retention_posture_unavailable' },
      });
      expect(issue).not.toHaveBeenCalled();
    });

    it('keeps minting zdr:true tokens for an org last known to be ZDR while org-core is down', async () => {
      const { mint, issue } = build();
      await expect(mint()).resolves.toMatchObject(mintedFor(true));

      orgCoreUp = false;
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);

      await expect(mint()).resolves.toMatchObject(mintedFor(true));
      expect(issue).toHaveBeenCalledTimes(2);
    });

    it('mints again as soon as org-core is back, with whatever it now reports', async () => {
      orgCoreUp = false;
      const { mint } = build();
      await expect(mint()).rejects.toBeInstanceOf(ServiceUnavailableException);

      orgCoreUp = true;
      orgCorePosture = { zdrIntent: false, entitled: true };

      await expect(mint()).resolves.toMatchObject(mintedFor(false));
    });
  });
});
