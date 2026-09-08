import { fileURLToPath, URL } from 'node:url'
import { configDefaults, defineConfig } from 'vitest/config'
import { ViteMcp } from 'vite-plugin-mcp'
import solid from '@solidjs/vite-plugin'

// Server-only proxy target (NOT VITE_-prefixed, so it never leaks to the client
// bundle). VITE_VEREVON_GATEWAY_URL is kept as a fallback for backward compat.
const gatewayProxyTarget =
  process.env.GATEWAY_PROXY_TARGET ||
  process.env.VITE_VEREVON_GATEWAY_URL ||
  'http://127.0.0.1:3185'
const isVitest = Boolean(process.env.VITEST)
// vite-plugin-mcp 0.3.x attaches a single MCP Server instance to the Vite
// process. A second SSE client tries to connect that same instance again and
// crashes the dev server, taking the local Support workspace down with it.
// Keep the development endpoint available for focused debugging, but opt in
// explicitly so normal local Docker/browser work stays reliable.
const enableViteMcp = process.env.VEREVON_VITE_MCP_ENABLED === 'true'

export default defineConfig({
  plugins: [
    solid(),
    // Exposes Vite's dev-time MCP endpoint at /__mcp/sse only when an
    // operator deliberately starts a single-client MCP debugging session.
    !isVitest && enableViteMcp && ViteMcp({
      updateConfig: false,
      updateConfigServerName: 'verevon-v3-vite',
    }),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  // Same-origin BFF: the browser calls /api on the dev origin; Vite forwards to
  // the gateway server-side, so Better Auth's session cookie stays first-party.
  // The proxy target is a server-only (non-VITE_) var so it is NEVER baked into
  // the client bundle — otherwise the browser would try to hit the gateway's
  // Docker service name directly and fail (ERR_NAME_NOT_RESOLVED). In-container
  // it must be the gateway service name; host-run `pnpm dev` falls back to
  // 127.0.0.1:3185 (the host-mapped gateway port).
  //
  // Known limitation on Docker Desktop for Windows: host filesystem change
  // events don't always reach the container's inotify watcher through the
  // bind mount — an edit lands on disk but Vite keeps serving the pre-edit
  // module. `server.watch.usePolling` "fixes" this but pegs the event loop
  // hard enough to make the dev server stop answering requests entirely
  // (this tree's `.claude/worktrees/` holds several full extra monorepo
  // checkouts, and scoping `ignored` to exclude them wasn't enough either).
  // If an edit isn't reflected, `docker restart frontend-plane-verevonv3-frontend-1`
  // rather than reaching for polling again.
  server: {
    proxy: {
      '/api': {
        target: gatewayProxyTarget,
        changeOrigin: true,
        ws: true,
      },
      '/health': {
        target: gatewayProxyTarget,
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    // Playwright specs live under tests/e2e and import @playwright/test — they
    // must never be collected by vitest (they run via `pnpm test:e2e`).
    exclude: [
      ...configDefaults.exclude,
      'tests/e2e/**',
      // This gateway coverage gate intentionally uses Node's native test runner
      // and is executed explicitly by `pnpm test` before the Vitest suite.
      'apps/gateway/scripts/**/*.test.mjs',
      // `.claude/worktrees/**` holds nested full-repo checkouts (git worktrees for
      // other sessions), e.g. a copy of verevonv2 (React) — never collect their
      // tests against this SolidJS-oriented config, regardless of what
      // worktrees exist on disk.
      '**/.claude/worktrees/**',
      // Workspace packages and sub-apps own their own vitest config and runner
      // (`pnpm --filter <name> test`). Sweeping them in here runs them under
      // jsdom, where e.g. remote-core's libsodium WASM fails on cross-realm
      // Uint8Array checks that never occur in its real (node) environment.
      'packages/**',
      'apps/remote-dev/**',
    ],
    // Default per-test budget. The A8 fabrication-guard test spins up the real
    // ESLint flat config (cold-start ~8s), which exceeds vitest's 5s default when
    // that file runs in isolation; 30s keeps the suite stable on CI / slow hosts.
    testTimeout: 30_000,
  },
})
