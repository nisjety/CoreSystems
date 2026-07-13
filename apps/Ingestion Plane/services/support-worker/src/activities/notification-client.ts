import { createHash, createHmac, randomBytes } from 'node:crypto';

export type NotificationRequest = {
  organization_id: string;
  idempotency_key: string;
  retention_mode: 'zdr';
  recipient: { kind: 'user'; id: string };
  type: string;
  payload: Record<string, unknown>;
};

type NotificationClientConfig = {
  baseUrl: string;
  serviceToken: string;
  request: NotificationRequest;
  now?: () => Date;
  nonce?: () => string;
};

type NotificationKeyInput = {
  type: string;
  ticketId: number;
  controlUserId?: string;
};

export function buildNotificationIdempotencyKey(
  input: NotificationKeyInput,
): string {
  const prefix = `support:${input.type}:ticket:${input.ticketId}`;
  const controlUserId = input.controlUserId?.trim();
  if (!controlUserId) return prefix;
  const recipientHash = createHash('sha256')
    .update(controlUserId)
    .digest('hex')
    .slice(0, 16);
  return `${prefix}:recipient:${recipientHash}`;
}

export async function postNotification(
  input: NotificationClientConfig,
  fetchImpl: typeof fetch = fetch,
): Promise<void> {
  const url = `${input.baseUrl.replace(/\/$/, '')}/api/v1/notification-requests`;
  const serviceToken = input.serviceToken.trim();
  if (serviceToken.length < 32) {
    throw new Error('notification-core service token must be at least 32 bytes');
  }
  const organizationId = input.request.organization_id.trim();
  const recipientId = input.request.recipient.id.trim();
  if (!organizationId || input.request.recipient.kind !== 'user' || !recipientId) {
    throw new Error('notification request requires an organization-scoped user recipient');
  }
  const body = JSON.stringify(input.request);
  const timestamp = (input.now ?? (() => new Date()))().toISOString();
  const nonce = (input.nonce ?? (() => randomBytes(24).toString('base64url')))();
  const bodyDigest = createHash('sha256').update(body).digest('base64url');
  const parsedUrl = new URL(url);
  const canonical = [
    'v2',
    'support-worker',
    'notification-core',
    timestamp,
    nonce,
    'POST',
    `${parsedUrl.pathname}${parsedUrl.search}`,
    '',
    organizationId,
    '',
    bodyDigest,
  ].join('\n');
  const signature = createHmac('sha256', serviceToken)
    .update(canonical)
    .digest('base64url');
  const response = await fetchImpl(url, {
    method: 'POST',
    redirect: 'manual',
    signal: AbortSignal.timeout(10_000),
    headers: {
      'Content-Type': 'application/json',
      'X-Service-Id': 'support-worker',
      'X-Org-Id': organizationId,
      'X-Delegation-Timestamp': timestamp,
      'X-Delegation-Nonce': nonce,
      'X-Delegation-Body-Sha256': bodyDigest,
      'X-Delegation-Signature': signature,
    },
    body,
  });

  if (!response.ok) {
    throw new Error(
      `notification-core POST /api/v1/notification-requests failed: ${response.status} ${response.statusText}`,
    );
  }

  const responseBody = (await response.json().catch(() => null)) as
    | { status?: unknown }
    | null;
  const status = typeof responseBody?.status === 'string' ? responseBody.status : '';
  if (!status) {
    throw new Error('notification-core response is missing submitted or suppressed status');
  }
  if (!['submitted', 'suppressed'].includes(status)) {
    throw new Error(
      `notification-core returned non-delivery status: ${status}`,
    );
  }
}
