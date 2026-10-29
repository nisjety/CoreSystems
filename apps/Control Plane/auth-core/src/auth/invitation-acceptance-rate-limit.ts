import { createHmac, timingSafeEqual } from 'node:crypto';

const MARKER_MAX_SKEW_SECONDS = 30;

/**
 * Better Auth 1.6.x keys its built-in limiter by IP. The Control gateway is the
 * only browser-facing caller of this wrapper, so the inner request deliberately
 * replaces any caller IP header with a stable, pseudonymous address derived
 * from the authenticated actor. This gives every signed-in user an independent
 * limiter bucket without persisting the raw user id in Dragonfly.
 */
export function invitationActorRateLimitAddress(
  rawUserId: string,
  rawSecret: string,
): string {
  const userId = rawUserId.trim();
  const secret = rawSecret.trim();
  if (!userId) throw new Error('authenticated user id is required');
  if (!secret) throw new Error('Better Auth secret is required');

  const digest = createHmac('sha256', secret).update(userId).digest();
  // Use a ULA-shaped IPv6 value with 120 HMAC bits. Better Auth may normalize
  // IPv6 to a subnet, but this still leaves far more collision resistance than
  // the previous 22-bit IPv4 mapping.
  const bytes = Buffer.concat([Buffer.from([0xfd]), digest.subarray(0, 15)]);
  const groups = Array.from({ length: 8 }, (_, index) =>
    bytes
      .readUInt16BE(index * 2)
      .toString(16)
      .padStart(4, '0'),
  );
  return groups.join(':');
}

function markerSignature(
  invitationId: string,
  timestampSeconds: number,
  secret: string,
): Buffer {
  return createHmac('sha256', secret)
    .update(`invitation-acceptance\n${timestampSeconds}\n${invitationId}`)
    .digest();
}

export function invitationAcceptanceInternalMarker(
  rawInvitationId: string,
  rawSecret: string,
  nowMs = Date.now(),
): string {
  const invitationId = rawInvitationId.trim();
  const secret = rawSecret.trim();
  if (!invitationId) throw new Error('invitation id is required');
  if (!secret) throw new Error('Better Auth secret is required');
  const timestampSeconds = Math.floor(nowMs / 1000);
  const signature = markerSignature(invitationId, timestampSeconds, secret);
  return `v1.${timestampSeconds}.${signature.toString('base64url')}`;
}

export function verifyInvitationAcceptanceInternalMarker(
  rawMarker: string | null | undefined,
  rawInvitationId: string,
  rawSecret: string,
  nowMs = Date.now(),
): boolean {
  const invitationId = rawInvitationId.trim();
  const secret = rawSecret.trim();
  const marker = rawMarker?.trim() ?? '';
  if (!invitationId || !secret || marker.length > 96) return false;
  const parts = marker.split('.');
  if (
    parts.length !== 3 ||
    parts[0] !== 'v1' ||
    !/^\d{10,11}$/.test(parts[1]) ||
    !/^[A-Za-z0-9_-]{43}$/.test(parts[2])
  ) {
    return false;
  }
  const timestampSeconds = Number.parseInt(parts[1], 10);
  const nowSeconds = Math.floor(nowMs / 1000);
  if (Math.abs(nowSeconds - timestampSeconds) > MARKER_MAX_SKEW_SECONDS) {
    return false;
  }
  const supplied = Buffer.from(parts[2], 'base64url');
  const expected = markerSignature(invitationId, timestampSeconds, secret);
  return (
    supplied.length === expected.length && timingSafeEqual(supplied, expected)
  );
}
