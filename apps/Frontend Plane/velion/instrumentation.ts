export async function register() {
  if (process.env.NEXT_RUNTIME === 'nodejs') {
    // G39: the boot-time internal-API-key validation + cross-service
    // handshake have moved to `scripts/check-internal-api-keys.mjs` because
    // Next.js 16 webpack dev mode does not reliably invoke this hook. The
    // script wraps `next dev` / `next start` in `package.json`, so the
    // checks fire synchronously before the server binds. The TS modules
    // (`src/lib/server/internal-api-key-{assertion,handshake}.ts`) are
    // kept so other code can re-use them.

    await import('./src/components/auth/lib/orpc/orpc.server');
  }
}
