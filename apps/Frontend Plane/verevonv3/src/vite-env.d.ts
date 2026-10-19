/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	readonly VITE_PLAN_EXECUTE_ENABLED?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}
