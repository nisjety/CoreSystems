import { AuthEventPublisher } from './auth-event.publisher';

describe('AuthEventPublisher Control audit routing', () => {
  it('publishes canonical organization and membership revisions with stable message ids', async () => {
    const publish = jest.fn().mockResolvedValue(undefined);
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      { publish } as never,
    );

    await publisher.publishOrganizationProjection(
      {
        organizationId: 'org-1',
        name: 'Org One',
        slug: 'org-one',
        ownerUserId: 'owner-1',
        metadata: { region: 'eu' },
        revision: 7,
      },
      'organization:org-1:7:upsert',
    );
    await publisher.publishOrganizationMembershipProjection(
      {
        organizationId: 'org-1',
        userId: 'user-1',
        role: 'admin',
        action: 'upsert',
        revision: 9,
        organizationRevision: 7,
        userEmail: 'user-1@example.invalid',
      },
      'organization:org-1:member:user-1:9:upsert',
    );
    await publisher.publishOrganizationDeletionProjection(
      { organizationId: 'org-1', revision: 10 },
      'organization:org-1:10:deleted',
    );

    expect(publish.mock.calls).toEqual([
      [
        'aqencia.controlplane.org.changed',
        {
          schema_version: 1,
          event_id: 'organization:org-1:7:upsert',
          action: 'upsert',
          org_id: 'org-1',
          name: 'Org One',
          slug: 'org-one',
          owner_user_id: 'owner-1',
          metadata: { region: 'eu' },
          revision: 7,
        },
        { msgID: 'organization:org-1:7:upsert' },
      ],
      [
        'aqencia.controlplane.org.member_changed',
        {
          schema_version: 1,
          event_id: 'organization:org-1:member:user-1:9:upsert',
          action: 'upsert',
          org_id: 'org-1',
          user_id: 'user-1',
          role: 'admin',
          user_email: 'user-1@example.invalid',
          revision: 9,
          organization_revision: 7,
        },
        { msgID: 'organization:org-1:member:user-1:9:upsert' },
      ],
      [
        'aqencia.controlplane.org.changed',
        {
          schema_version: 1,
          event_id: 'organization:org-1:10:deleted',
          action: 'remove',
          org_id: 'org-1',
          revision: 10,
        },
        { msgID: 'organization:org-1:10:deleted' },
      ],
    ]);
  });

  it('waits for a valid JetStream PubAck on the plane-local broker', async () => {
    const sharedNats = { publishPlain: jest.fn() };
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      sharedNats as never,
    );
    let publishedPayload: Uint8Array | undefined;
    const localPublish = jest.fn((_subject: string, payload: Uint8Array) => {
      publishedPayload = payload;
      return Promise.resolve({
        stream: 'VELION_CONTROL_OBSERVABILITY',
        seq: 41,
      });
    });
    Object.assign(publisher as object, {
      isEnabled: true,
      jetStream: { publish: localPublish },
    });

    await publisher.publishVelionAudit({
      occurred_at: '2026-07-15T00:00:00.000Z',
      org_id: 'org-1',
      user_id: 'user-1',
      event: 'sign_in',
      outcome: 'ok',
      event_id: 'session:session-1:sign_in',
    });

    expect(localPublish).toHaveBeenCalledTimes(1);
    expect(localPublish).toHaveBeenCalledWith(
      'velion.audit.v2.control.auth-core.sign_in',
      expect.any(Uint8Array),
      { msgID: 'session:session-1:sign_in' },
    );
    expect(
      JSON.parse(new TextDecoder().decode(publishedPayload)),
    ).toMatchObject({
      occurred_at: '2026-07-15T00:00:00.000Z',
      event_id: 'session:session-1:sign_in',
      producer: 'auth-core',
      plane: 'control',
      event: 'sign_in',
    });
    expect(sharedNats.publishPlain).not.toHaveBeenCalled();
  });

  it('fails closed when the audit transport is unavailable', async () => {
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      { publishPlain: jest.fn() } as never,
    );
    Object.assign(publisher as object, { isEnabled: true, jetStream: null });

    await expect(
      publisher.publishVelionAudit({
        occurred_at: '2026-07-15T00:00:00.000Z',
        org_id: 'org-1',
        user_id: 'user-1',
        event: 'sign_in',
        outcome: 'ok',
        event_id: 'session:session-1:sign_in',
      }),
    ).rejects.toThrow('Durable audit transport unavailable');
  });

  it('rejects an invalid JetStream PubAck', async () => {
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      { publishPlain: jest.fn() } as never,
    );
    Object.assign(publisher as object, {
      isEnabled: true,
      jetStream: {
        publish: jest.fn().mockResolvedValue({ stream: '', seq: 0 }),
      },
    });

    await expect(
      publisher.publishVelionAudit({
        occurred_at: '2026-07-15T00:00:00.000Z',
        org_id: 'org-1',
        user_id: 'user-1',
        event: 'sign_in',
        outcome: 'ok',
        event_id: 'session:session-1:sign_in',
      }),
    ).rejects.toThrow('Invalid durable audit PubAck');
  });

  it('uses the outbox identity as the JetStream de-duplication key', async () => {
    const publish = jest.fn((_subject: string, payload: Uint8Array) => {
      expect(JSON.parse(new TextDecoder().decode(payload))).toMatchObject({
        event_id: 'membership:org-1:user-1:2:role_changed',
      });
      return Promise.resolve({
        stream: 'VELION_CONTROL_OBSERVABILITY',
        seq: 42,
      });
    });
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      { publishPlain: jest.fn() } as never,
    );
    Object.assign(publisher as object, {
      isEnabled: true,
      jetStream: { publish },
    });

    await publisher.publishVelionAudit({
      occurred_at: '2026-07-15T00:00:00.000Z',
      org_id: 'org-1',
      event: 'role_change',
      outcome: 'ok',
      event_id: 'membership:org-1:user-1:2:role_changed',
    });

    expect(publish).toHaveBeenCalledWith(
      'velion.audit.v2.control.auth-core.role_change',
      expect.any(Uint8Array),
      { msgID: 'membership:org-1:user-1:2:role_changed' },
    );
  });

  it.each([
    {
      occurred_at: '2026-07-15T00:00:00.000Z',
      event_id: '',
      label: 'missing event id',
    },
    {
      occurred_at: '',
      event_id: 'session:session-1:sign_in',
      label: 'missing producer occurrence time',
    },
  ])('fails closed for $label', async ({ occurred_at, event_id }) => {
    const publish = jest.fn();
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      { publishPlain: jest.fn() } as never,
    );
    Object.assign(publisher as object, {
      isEnabled: true,
      jetStream: { publish },
    });

    await expect(
      publisher.publishVelionAudit({
        occurred_at,
        org_id: 'org-1',
        event: 'sign_in',
        outcome: 'ok',
        event_id,
      }),
    ).rejects.toThrow('Invalid durable audit identity');
    expect(publish).not.toHaveBeenCalled();
  });

  it('fails closed for durable identity events when the shared publisher is unavailable', async () => {
    const publisher = new AuthEventPublisher(
      { get: jest.fn().mockReturnValue('test') } as never,
      undefined as never,
    );

    await expect(
      publisher.publishUserRegistered(
        {
          userId: 'user-1',
          email: 'user-1@example.invalid',
          provider: 'microsoft',
          emailVerified: true,
        },
        undefined,
        'user:user-1:registered',
      ),
    ).rejects.toThrow('shared NATS publisher unavailable');

    await expect(
      publisher.publishUserProviderLinked(
        {
          userId: 'user-1',
          email: 'user-1@example.invalid',
          provider: 'microsoft',
          providerAccountId: 'external-1',
        },
        undefined,
        'account:account-1:provider_linked',
      ),
    ).rejects.toThrow('shared NATS publisher unavailable');
  });
});
