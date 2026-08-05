import {
  publishVerevonAuditDurable,
  setAuditNatsPublisher,
} from './audit-plugin';

jest.mock('better-auth/api', () => ({
  createAuthMiddleware: (handler: unknown) => handler,
}));

describe('Better Auth audit durability', () => {
  afterEach(() => setAuditNatsPublisher(null));

  it('awaits the durable publisher for security audit events', async () => {
    const publishAuditDurable = jest
      .fn()
      .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 9 });
    setAuditNatsPublisher({ publishAuditDurable });

    await publishVerevonAuditDurable({
      occurred_at: '2026-07-15T00:00:00.000Z',
      event_id: 'session:session-1:sign_in',
      org_id: 'org-1',
      user_id: 'user-1',
      plane: 'control',
      event: 'sign_in',
      outcome: 'ok',
    });

    expect(publishAuditDurable).toHaveBeenCalledWith(
      'verevon.audit.v2.control.auth-core.sign_in',
      expect.objectContaining({
        event_id: 'session:session-1:sign_in',
        org_id: 'org-1',
        producer: 'auth-core',
        event: 'sign_in',
      }),
    );
  });

  it('propagates a durable publish failure', async () => {
    setAuditNatsPublisher({
      publishAuditDurable: jest.fn().mockRejectedValue(new Error('no PubAck')),
    });

    await expect(
      publishVerevonAuditDurable({
        occurred_at: '2026-07-15T00:00:00.000Z',
        event_id: 'twofa:mutation-1:enable',
        org_id: 'org-1',
        plane: 'control',
        event: 'twofa_enable',
        outcome: 'ok',
      }),
    ).rejects.toThrow('no PubAck');
  });

  it('fails closed when the durable publisher is unavailable', async () => {
    await expect(
      publishVerevonAuditDurable({
        occurred_at: '2026-07-15T00:00:00.000Z',
        event_id: 'session:session-1:sign_in',
        org_id: 'org-1',
        plane: 'control',
        event: 'sign_in',
        outcome: 'ok',
      }),
    ).rejects.toThrow('Durable audit transport unavailable');
  });
});
