/**
 * Organization Events Plugin
 *
 * Publishes organization lifecycle events to NATS when organizations are created,
 * members are added, or members are removed.
 */

import type { BetterAuthPlugin } from 'better-auth';
import { AuthEventPublisher } from '../internal/auth-event.publisher';
import { sqlClient } from '../db';
import {
  normalizeOutboxRevision,
  type OutboxRevision,
} from './outbox-revision';
import { fetchReconciliation } from './reconciliation-http';

let eventPublisher: AuthEventPublisher | null = null;

type OrganizationOutboxRow = {
  organization_id: string;
  name: string;
  slug: string | null;
  metadata: Record<string, unknown> | null;
  owner_user_id: string;
  revision: OutboxRevision;
};

type MembershipOutboxRow = {
  organization_id: string;
  user_id: string;
  role: string;
  desired_action: 'upsert' | 'remove';
  revision: OutboxRevision;
};

type OrganizationDeletionOutboxRow = {
  organization_id: string;
  name: string;
  billing_synced_at: Date | null;
  org_synced_at: Date | null;
};

function orgCoreConfig(): { url: string; key: string } {
  const url = (
    process.env.ORG_SERVICE_URL ||
    process.env.ORG_CORE_URL ||
    ''
  ).replace(/\/$/, '');
  const key =
    process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || '';
  if (!url || !key)
    throw new Error('Org Core reconciliation is not configured');
  return { url, key };
}

async function postOrgCore(
  path: string,
  body: unknown,
): Promise<{ applied?: boolean }> {
  const config = orgCoreConfig();
  const response = await fetchReconciliation(`${config.url}${path}`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-internal-api-key': config.key,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`Org Core reconciliation returned ${response.status}`);
  }
  return (await response.json()) as { applied?: boolean };
}

async function deactivateOrganizationBilling(
  organizationId: string,
  reason: string,
): Promise<void> {
  const url = (process.env.BILLING_CORE_URL || '').replace(/\/$/, '');
  const key =
    process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || '';
  if (!url || !key)
    throw new Error('Billing Core reconciliation is not configured');

  const response = await fetchReconciliation(
    `${url}/api/v1/billing/orgs/${encodeURIComponent(organizationId)}/deactivate`,
    {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-internal-api-key': key,
      },
      body: JSON.stringify({ reason }),
    },
  );
  if (!response.ok) {
    throw new Error(`Billing Core reconciliation returned ${response.status}`);
  }
}

async function recordOrganizationProjection(organization: {
  id: string;
  name: string;
  slug?: string | null;
  metadata?: unknown;
}): Promise<void> {
  const metadata =
    organization.metadata && typeof organization.metadata === 'object'
      ? (organization.metadata as Record<string, unknown>)
      : {};
  const jsonMetadata = JSON.parse(JSON.stringify(metadata));
  await sqlClient`
    INSERT INTO organization_projection_outbox (organization_id, name, slug, metadata)
    VALUES (${organization.id}, ${organization.name}, ${organization.slug || null}, ${sqlClient.json(jsonMetadata)})
    ON CONFLICT (organization_id) DO UPDATE SET
      name = EXCLUDED.name,
      slug = EXCLUDED.slug,
      metadata = EXCLUDED.metadata,
      revision = organization_projection_outbox.revision + 1,
      published_at = NULL,
      processing_at = NULL,
      last_error = NULL,
      updated_at = NOW()
  `;
}

async function recordOrganizationOwner(
  organizationId: string,
  ownerUserId: string,
): Promise<void> {
  await sqlClient`
    UPDATE organization_projection_outbox
    SET owner_user_id = ${ownerUserId},
        revision = revision + 1,
        published_at = NULL,
        processing_at = NULL,
        last_error = NULL,
        updated_at = NOW()
    WHERE organization_id = ${organizationId}
  `;
}

