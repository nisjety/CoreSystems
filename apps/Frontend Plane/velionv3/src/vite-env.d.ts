/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_VELION_GATEWAY_URL?: string
  readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string
  readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
