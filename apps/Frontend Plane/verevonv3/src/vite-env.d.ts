/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	/** "1" includes the fictional product-scene runtime; never enable in production. */
	readonly VITE_SCENE_MODE?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}
