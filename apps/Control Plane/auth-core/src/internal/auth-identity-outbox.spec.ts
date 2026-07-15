import { sqlClient } from '../db';
import {
  parseIdentityOutboxRow,
  type IdentityOutboxRow,
} from './auth-identity-outbox';
import { AuthIntegrationService } from './auth-integration.service';

jest.mock('../db', () => ({ sqlClient: jest.fn() }));

type SqlTemplate = readonly string[] & { raw?: readonly string[] };
type SqlMock = jest.Mock & { begin: jest.Mock };

describe('AuthIntegrationService identity outbox', () => {
  const sqlMock = sqlClient as unknown as SqlMock;

  beforeEach(() => {
    sqlMock.mockReset();
    sqlMock.begin = jest.fn(
      async (callback: (tx: typeof sqlMock) => Promise<unknown>) =>
        callback(sqlMock),
    );
  });

  it('retries a failed PubAck with the same stable Msg-Id and acknowledges by exact event id', async () => {
    const row = {
      event_id: 'user:user-1:registered',
      event_type: 'user_registered',
      user_id: 'user-1',
      payload: {
        userId: 'user-1',
        email: 'user-1@example.invalid',
        name: 'User One',
        provider: 'microsoft',
        emailVerified: true,
      },
      attempts: 0,
    };
    const executed: string[] = [];
    sqlMock.mockImplementation((template: SqlTemplate) => {
      const statement = template.join('');
      executed.push(statement);
      if (statement.includes('RETURNING o.event_id'))
        return Promise.resolve([row]);
      if (
        statement.includes('FROM auth_identity_event_outbox') &&
        statement.includes('FOR UPDATE')
      ) {
        return Promise.resolve([row]);
      }
      if (statement.includes('RETURNING event_id')) {
        return Promise.resolve([{ event_id: row.event_id }]);
      }
      return Promise.resolve([]);
    });
    const publisher = {
      publishUserRegistered: jest
        .fn()
        .mockRejectedValueOnce(new Error('missing PubAck'))
        .mockResolvedValueOnce(undefined),
      publishUserProviderLinked: jest.fn(),
    };
    const service = new AuthIntegrationService(
      {} as never,
      {} as never,
      publisher as never,
    );

    await expect(service.flushIdentityEventOutbox()).resolves.toBe(0);
    await expect(service.flushIdentityEventOutbox()).resolves.toBe(1);

    expect(publisher.publishUserRegistered).toHaveBeenCalledTimes(2);
    expect(publisher.publishUserRegistered).toHaveBeenNthCalledWith(
      1,
      expect.any(Object),
      undefined,
      row.event_id,
    );
    expect(publisher.publishUserRegistered).toHaveBeenNthCalledWith(
      2,
      expect.any(Object),
      undefined,
      row.event_id,
    );
    expect(executed.join('\n')).toMatch(
      /WHERE event_id = [\s\S]*AND published_at IS NULL[\s\S]*RETURNING event_id/,
    );
  });

  it('blocks provider delivery until registration is published and exposes max-attempt dead letters', async () => {
    const row = {
      event_id: 'account:account-1:provider_linked',
      event_type: 'provider_linked',
      user_id: 'user-1',
      payload: {
        userId: 'user-1',
        email: 'user-1@example.invalid',
        provider: 'microsoft',
        providerAccountId: 'external-1',
        scopesGranted: ['openid', 'profile', 'email'],
      },
      attempts: 19,
    };
    const executed: string[] = [];
    sqlMock.mockImplementation((template: SqlTemplate) => {
      const statement = template.join('');
      executed.push(statement);
      if (statement.includes('RETURNING o.event_id'))
        return Promise.resolve([row]);
      if (
        statement.includes('FROM auth_identity_event_outbox') &&
        statement.includes('FOR UPDATE')
      ) {
        return Promise.resolve([row]);
      }
      return Promise.resolve([]);
    });
    const publisher = {
      publishUserRegistered: jest.fn(),
      publishUserProviderLinked: jest
        .fn()
        .mockRejectedValue(new Error('broker unavailable')),
    };
    const service = new AuthIntegrationService(
      {} as never,
      {} as never,
      publisher as never,
    );

    await expect(service.flushIdentityEventOutbox()).resolves.toBe(0);

    const statements = executed.join('\n');
    expect(statements).toMatch(
      /event_type <> 'provider_linked'[\s\S]*dependency\.event_type = 'user_registered'[\s\S]*dependency\.published_at IS NOT NULL/,
    );
    expect(statements).toContain('WHEN attempts + 1 >= 20 THEN NOW()');
    expect(publisher.publishUserProviderLinked).toHaveBeenCalledWith(
      expect.any(Object),
      undefined,
      row.event_id,
    );
  });

  it('rejects malformed persisted payloads without publishing and advances them toward the visible dead letter', async () => {
    const row = {
      event_id: 'user:user-1:registered',
      event_type: 'user_registered',
      user_id: 'user-1',
      payload: { email: 'missing-user-id@example.invalid', provider: 'email' },
      attempts: 19,
    };
    const executed: string[] = [];
    sqlMock.mockImplementation((template: SqlTemplate) => {
      const statement = template.join('');
      executed.push(statement);
      if (statement.includes('RETURNING o.event_id'))
        return Promise.resolve([row]);
      if (
        statement.includes('FROM auth_identity_event_outbox') &&
        statement.includes('FOR UPDATE')
      ) {
        return Promise.resolve([row]);
      }
      return Promise.resolve([]);
    });
    const publisher = {
      publishUserRegistered: jest.fn(),
      publishUserProviderLinked: jest.fn(),
    };
    const service = new AuthIntegrationService(
      {} as never,
      {} as never,
      publisher as never,
    );

    await expect(service.flushIdentityEventOutbox()).resolves.toBe(0);

    expect(publisher.publishUserRegistered).not.toHaveBeenCalled();
    expect(publisher.publishUserProviderLinked).not.toHaveBeenCalled();
    expect(executed.join('\n')).toContain('WHEN attempts + 1 >= 20 THEN NOW()');
  });

  it.each([
    ['null payload', null],
    ['array payload', []],
    ['missing user id', { email: 'user-1@example.invalid', provider: 'email' }],
    [
      'blank user id',
      { userId: ' ', email: 'user-1@example.invalid', provider: 'email' },
    ],
    [
      'mismatched user id',
      {
        userId: 'user-2',
        email: 'user-1@example.invalid',
        provider: 'email',
        emailVerified: true,
      },
    ],
    [
      'invalid email',
      {
        userId: 'user-1',
        email: 'not-an-email',
        provider: 'email',
        emailVerified: true,
      },
    ],
  ])('rejects %s at the durable payload boundary', (_name, payload) => {
    expect(() =>
      parseIdentityOutboxRow({
        event_id: 'user:user-1:registered',
        event_type: 'user_registered',
        user_id: 'user-1',
        payload,
        attempts: 0,
      }),
    ).toThrow('identity outbox');
  });

  it('rejects event-type-specific malformed fields and accepts absent optional scopes', () => {
    const registration = {
      event_id: 'user:user-1:registered',
      event_type: 'user_registered',
      user_id: 'user-1',
      payload: {
        userId: 'user-1',
        email: 'user-1@example.invalid',
        provider: 'email',
        emailVerified: true,
      },
      attempts: 0,
    } satisfies IdentityOutboxRow;
    expect(() =>
      parseIdentityOutboxRow({ ...registration, event_id: 'wrong' }),
    ).toThrow('registration event id');
    expect(() =>
      parseIdentityOutboxRow({
        ...registration,
        payload: { ...registration.payload, emailVerified: 'yes' },
      }),
    ).toThrow('emailVerified');

    const provider = {
      event_id: 'account:account-1:provider_linked',
      event_type: 'provider_linked',
      user_id: 'user-1',
      payload: {
        userId: 'user-1',
        email: 'user-1@example.invalid',
        provider: 'microsoft',
        providerAccountId: 'external-1',
      },
      attempts: 0,
    } satisfies IdentityOutboxRow;
    expect(parseIdentityOutboxRow(provider)).toMatchObject({
      eventType: 'provider_linked',
      data: { scopesGranted: undefined },
    });
    expect(() =>
      parseIdentityOutboxRow({ ...provider, event_id: 'wrong' }),
    ).toThrow('provider event id');
    expect(() =>
      parseIdentityOutboxRow({
        ...provider,
        payload: { ...provider.payload, providerAccountId: undefined },
      }),
    ).toThrow('providerAccountId');
    expect(() =>
      parseIdentityOutboxRow({
        ...provider,
        payload: { ...provider.payload, scopesGranted: 'openid' },
      }),
    ).toThrow('scopesGranted');
    expect(() =>
      parseIdentityOutboxRow({
        ...provider,
        payload: { ...provider.payload, scopesGranted: [' '] },
      }),
    ).toThrow('scope');
  });
});