export async function flushOrganizationProjectionOutbox(
  organizationId?: string,
): Promise<number> {
  await sqlClient`
    UPDATE organization_projection_outbox o
    SET owner_user_id = (
      SELECT m.user_id
      FROM member m
      WHERE m.organization_id = o.organization_id
        AND 'owner' = ANY(string_to_array(m.role, ','))
      ORDER BY m.created_at
      LIMIT 1
    ), updated_at = NOW()
    WHERE o.owner_user_id IS NULL
      AND EXISTS (
        SELECT 1 FROM member m
        WHERE m.organization_id = o.organization_id
          AND 'owner' = ANY(string_to_array(m.role, ','))
      )
  `;
  const rows = organizationId
    ? await sqlClient<OrganizationOutboxRow[]>`
        WITH claimed AS (
          SELECT organization_id
          FROM organization_projection_outbox
          WHERE published_at IS NULL AND owner_user_id IS NOT NULL
            AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
            AND organization_id = ${organizationId}
          FOR UPDATE SKIP LOCKED
        )
        UPDATE organization_projection_outbox o
        SET processing_at = NOW(), updated_at = NOW()
        FROM claimed
        WHERE o.organization_id = claimed.organization_id
        RETURNING o.organization_id, o.name, o.slug, o.metadata, o.owner_user_id, o.revision
      `
    : await sqlClient<OrganizationOutboxRow[]>`
        WITH claimed AS (
          SELECT organization_id
          FROM organization_projection_outbox
          WHERE published_at IS NULL AND owner_user_id IS NOT NULL
            AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
          ORDER BY created_at
          LIMIT 100
          FOR UPDATE SKIP LOCKED
        )
        UPDATE organization_projection_outbox o
        SET processing_at = NOW(), updated_at = NOW()
        FROM claimed
        WHERE o.organization_id = claimed.organization_id
        RETURNING o.organization_id, o.name, o.slug, o.metadata, o.owner_user_id, o.revision
      `;

  let published = 0;
  for (const row of rows) {
    try {
      const revision = normalizeOutboxRevision(row.revision);
      const projection = await postOrgCore(
        `/internal/orgs/${encodeURIComponent(row.organization_id)}/reconcile`,
        {
          name: row.name,
          slug: row.slug,
          metadata: row.metadata || {},
          ownerUserId: row.owner_user_id,
          revision,
        },
      );
      const acknowledged = await sqlClient<Array<{ organization_id: string }>>`
        UPDATE organization_projection_outbox
        SET published_at = NOW(), attempts = attempts + 1,
            last_error = NULL, processing_at = NULL, updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND revision = ${revision}
        RETURNING organization_id
      `;
      if (acknowledged.length !== 1) continue;
      published++;
      if (eventPublisher && projection.applied) {
        try {
          await eventPublisher.publishOrganizationCreated({
            organizationId: row.organization_id,
            name: row.name,
            slug: row.slug || '',
            creatorId: row.owner_user_id,
            creatorEmail: '',
            metadata: row.metadata || {},
          });
        } catch (notificationError) {
          console.warn(
            'Organization projection synced; notification publish failed',
            notificationError,
          );
        }
      }
    } catch (error) {
      await sqlClient`
        UPDATE organization_projection_outbox
        SET attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)},
            processing_at = NULL,
            updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND revision = ${row.revision}
      `;
    }
  }
  return published;
}

export async function flushOrganizationMembershipOutbox(): Promise<number> {
  const rows = await sqlClient<MembershipOutboxRow[]>`
    WITH claimed AS (
      SELECT organization_id, user_id
      FROM organization_membership_outbox
      WHERE synced_at IS NULL
        AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
      ORDER BY updated_at
      LIMIT 200
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_membership_outbox o
    SET processing_at = NOW(), updated_at = NOW()
    FROM claimed
    WHERE o.organization_id = claimed.organization_id
      AND o.user_id = claimed.user_id
    RETURNING o.organization_id, o.user_id, o.role, o.desired_action, o.revision
  `;

  let synced = 0;
  for (const row of rows) {
    try {
      const revision = normalizeOutboxRevision(row.revision);
      await postOrgCore(
        `/internal/orgs/${encodeURIComponent(row.organization_id)}/members/reconcile`,
        {
          userId: row.user_id,
          role: row.role,
          action: row.desired_action,
          revision,
        },
      );
      const acknowledged = await sqlClient<Array<{ organization_id: string }>>`
        UPDATE organization_membership_outbox
        SET synced_at = NOW(), processing_at = NULL,
            attempts = attempts + 1, last_error = NULL, updated_at = NOW()
        WHERE organization_id = ${row.organization_id} AND user_id = ${row.user_id}
          AND revision = ${revision} AND desired_action = ${row.desired_action}
        RETURNING organization_id
      `;
      if (acknowledged.length !== 1) continue;
      synced++;
    } catch (error) {
      await sqlClient`
        UPDATE organization_membership_outbox
        SET processing_at = NULL, attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)}, updated_at = NOW()
        WHERE organization_id = ${row.organization_id} AND user_id = ${row.user_id}
          AND revision = ${row.revision} AND desired_action = ${row.desired_action}
      `;
    }
  }
  return synced;
}

