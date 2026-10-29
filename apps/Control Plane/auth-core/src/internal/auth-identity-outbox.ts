import { randomUUID } from 'node:crypto';
import { sqlClient } from '../db';
import { AuthEventPublisher } from './auth-event.publisher';
import {
  authGdprEmailIdentity,
  authGdprUserIdentity,
  lockAuthGdprIdentities,
} from './auth-gdpr-publish-fence';

export type IdentityOutboxRow = {
  event_id: string;
  event_type: 'user_registered' | 'provider_linked';
  user_id: string;
  payload: unknown;
  attempts: number;
};

type ParsedIdentityEvent =
  | {
      eventType: 'user_registered';
      data: {
        userId: string;
        email: string;
        name?: string;
        provider: string;
        emailVerified: boolean;
      };
    }
  | {
      eventType: 'provider_linked';
      data: {
        userId: string;
        email: string;
        provider: string;
        providerAccountId: string;
        scopesGranted?: string[];
      };
    };

function requiredString(
  value: unknown,
  field: string,
  maxLength: number,
): string {
  if (typeof value !== 'string') {
    throw new Error(`identity outbox ${field} must be a string`);
  }
  const normalized = value.trim();
  if (normalized.length === 0 || normalized.length > maxLength) {
    throw new Error(`identity outbox ${field} has invalid length`);
  }
  return normalized;
}

export function parseIdentityOutboxRow(
  row: IdentityOutboxRow,
): ParsedIdentityEvent {
  if (
    !row.payload ||
    typeof row.payload !== 'object' ||
    Array.isArray(row.payload)
  ) {
    throw new Error('identity outbox payload must be an object');
  }
  const payload = row.payload as Record<string, unknown>;
  const userId = requiredString(payload.userId, 'userId', 255);
  if (userId !== row.user_id) {
    throw new Error('identity outbox userId does not match canonical row');
  }
  const email = requiredString(payload.email, 'email', 320).toLowerCase();
  if (!/^[^@\s]+@[^@\s]+$/.test(email)) {
    throw new Error('identity outbox email is invalid');
  }
  const provider = requiredString(payload.provider, 'provider', 128);

  if (row.event_type === 'user_registered') {
    if (row.event_id !== `user:${userId}:registered`) {
      throw new Error('identity outbox registration event id is invalid');
    }
    if (typeof payload.emailVerified !== 'boolean') {
      throw new Error('identity outbox emailVerified must be boolean');
    }
    const name =
      payload.name === null || payload.name === undefined
        ? undefined
        : requiredString(payload.name, 'name', 256);
    return {
      eventType: row.event_type,
      data: {
        userId,
        email,
        name,
        provider,
        emailVerified: payload.emailVerified,
      },
    };
  }

  if (!/^account:[^:]{1,255}:provider_linked$/.test(row.event_id)) {
    throw new Error('identity outbox provider event id is invalid');
  }
  const providerAccountId = requiredString(
    payload.providerAccountId,
    'providerAccountId',
    512,
  );
  let scopesGranted: string[] | undefined;
  if (payload.scopesGranted !== undefined) {
    if (
      !Array.isArray(payload.scopesGranted) ||
      payload.scopesGranted.length > 256
    ) {
      throw new Error('identity outbox scopesGranted must be a bounded array');
    }
    scopesGranted = payload.scopesGranted.map((scope) =>
      requiredString(scope, 'scope', 256),
    );
  }
  return {
    eventType: row.event_type,
    data: { userId, email, provider, providerAccountId, scopesGranted },
  };
}

