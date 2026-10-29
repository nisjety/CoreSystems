type ReconciliationEnvironment = Record<string, string | undefined>;

export type ReconciliationAuthorization = {
  authorized: boolean;
  nonce: string;
  orgId: string;
  timestamp: number;
};

const maxClockSkewMs = 5 * 60 * 1000;

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function constantTimeEqual(left: string, right: string): boolean {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left.charCodeAt(index) ^ right.charCodeAt(index);
  }
  return difference === 0;
}

async function sign(key: string, value: string): Promise<string> {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign(
    "HMAC",
    cryptoKey,
    new TextEncoder().encode(value),
  );
  return bytesToHex(new Uint8Array(signature));
}

export async function authorizeReconciliationRequest(
  request: Request,
  rawBody: string,
  environment: ReconciliationEnvironment = process.env,
  now: number = Date.now(),
): Promise<ReconciliationAuthorization> {
  const key = environment.CONVEX_RECONCILIATION_KEY;
  if (!key) throw new Error("CONVEX_RECONCILIATION_KEY is not configured");

  const orgId = request.headers.get("x-reconciliation-org")?.trim() ?? "";
  const nonce = request.headers.get("x-reconciliation-nonce")?.trim() ?? "";
  const signature = request.headers.get("x-reconciliation-signature")?.trim() ?? "";
  const timestamp = Number(request.headers.get("x-reconciliation-timestamp"));
  const validShape =
    orgId.length > 0 &&
    /^[A-Za-z0-9_-]{16,128}$/.test(nonce) &&
    /^[a-f0-9]{64}$/.test(signature) &&
    Number.isSafeInteger(timestamp) &&
    Math.abs(now - timestamp) <= maxClockSkewMs;
  if (!validShape) return { authorized: false, nonce, orgId, timestamp };

  const path = new URL(request.url).pathname;
  const canonical = [
    timestamp,
    request.method.toUpperCase(),
    path,
    orgId,
    nonce,
    rawBody,
  ].join("\n");
  const expected = await sign(key, canonical);
  return {
    authorized: constantTimeEqual(signature, expected),
    nonce,
    orgId,
    timestamp,
  };
}
