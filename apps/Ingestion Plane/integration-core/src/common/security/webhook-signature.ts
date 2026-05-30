import { createHash, timingSafeEqual } from 'node:crypto';

export function extractWebhookSignature(
  headers: Record<string, string | string[] | undefined>
): string | null {
  const candidates = [
    headers['x-nango-signature'],
    headers['x-connector-signature'],
    headers['X-Nango-Signature'],
    headers['X-Connector-Signature']
  ];

  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim().length > 0) {
      return candidate.trim();
    }

    if (Array.isArray(candidate) && candidate.length > 0) {
      return candidate[0]?.trim() || null;
    }
  }

  return null;
}

export function verifyWebhookSignature(
  body: string,
  signature: string | null,
  secret: string | undefined
): boolean {
  if (!secret || !signature) {
    return false;
  }

  const expected = createHash('sha256').update(secret).update(body).digest('hex');
  const expectedBuffer = Buffer.from(expected, 'utf8');
  const signatureBuffer = Buffer.from(signature, 'utf8');

  if (expectedBuffer.length !== signatureBuffer.length) {
    return false;
  }

  return timingSafeEqual(expectedBuffer, signatureBuffer);
}