export async function flushAuthIdentityEventOutbox(
  publisher: AuthEventPublisher,
  userId?: string,
): Promise<number> {
  const claimToken = randomUUID();
  const rows = userId
    ? await sqlClient<IdentityOutboxRow[]>`
        WITH claimed AS (
          SELECT candidate.event_id
          FROM auth_identity_event_outbox candidate
          WHERE candidate.published_at IS NULL
            AND candidate.dead_lettered_at IS NULL
            AND (candidate.processing_at IS NULL OR candidate.processing_at < NOW() - INTERVAL '5 minutes')
            AND candidate.user_id = ${userId}
            AND (
              candidate.event_type <> 'provider_linked'
              OR EXISTS (
                SELECT 1
                FROM auth_identity_event_outbox dependency
                WHERE dependency.user_id = candidate.user_id
                  AND dependency.event_type = 'user_registered'
                  AND dependency.published_at IS NOT NULL
              )
            )
          ORDER BY candidate.created_at, candidate.event_id
          LIMIT 100
          FOR UPDATE SKIP LOCKED
        )
        UPDATE auth_identity_event_outbox o
        SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
        FROM claimed
        WHERE o.event_id = claimed.event_id
        RETURNING o.event_id, o.event_type, o.user_id, o.payload, o.attempts
      `
    : await sqlClient<IdentityOutboxRow[]>`
        WITH claimed AS (
          SELECT candidate.event_id
          FROM auth_identity_event_outbox candidate
          WHERE candidate.published_at IS NULL
            AND candidate.dead_lettered_at IS NULL
            AND (candidate.processing_at IS NULL OR candidate.processing_at < NOW() - INTERVAL '5 minutes')
            AND (
              candidate.event_type <> 'provider_linked'
              OR EXISTS (
                SELECT 1
                FROM auth_identity_event_outbox dependency
                WHERE dependency.user_id = candidate.user_id
                  AND dependency.event_type = 'user_registered'
                  AND dependency.published_at IS NOT NULL
              )
            )
          ORDER BY candidate.created_at, candidate.event_id
          LIMIT 100
          FOR UPDATE SKIP LOCKED
        )
        UPDATE auth_identity_event_outbox o
        SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
        FROM claimed
        WHERE o.event_id = claimed.event_id
        RETURNING o.event_id, o.event_type, o.user_id, o.payload, o.attempts
      `;

  let published = 0;
  for (const row of rows) {
    try {
      const payload = row.payload as { email?: unknown } | null;
      const email =
        payload && typeof payload.email === 'string'
          ? payload.email
          : undefined;
      const acknowledged = await sqlClient.begin(async (tx) => {
        await lockAuthGdprIdentities(tx, [
          authGdprEmailIdentity(email),
          authGdprUserIdentity(row.user_id),
        ]);
        const current = await tx<IdentityOutboxRow[]>`
          SELECT event_id, event_type, user_id, payload, attempts
          FROM auth_identity_event_outbox
          WHERE event_id = ${row.event_id}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
            AND dead_lettered_at IS NULL
          FOR UPDATE
        `;
        if (current.length !== 1) return false;
        const event = parseIdentityOutboxRow(current[0]);
        if (event.eventType === 'user_registered') {
          await publisher.publishUserRegistered(
            event.data,
            undefined,
            row.event_id,
          );
        } else {
          await publisher.publishUserProviderLinked(
            event.data,
            undefined,
            row.event_id,
          );
        }

        const updated = await tx<Array<{ event_id: string }>>`
          UPDATE auth_identity_event_outbox
          SET published_at = NOW(), processing_at = NULL, claim_token = NULL,
              attempts = attempts + 1, last_error = NULL, updated_at = NOW()
          WHERE event_id = ${row.event_id}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          RETURNING event_id
        `;
        return updated.length === 1;
      });
      if (acknowledged) published++;
    } catch (error) {
      await sqlClient`
        UPDATE auth_identity_event_outbox
        SET processing_at = NULL, claim_token = NULL,
            attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)},
            dead_lettered_at = CASE
              WHEN attempts + 1 >= 20 THEN NOW()
              ELSE dead_lettered_at
            END,
            updated_at = NOW()
        WHERE event_id = ${row.event_id}
          AND claim_token = ${claimToken}
          AND published_at IS NULL
      `;
    }
  }
  return published;
}
