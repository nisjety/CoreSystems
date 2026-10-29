/**
 * Application owns the product-level recipient set for a room/case
 * conversation. This module commits to stable principal identifiers only; it
 * intentionally never accepts display names, emails, or an organization-wide
 * membership list as a recipient audience.
 */

const AUDIENCE_DOMAIN = 'recipient-audience/v1\0';

function encodeUint64(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error('recipient audience length must be a non-negative safe integer');
  }
  const output = new Uint8Array(8);
  const view = new DataView(output.buffer);
  view.setUint32(0, Math.floor(value / 2 ** 32));
  view.setUint32(4, value >>> 0);
  return output;
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const length = parts.reduce((total, part) => total + part.length, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

export function canonicalRecipientSubjectIds(recipientSubjectIds: readonly string[]): readonly string[] {
  if (recipientSubjectIds.length === 0) {
    throw new Error('recipient audience must not be empty');
  }
  const canonical = recipientSubjectIds.map((id) => id.trim());
  if (canonical.some((id) => !id)) {
    throw new Error('recipient audience contains an empty subject');
  }
  canonical.sort((left, right) => left.localeCompare(right));
  if (canonical.some((id, index) => id === canonical[index - 1])) {
    throw new Error('recipient audience contains a duplicate subject');
  }
  return canonical;
}

/**
 * A content-free commitment to an exact recipient set. The byte encoding is
 * intentionally identical to Control's RecipientAudienceHash: a domain
 * separator followed by sorted, length-prefixed stable principal IDs.
 */
export async function recipientAudienceHash(recipientSubjectIds: readonly string[]): Promise<string> {
  const encoder = new TextEncoder();
  const canonical = canonicalRecipientSubjectIds(recipientSubjectIds);
  const payload = concat([
    encoder.encode(AUDIENCE_DOMAIN),
    ...canonical.flatMap((id) => {
      const bytes = encoder.encode(id);
      return [encodeUint64(bytes.length), bytes];
    }),
  ]);
  // `Uint8Array` is backed by an ArrayBuffer here; retain an exact copy so
  // TypeScript does not permit a SharedArrayBuffer-backed view at the Web
  // Crypto boundary.
  const bytes = payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength) as ArrayBuffer;
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return `sha256:${Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}

export function spaceRecipientAudienceRef(spaceRef: string, revision: number): string {
  const normalized = spaceRef.trim();
  if (!normalized || !Number.isSafeInteger(revision) || revision <= 0) {
    throw new Error('recipient audience identity is invalid');
  }
  return `space:${normalized}:recipient-audience:${revision}`;
}
