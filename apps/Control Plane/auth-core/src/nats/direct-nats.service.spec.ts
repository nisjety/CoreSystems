import type { JetStreamClient, NatsConnection } from 'nats';

import { DirectNatsService } from './direct-nats.service';

describe('DirectNatsService durable audit publishing', () => {
  beforeAll(() => {
    process.env.NODE_ENV = 'test';
    process.env.AUTH_INTERNAL_SERVICE_CREDENTIALS = JSON.stringify([
      {
        credentialId: 'user-core-test-suite',
        principal: 'user-core',
        audience: 'auth-core-internal',
        token: 'unit-suite-user-core-0123456789abcdef0123456789abcdef',
        scopes: ['nats:authenticate'],
      },
    ]);
  });

  afterAll(() => {
    delete process.env.AUTH_INTERNAL_SERVICE_CREDENTIALS;
  });
  const subject =
    'verevon.audit.v2.control.auth-core.plane_service_token_issued';
  const payload = {
    occurred_at: '2026-07-15T00:00:00.000Z',
    event_id: 'plane-token:stable-token-artifact',
    org_id: 'org-a',
    plane: 'control',
    producer: 'auth-core',
    event: 'plane_service_token_issued',
  };

  function serviceWith(
    publish: jest.Mock,
    connectionClosed = false,
  ): DirectNatsService {
    const service = new DirectNatsService();
    Object.assign(service, {
      nc: {
        isClosed: jest.fn().mockReturnValue(connectionClosed),
      } as unknown as NatsConnection,
      jetStream: { publish } as unknown as JetStreamClient,
    });
    return service;
  }

  it('resolves only after JetStream returns a valid PubAck', async () => {
    const publish = jest.fn().mockResolvedValue({
      stream: 'VEREVON_CONTROL_OBSERVABILITY',
      seq: 17,
      duplicate: false,
    });
    const service = serviceWith(publish);

    await expect(
      service.publishAuditDurable(subject, payload),
    ).resolves.toEqual({
      stream: 'VEREVON_CONTROL_OBSERVABILITY',
      seq: 17,
    });
    expect(publish).toHaveBeenCalledWith(subject, expect.any(Uint8Array), {
      msgID: 'plane-token:stable-token-artifact',
    });
  });

  it.each([
    ['missing connection', null, null],
    ['closed connection', {}, null],
    ['missing JetStream client', {}, undefined],
  ])('rejects when %s prevents a PubAck', async (_case, nc, jetStream) => {
    const service = new DirectNatsService();
    Object.assign(service, {
      nc:
        nc === null
          ? undefined
          : ({
              isClosed: jest
                .fn()
                .mockReturnValue(_case === 'closed connection'),
            } as unknown as NatsConnection),
      jetStream,
    });

    await expect(service.publishAuditDurable(subject, payload)).rejects.toThrow(
      'Durable audit transport unavailable',
    );
  });

  it('propagates a JetStream PubAck timeout', async () => {
    const service = serviceWith(
      jest.fn().mockRejectedValue(new Error('PubAck timeout')),
    );

    await expect(service.publishAuditDurable(subject, payload)).rejects.toThrow(
      'PubAck timeout',
    );
  });

  it.each([
    undefined,
    { stream: '', seq: 1, duplicate: false },
    { stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 0, duplicate: false },
  ])('rejects an invalid JetStream PubAck: %p', async (ack) => {
    const service = serviceWith(jest.fn().mockResolvedValue(ack));

    await expect(service.publishAuditDurable(subject, payload)).rejects.toThrow(
      'Invalid durable audit PubAck',
    );
  });

  it('rejects non-audit subjects at the boundary', async () => {
    const publish = jest.fn();
    const service = serviceWith(publish);

    await expect(
      service.publishAuditDurable('service.authenticate', payload),
    ).rejects.toThrow('Invalid durable audit subject');
    expect(publish).not.toHaveBeenCalled();
  });

  it.each([
    [
      'legacy subject',
      'verevon.audit.v1.control.plane_service_token_issued',
      payload,
    ],
    [
      'wrong producer subject',
      'verevon.audit.v2.control.billing-core.plane_service_token_issued',
      payload,
    ],
    [subject, subject, { ...payload, event_id: '' }],
    [subject, subject, { ...payload, occurred_at: '' }],
    [subject, subject, { ...payload, occurred_at: '2026-07-15' }],
    [subject, subject, { ...payload, org_id: null }],
    [subject, subject, { ...payload, producer: 'billing-core' }],
    [subject, subject, { ...payload, event: 'different_event' }],
  ])(
    'rejects invalid audit identity: %s',
    async (_case, invalidSubject, invalidPayload) => {
      const publish = jest.fn();
      const service = serviceWith(publish);

      await expect(
        service.publishAuditDurable(invalidSubject, invalidPayload),
      ).rejects.toThrow('Invalid durable audit');
      expect(publish).not.toHaveBeenCalled();
    },
  );
});
