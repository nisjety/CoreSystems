import type { JetStreamClient, NatsConnection } from 'nats';

import { DirectNatsService } from './direct-nats.service';

describe('DirectNatsService durable audit publishing', () => {
  const subject = 'velion.audit.v1.control.plane_service_token_issued';
  const payload = { org_id: 'org-a', event: 'plane_service_token_issued' };

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
      stream: 'VELION_CONTROL_OBSERVABILITY',
      seq: 17,
      duplicate: false,
    });
    const service = serviceWith(publish);

    await expect(
      service.publishAuditDurable(subject, payload),
    ).resolves.toEqual({
      stream: 'VELION_CONTROL_OBSERVABILITY',
      seq: 17,
    });
    expect(publish).toHaveBeenCalledWith(subject, expect.any(Uint8Array));
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
    { stream: 'VELION_CONTROL_OBSERVABILITY', seq: 0, duplicate: false },
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
});
