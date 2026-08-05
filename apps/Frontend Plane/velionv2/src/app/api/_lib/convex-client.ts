import { ConvexHttpClient } from "convex/browser";

/**
 * Server-side Convex client for verevonv2 BFF routes. Ported from verevon v1
 * (src/app/api/_lib/convex-client.ts) and adapted to the convex-core
 * service-key convention: every call injects `serviceKey`, which the target
 * mutation/query validates via `assertServiceKey` (see convex-core/convex/
 * authz.ts). Search-history writes therefore stay server-side — the browser
 * never holds the service key.
 *
 * Transport: createThread/appendTurn are PUBLIC functions gated by the
 * serviceKey arg, so no admin auth is required on self-hosted Convex. We still
 * set admin auth opportunistically when CONVEX_ADMIN_KEY is present (defense in
 * depth / parity with verevon v1), but its absence is not fatal.
 */

// In-network address for the self-hosted backend. verevonv2 and convex-backend
// share the `inter-plane-bus` docker network, so `convex-backend:3210` resolves.
const CONVEX_API_URL =
  process.env.CONVEX_API_URL ||
  process.env.NEXT_PUBLIC_CONVEX_HTTP ||
  process.env.NEXT_PUBLIC_CONVEX_URL ||
  "http://convex-backend:3210";

const CONVEX_ADMIN_KEY =
  process.env.CONVEX_ADMIN_KEY || process.env.CONVEX_SELF_HOSTED_ADMIN_KEY || "";

// The service key the convex-core deployment expects (CONVEX_INTERNAL_SERVICE_KEY
// on the deployment). No hardcoded fallback: convex-core's validator now rejects
// an unset/placeholder key outright, so a real value here is required, not
// optional — falling back to a literal would just send a value guaranteed to be
// refused.
export const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY ||
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  "";

export function getConvexClient(): ConvexHttpClient {
  const client = new ConvexHttpClient(CONVEX_API_URL, {
    skipConvexDeploymentUrlCheck: true,
  });
  const unsafeClient = client as ConvexHttpClient & {
    setAdminAuth?: (token: string) => void;
    setFetchOptions?: (options: { cache: "force-cache" | "no-store" }) => void;
  };

  if (CONVEX_ADMIN_KEY) {
    unsafeClient.setAdminAuth?.(CONVEX_ADMIN_KEY);
  }
  unsafeClient.setFetchOptions?.({ cache: "no-store" });
  return client;
}

/** Run a convex-core query, injecting the service key. */
export async function convexQuery<T>(
  name: string,
  args: Record<string, unknown>,
): Promise<T> {
  return (await (getConvexClient() as unknown as {
    query: (n: string, a: Record<string, unknown>) => Promise<unknown>;
  }).query(name, { serviceKey: CONVEX_INTERNAL_SERVICE_KEY, ...args })) as T;
}

/** Run a convex-core mutation, injecting the service key. */
export async function convexMutation<T>(
  name: string,
  args: Record<string, unknown>,
): Promise<T> {
  return (await (getConvexClient() as unknown as {
    mutation: (
      n: string,
      a: Record<string, unknown>,
      o?: { skipQueue?: boolean },
    ) => Promise<unknown>;
  }).mutation(
    name,
    { serviceKey: CONVEX_INTERNAL_SERVICE_KEY, ...args },
    { skipQueue: true },
  )) as T;
}
