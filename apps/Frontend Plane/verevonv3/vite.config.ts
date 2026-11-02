import { fileURLToPath, URL } from 'node:url'
import { configDefaults, defineConfig } from 'vitest/config'
import { ViteMcp } from 'vite-plugin-mcp'
import solid from '@solidjs/vite-plugin'
import type { Plugin } from 'vite'

// Server-only proxy target (NOT VITE_-prefixed, so it never leaks to the client
// bundle). VITE_VEREVON_GATEWAY_URL is kept as a fallback for backward compat.
const gatewayProxyTarget =
  process.env.GATEWAY_PROXY_TARGET ||
  process.env.VITE_VEREVON_GATEWAY_URL ||
  'http://127.0.0.1:3185'
const isVitest = Boolean(process.env.VITEST)
const sceneCaptureMode = process.env.VITE_SCENE_MODE === '1'
// vite-plugin-mcp 0.3.x attaches a single MCP Server instance to the Vite
// process. A second SSE client tries to connect that same instance again and
// crashes the dev server, taking the local Support workspace down with it.
// Keep the development endpoint available for focused debugging, but opt in
// explicitly so normal local Docker/browser work stays reliable.
const enableViteMcp = process.env.VEREVON_VITE_MCP_ENABLED === 'true'

// Build id (WS07-S2). The Docker `build` stage passes SOURCE_REVISION (the git
// sha) and BUILD_DATE; outside an image build neither is set and the id is
// `dev`, which the stale-tab check never treats as a new version. At most 12
// characters, so the same value fits the diagnostics beacon's `build` field.
const rawRevision = (process.env.SOURCE_REVISION ?? '').trim()
const buildId =
  rawRevision === '' || rawRevision === 'unverified' || rawRevision === 'unknown'
    ? 'dev'
    : rawRevision.slice(0, 12)
const builtAt = (process.env.BUILD_DATE ?? '').trim() || new Date().toISOString()
// The SPA's client contract version, written to version.json. WS07-S3 owns the
// constant pair (gateway + SPA) and its parity test; keep this number equal to it.
const clientContract = 1

// Emits dist/version.json and stamps <meta name="verevon-build"> into index.html.
// nginx serves /version.json no-store; the SPA and the boot watchdog compare it
// with the meta value / the compiled-in id to detect a newer deploy.
function verevonBuildInfo(): Plugin {
  return {
    name: 'verevon-build-info',
    transformIndexHtml() {
      return [
        { tag: 'meta', attrs: { name: 'verevon-build', content: buildId }, injectTo: 'head-prepend' },
      ]
    },
    generateBundle() {
      this.emitFile({
        type: 'asset',
        fileName: 'version.json',
        source: JSON.stringify({ build_id: buildId, built_at: builtAt, client_contract: clientContract }),
      })
    },
  }
}

export default defineConfig({
  define: {
    __VEREVON_BUILD_ID__: JSON.stringify(buildId),
  },
  plugins: [
    solid(),
    sceneCaptureMode && {
      name: 'scene-capture-block-gateway',
      configureServer(server) {
        server.middlewares.use((request, response, next) => {
          const pathname = new URL(request.url ?? '/', 'http://scene.local').pathname
          if (pathname === '/api' || pathname.startsWith('/api/') || pathname === '/health' || pathname.startsWith('/health/')) {
            response.statusCode = 503
            response.setHeader('Content-Type', 'text/plain; charset=utf-8')
            response.end('Gateway access is disabled in scene capture mode.')
            return
          }
          next()
        })
      },
    },
    verevonBuildInfo(),
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
  // This SPA has one entry. The default **/*.html discovery also traverses
  // the sibling apps, package store, and generated reports on the Windows
  // bind mount, delaying startup before the first page can be served.
  optimizeDeps: { entries: ['index.html'] },
  // Same-origin BFF: the browser calls /api on the dev origin; Vite forwards to
  // the gateway server-side, so Better Auth's session cookie stays first-party.
  // The proxy target is a server-only (non-VITE_) var so it is NEVER baked into
  // the client bundle — otherwise the browser would try to hit the gateway's
  // Docker service name directly and fail (ERR_NAME_NOT_RESOLVED). In-container
  // it must be the gateway service name; host-run `pnpm dev` falls back to
  // 127.0.0.1:3185 (the host-mapped gateway port).
  //
  // Docker Desktop does not always deliver host edits through inotify.
  // Restart the frontend if an edit is stale; polling this large bind mount
  // is deliberately disabled. Bound both dependency discovery and watching
  // so unrelated app builds and package caches cannot starve HTTP startup.
  server: {
    watch: {
      // Ignore directory roots so chokidar prunes unrelated trees before
      // walking them. These are separate apps, generated data, or package
      // caches, not inputs to this SPA; linked packages remain watched.
      ignored: [
        '**/.pnpm-store/**',
        '**/.claude/**',
        '**/apps/gateway/**',
        '**/apps/verevon-web/**',
        '**/apps/remote-dev/**',
        '**/test-results/**',
      ],
    },
    proxy: sceneCaptureMode ? {} : {
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
    // Solid's lazy() also raises a rejected loader on a derived promise the boundary cannot see. This one
    // sentinel is the stale-deploy case in RegionBoundary.test.tsx, which the boundary does handle (the
    // test asserts it); every other unhandled error still fails the run.
    onUnhandledError(error) {
      // Duck-typed: the error comes from jsdom's realm, where `instanceof Error` is false.
      if (String((error as { message?: unknown } | null)?.message ?? '').includes('/assets/Page-x.js')) return false
    },
    // Playwright specs live under tests/e2e and import @playwright/test — they
    // must never be collected by vitest (they run via `pnpm test:e2e`).
    exclude: [
      ...configDefaults.exclude,
      'tests/e2e/**',
      // This gateway coverage gate intentionally uses Node's native test runner
      // and is executed explicitly by `pnpm test` before the Vitest suite.
      'apps/gateway/scripts/**/*.test.mjs',
      // node:test files run outside Vitest: the Frontend Plane deploy-contract tests
      // (see `pnpm test`) and the bundle-budget gate (`node --test` in CI).
      'scripts/**/*.test.mjs',
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
      // verevon-web is React (Next). This config compiles every .tsx with the
      // Solid JSX transform, so a React component test cannot render here; those
      // run from apps/verevon-web/vitest.config.mts (`pnpm --filter verevon-web test`).
      'apps/verevon-web/**/*.test.tsx',
    ],
    // Default per-test budget. The A8 fabrication-guard test spins up the real
    // ESLint flat config (cold-start ~8s), which exceeds vitest's 5s default when
    // that file runs in isolation; 30s keeps the suite stable on CI / slow hosts.
    testTimeout: 30_000,
  },
})
