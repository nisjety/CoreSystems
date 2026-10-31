import {
  BadRequestException,
  ForbiddenException,
  HttpException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';

const mockGetSession = jest.fn();
const mockVerifyPassword = jest.fn();
const mockAuthContext = Promise.resolve({
  password: { verify: mockVerifyPassword },
});
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession }, $context: mockAuthContext },
}));
const mockRedisGet = jest.fn();
const mockRedisSet = jest.fn();
const mockRedisIncrement = jest.fn();
jest.mock('../db/redis', () => ({
  redisSecondaryStorage: {
    get: mockRedisGet,
    set: mockRedisSet,
    increment: mockRedisIncrement,
  },
}));
const mockCredentialLookup = jest.fn();
jest.mock('../db', () => ({
  db: {
    select: () => ({
      from: () => ({
        where: () => ({ limit: mockCredentialLookup }),
      }),
    }),
  },
}));
import { readFreshStepUp, StepUpController } from './step-up.controller';

const NOW_SECONDS = 1_800_000_000;
const NOW_MS = NOW_SECONDS * 1000;

function requestWithCookie() {
  return { headers: { cookie: 'session=test' } } as never;
}

function signedIn() {
  mockGetSession.mockResolvedValue({
    user: { id: 'user-a' },
    session: { id: 'session-a' },
  });
}

describe('StepUpController', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    jest.spyOn(Date, 'now').mockReturnValue(NOW_MS);
    mockRedisIncrement.mockResolvedValue(1);
    mockRedisSet.mockResolvedValue(undefined);
    mockCredentialLookup.mockResolvedValue([{ password: 'stored-hash' }]);
    mockVerifyPassword.mockResolvedValue(true);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('needs an authenticated session before it checks anything', async () => {
    mockGetSession.mockResolvedValue(null);
    const controller = new StepUpController();

    await expect(
      controller.stepUp(requestWithCookie(), { password: 'right' }),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    expect(mockRedisIncrement).not.toHaveBeenCalled();
    expect(mockVerifyPassword).not.toHaveBeenCalled();
    expect(mockRedisSet).not.toHaveBeenCalled();
  });

  it('refuses a missing, empty or oversized password before an attempt is counted', async () => {
    signedIn();
    const controller = new StepUpController();

    await expect(
      controller.stepUp(requestWithCookie(), {}),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      controller.stepUp(requestWithCookie(), { password: '' }),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      controller.stepUp(requestWithCookie(), { password: 'x'.repeat(1025) }),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(mockRedisIncrement).not.toHaveBeenCalled();
  });

  it('rate-limits password attempts per user and never keys them by the raw user id', async () => {
    signedIn();
    mockRedisIncrement.mockResolvedValue(6);
    const controller = new StepUpController();

    const error = await controller
      .stepUp(requestWithCookie(), { password: 'right' })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(HttpException);
    expect((error as HttpException).getStatus()).toBe(429);
    expect(mockVerifyPassword).not.toHaveBeenCalled();
    const [key, windowSeconds] = mockRedisIncrement.mock.calls[0] as [
      string,
      number,
    ];
    expect(key).not.toContain('user-a');
    expect(windowSeconds).toBe(900);
  });

  it('refuses an account with no password to confirm, naming the method it lacks', async () => {
    signedIn();
    mockCredentialLookup.mockResolvedValue([]);
    const controller = new StepUpController();

    const error = await controller
      .stepUp(requestWithCookie(), { password: 'right' })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(ForbiddenException);
    expect((error as ForbiddenException).getResponse()).toMatchObject({
      code: 'STEP_UP_METHOD_UNAVAILABLE',
      details: { method: 'password' },
    });
    expect(mockRedisSet).not.toHaveBeenCalled();
  });

  it('records no proof when the password is wrong', async () => {
    signedIn();
    mockVerifyPassword.mockResolvedValue(false);
    const controller = new StepUpController();

    const error = await controller
      .stepUp(requestWithCookie(), { password: 'wrong' })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(UnauthorizedException);
    expect((error as UnauthorizedException).getResponse()).toMatchObject({
      code: 'INVALID_PASSWORD',
    });
    expect(mockRedisSet).not.toHaveBeenCalled();
  });

  it('records a 120-second proof for the session when the password is right', async () => {
    signedIn();
    const controller = new StepUpController();

    const result = await controller.stepUp(requestWithCookie(), {
      password: 'right',
    });

    expect(mockVerifyPassword).toHaveBeenCalledWith({
      hash: 'stored-hash',
      password: 'right',
    });
    expect(mockRedisSet).toHaveBeenCalledWith(
      'model-plane:step-up:session-a',
      JSON.stringify({ at: NOW_SECONDS, method: 'password' }),
      120,
    );
    expect(result).toEqual({
      data: {
        method: 'password',
        stepUpAt: new Date(NOW_MS).toISOString(),
        expiresAt: new Date(NOW_MS + 120_000).toISOString(),
        expiresInSeconds: 120,
      },
    });
  });

  it('answers unavailable, never success, when the proof cannot be recorded', async () => {
    signedIn();
    mockRedisSet.mockRejectedValue(new Error('store down'));
    const controller = new StepUpController();

    const error = await controller
      .stepUp(requestWithCookie(), { password: 'right' })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(ServiceUnavailableException);
    expect((error as ServiceUnavailableException).getResponse()).toMatchObject({
      code: 'STEP_UP_UNAVAILABLE',
    });
  });
});

describe('readFreshStepUp', () => {
  const proof = (at: number) => JSON.stringify({ at, method: 'password' });

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('returns the proof only while it is within its 120-second validity', async () => {
    mockRedisGet.mockResolvedValueOnce(proof(NOW_SECONDS - 120));
    await expect(readFreshStepUp('session-a', NOW_SECONDS)).resolves.toEqual({
      at: NOW_SECONDS - 120,
      method: 'password',
    });

    mockRedisGet.mockResolvedValueOnce(proof(NOW_SECONDS - 121));
    await expect(
      readFreshStepUp('session-a', NOW_SECONDS),
    ).resolves.toBeUndefined();

    mockRedisGet.mockResolvedValueOnce(null);
    await expect(
      readFreshStepUp('session-a', NOW_SECONDS),
    ).resolves.toBeUndefined();
  });

  it('never reads a proof it cannot trust, and reads nothing without a session', async () => {
    mockRedisGet.mockResolvedValueOnce('not json');
    await expect(
      readFreshStepUp('session-a', NOW_SECONDS),
    ).resolves.toBeUndefined();

    mockRedisGet.mockResolvedValueOnce(
      JSON.stringify({ at: NOW_SECONDS, method: 'totp' }),
    );
    await expect(
      readFreshStepUp('session-a', NOW_SECONDS),
    ).resolves.toBeUndefined();

    mockRedisGet.mockRejectedValueOnce(new Error('store down'));
    await expect(
      readFreshStepUp('session-a', NOW_SECONDS),
    ).resolves.toBeUndefined();

    await expect(
      readFreshStepUp(undefined, NOW_SECONDS),
    ).resolves.toBeUndefined();
    expect(mockRedisGet).toHaveBeenCalledTimes(3);
  });
});
