import { fileURLToPath, URL } from 'node:url'
import { createLogger, defineConfig, type LogOptions, type Logger } from 'vite'
import solid from '@solidjs/vite-plugin'

// The interactive demo (stage 04 of the product loop) has its own Vite build: a separate
// entry, its own public folder, no dev server, no proxy and no environment files.
// scripts/build-product-demo.mjs runs this config and publishes the output. Keep this file
// free of hosts, keys and personal data.

const demoRoot = fileURLToPath(new URL('./src/demo/product-demo', import.meta.url))

/**
 * Vite warns, and leaves the reference as it is, when a CSS url() or an asset path does not resolve at
 * build time. In the demo such a reference would load from the site root, outside /product-demo/, so the
 * build stops instead of publishing it.
 */
const UNRESOLVED_ASSET = /didn't resolve at build time/

function refusingLogger(): Logger {
  const base = createLogger('warn')
  const refuse = (message: string): void => {
    if (UNRESOLVED_ASSET.test(message)) throw new Error(`demo build refused: ${message.trim()}`)
  }
  // The base logger keeps its own state (hasWarned and the rest) through the prototype chain.
  return Object.create(base, {
    warn: {
      value: (message: string, options?: LogOptions): void => {
        refuse(message)
        base.warn(message, options)
      },
    },
    warnOnce: {
      value: (message: string, options?: LogOptions): void => {
        refuse(message)
        base.warnOnce(message, options)
      },
    },
  }) as Logger
}

export default defineConfig({
  root: demoRoot,
  // Set here, never on the command line: Git Bash rewrites a leading slash into a Windows path.
  base: '/product-demo/',
  // The SPA's .env files may hold dev-only switches (VITE_ALLOW_DEV_AUTH_BYPASS and similar).
  // They must never reach a public bundle, so the demo reads no environment files at all.
  envDir: false,
  customLogger: refusingLogger(),
  define: {
    'import.meta.env.VITE_DEMO': JSON.stringify('1'),
    'import.meta.env.VITE_SCENE_MODE': JSON.stringify('0'),
  },
  plugins: [solid()],
  resolve: {
    alias: [
      { find: /^mermaid$/, replacement: fileURLToPath(new URL('./src/demo/product-demo/mermaid-stub.ts', import.meta.url)) },
      // install.ts is shared with the films and must not change. The demo takes its approval from a copy of the
      // film fixture that differs only in that approval (see product-demo/fjordform-demo.ts).
      { find: /^\.\/fixtures\/fjordform$/, replacement: fileURLToPath(new URL('./src/demo/product-demo/fjordform-demo.ts', import.meta.url)) },
      { find: '@', replacement: fileURLToPath(new URL('./src', import.meta.url)) },
    ],
  },
  build: {
    outDir: fileURLToPath(new URL('./node_modules/.cache/product-demo/dist', import.meta.url)),
    emptyOutDir: true,
    sourcemap: false,
    manifest: true,
    modulePreload: { polyfill: false },
    chunkSizeWarningLimit: 700,
  },
})
