import { fileURLToPath, URL } from 'node:url'
import { defineConfig, type Plugin, type PreviewServer, type ViteDevServer } from 'vite'
import solid from 'vite-plugin-solid'
import { createPulseProxyMiddleware, pulseProxyConfigFromEnvironment } from './studio-demo/pulse-local-gate.ts'

const projectRoot = fileURLToPath(new URL('.', import.meta.url))

function blockBackendRoutes(): Plugin {
  const proxyConfig = pulseProxyConfigFromEnvironment()
  const pulseProxy = proxyConfig ? createPulseProxyMiddleware(proxyConfig) : null
  const installBlocker = (server: ViteDevServer | PreviewServer) => {
    if (pulseProxy) server.middlewares.use(pulseProxy)
    server.middlewares.use((request, response, next) => {
      const pathname = new URL(request.url ?? '/', 'http://studio-demo.local').pathname
      if (pathname === '/api' || pathname.startsWith('/api/') || pathname === '/health' || pathname.startsWith('/health/')) {
        response.statusCode = 404
        response.setHeader('Content-Type', 'application/json; charset=utf-8')
        response.end(JSON.stringify({ error: { code: 'DEMO_ONLY', message: 'The local Studio demo has no backend.' } }))
        return
      }
      next()
    })
  }

  return {
    name: 'studio-demo-block-backend-routes',
    configureServer(server) {
      installBlocker(server)
    },
    configurePreviewServer(server) {
      installBlocker(server)
    },
  }
}

export default defineConfig({
  root: 'studio-demo',
  publicDir: fileURLToPath(new URL('./public', import.meta.url)),
  plugins: [solid(), blockBackendRoutes()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5174,
    strictPort: true,
    fs: {
      allow: [projectRoot],
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 5174,
    strictPort: true,
  },
  build: {
    outDir: '../dist-studio-demo',
    emptyOutDir: true,
  },
})
