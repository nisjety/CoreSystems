/**
 * AFFiNE / OctoBase connection configuration.
 *
 * All values are driven by environment variables so that the same build can
 * point at different AFFiNE backends in dev / staging / production without
 * rebuilding.
 *
 * Required env vars (add to .env.local):
 *   NEXT_PUBLIC_AFFINE_SERVER_URL   – e.g. http://localhost:3010
 *   NEXT_PUBLIC_AFFINE_WS_URL       – e.g. ws://localhost:3010
 */

export const AFFINE_SERVER_URL =
  process.env.NEXT_PUBLIC_AFFINE_SERVER_URL ?? 'http://localhost:47810';

export const AFFINE_WS_URL =
  process.env.NEXT_PUBLIC_AFFINE_WS_URL ??
  AFFINE_SERVER_URL.replace(/^http/, 'ws');

/** Default workspace ID used for the planner.  Override per-user if needed. */
export const AFFINE_DEFAULT_WORKSPACE_ID = 'planner';

/** REST endpoints on the AFFiNE server */
export const AFFINE_ENDPOINTS = {
  /** Create-admin or fetch user info */
  user: `${AFFINE_SERVER_URL}/api/auth`,
  /** GraphQL entry point (AFFiNE uses NestJS + GraphQL) */
  graphql: `${AFFINE_SERVER_URL}/graphql`,
  /** Binary doc sync over WebSocket */
  sync: `${AFFINE_WS_URL}/api/sync`,
  /** Health check */
  health: `${AFFINE_SERVER_URL}/api/health`,
} as const;
