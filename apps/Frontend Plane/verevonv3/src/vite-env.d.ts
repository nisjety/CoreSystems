/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	readonly VITE_PLAN_EXECUTE_ENABLED?: string;
	readonly VITE_CHAT_VISION_AS_PARTS_ENABLED?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

/** Compile-time build id from vite.config.ts `define` (`dev` outside an image build). */
declare const __VEREVON_BUILD_ID__: string;
