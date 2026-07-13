type InternalAuthEnvironment = Record<string, string | undefined>;

async function digest(value: string): Promise<Uint8Array> {
  const bytes = new TextEncoder().encode(value);
  const result = await crypto.subtle.digest("SHA-256", bytes);
  return new Uint8Array(result);
}

function constantTimeEqual(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left[index] ^ right[index];
  }
  return difference === 0;
}

export async function authorizeInternalRequest(
  request: Request,
  environment: InternalAuthEnvironment = process.env,
): Promise<boolean> {
  const expectedKey =
    environment.CONVEX_INTERNAL_SERVICE_KEY || environment.INTERNAL_API_KEY;
  if (!expectedKey) {
    throw new Error("Convex internal service key is not configured");
  }

  const authorization = request.headers.get("authorization") || "";
  const match = /^Bearer ([^\s]+)$/.exec(authorization);
  if (!match) return false;

  const [providedDigest, expectedDigest] = await Promise.all([
    digest(match[1]),
    digest(expectedKey),
  ]);
  return constantTimeEqual(providedDigest, expectedDigest);
}
