import { fileURLToPath, URL } from 'node:url'
import { configDefaults, defineConfig } from 'vitest/config'
import { ViteMcp } from 'vite-plugin-mcp'
import solid from 'vite-plugin-solid'

// Server-only proxy target (NOT VITE_-prefixed, so it never leaks to the client
// bundle). VITE_VELION_GATEWAY_URL is kept as a fallback for backward compat.
const gatewayProxyTarget =
  process.env.GATEWAY_PROXY_TARGET ||
  process.env.VITE_VELION_GATEWAY_URL ||
  'http://127.0.0.1:3185'
const isVitest = Boolean(process.env.VITEST)

export default defineConfig({
  plugins: [
    solid(),
    // Exposes Vite's dev-time MCP endpoint at /__mcp/sse without leaking MCP state into the client bundle.
    !isVitest && ViteMcp({
      updateConfig: false,
      updateConfigServerName: 'velion-v3-vite',
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
    ],
    // Default per-test budget. The A8 fabrication-guard test spins up the real
    // ESLint flat config (cold-start ~8s), which exceeds vitest's 5s default when
    // that file runs in isolation; 30s keeps the suite stable on CI / slow hosts.
    testTimeout: 30_000,
  },
})