export async function flushOrganizationDeletionOutbox(): Promise<number> {
  const rows = await sqlClient<OrganizationDeletionOutboxRow[]>`
    WITH claimed AS (
      SELECT organization_id
      FROM organization_deletion_outbox
      WHERE completed_at IS NULL
        AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
      ORDER BY created_at
      LIMIT 100
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_deletion_outbox o
    SET processing_at = NOW(), updated_at = NOW()
    FROM claimed
    WHERE o.organization_id = claimed.organization_id
    RETURNING o.organization_id, o.name, o.billing_synced_at, o.org_synced_at
  `;

  let synced = 0;
  for (const row of rows) {
    const errors: string[] = [];
    if (!row.billing_synced_at) {
      try {
        await deactivateOrganizationBilling(
          row.organization_id,
          'organization_deleted',
        );
        await sqlClient`
          UPDATE organization_deletion_outbox
          SET billing_synced_at = NOW(), updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND billing_synced_at IS NULL
        `;
      } catch (error) {
        errors.push(`billing: ${String(error)}`);
      }
    }
    if (!row.org_synced_at) {
      try {
        await postOrgCore(
          `/internal/orgs/${encodeURIComponent(row.organization_id)}/reconcile-delete`,
          {},
        );
        await sqlClient`
          UPDATE organization_deletion_outbox
          SET org_synced_at = NOW(), updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND org_synced_at IS NULL
        `;
      } catch (error) {
        errors.push(`org: ${String(error)}`);
      }
    }

    if (errors.length === 0) {
      const completed = await sqlClient<Array<{ organization_id: string }>>`
        UPDATE organization_deletion_outbox
        SET completed_at = NOW(), processing_at = NULL,
            attempts = attempts + 1, last_error = NULL, updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND billing_synced_at IS NOT NULL
          AND org_synced_at IS NOT NULL
          AND completed_at IS NULL
        RETURNING organization_id
      `;
      synced += completed.length;
    } else {
      await sqlClient`
        UPDATE organization_deletion_outbox
        SET processing_at = NULL, attempts = attempts + 1,
            last_error = ${errors.join('; ').slice(0, 1000)}, updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
      `;
    }
  }
  return synced;
}

export function setOrganizationEventPublisher(publisher: AuthEventPublisher) {
  console.log(
    '🔧 Setting organization event publisher for plugin:',
    !!publisher,
  );
  eventPublisher = publisher;
}

export async function publishOrganizationDeleted(
  organizationId: string,
  reason: string,
): Promise<void> {
  if (!eventPublisher) return;
  await eventPublisher.publishEvent({
    type: 'organization.deleted',
    organizationId,
    reason,
    timestamp: new Date().toISOString(),
  } as any);
}

export function organizationEventsPlugin(): BetterAuthPlugin {
  return {
    id: 'organization-events',
    init() {
      console.log('🎉 Organization Events Plugin initialized');

      return {
        options: {
          databaseHooks: {
            organization: {
              create: {
                after: async (organization) => {
                  console.log(
                    '🎊 Organization created hook triggered:',
                    organization.name,
                  );

                  try {
                    await recordOrganizationProjection(organization);
                    // Find the creator from the members
                    const creatorMember = organization.members?.[0];

                    if (!creatorMember?.userId) {
                      // The durable outbox waits for the following owner-member
                      // commit; a process restart cannot lose this projection.
                      return;
                    }

                    await recordOrganizationOwner(
                      organization.id,
                      creatorMember.userId,
                    );
                    await flushOrganizationProjectionOutbox(organization.id);

                    console.log(
                      '📢 Published organization.created event:',
                      organization.id,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish organization created event:',
                      error,
                    );
                  }
                },
              },
            },
            member: {
              create: {
                after: async (member) => {
                  console.log(
                    '👤 Member added hook triggered for org:',
                    member.organizationId,
                  );

                  try {
                    if (
                      member.role
                        .split(',')
                        .some((role) => role.trim() === 'owner')
                    ) {
                      await recordOrganizationOwner(
                        member.organizationId,
                        member.userId,
                      );
                      await flushOrganizationProjectionOutbox(
                        member.organizationId,
                      );
                    }

                    if (!eventPublisher) {
                      console.warn(
                        '⚠️ Organization event publisher not available; membership event will rely on upstream retry',
                      );
                      return;
                    }

                    await eventPublisher.publishOrganizationMemberAdded({
                      organizationId: member.organizationId,
                      organizationName: '', // We don't have this in this context
                      userId: member.userId,
                      userEmail: '', // We don't have this in this context
                      role: member.role,
                      invitedBy: undefined,
                    });

                    console.log(
                      '📢 Published member_added event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member added event:',
                      error,
                    );
                  }
                },
              },
              delete: {
                after: async (member) => {
                  console.log(
                    '👋 Member removed hook triggered for org:',
                    member.organizationId,
                  );

                  try {
                    if (!eventPublisher) {
                      console.warn(
                        '⚠️ Organization event publisher not available; removal will use durable reconciliation',
                      );
                      return;
                    }
                    await eventPublisher.publishOrganizationMemberRemoved({
                      organizationId: member.organizationId,
                      organizationName: '', // We don't have this in this context
                      userId: member.userId,
                      userEmail: '', // We don't have this in this context
                      removedBy: undefined,
                    });

                    console.log(
                      '📢 Published member_removed event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member removed event:',
                      error,
                    );
                  }
                },
              },
            },
          },
        },
      } as any;
    },
  };
}
