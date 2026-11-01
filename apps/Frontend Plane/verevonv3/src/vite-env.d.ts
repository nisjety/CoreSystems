/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	readonly VITE_FLEET_CONSOLE?: string;
	readonly VITE_BROWSER_OMNIBOX_SEARCH?: string;
	/** "1" compiles in the product-scene runtime (src/demo). Never set in production. */
	readonly VITE_SCENE_MODE?: string;
	readonly VITE_PLAN_EXECUTE_ENABLED?: string;
	readonly VITE_CHAT_PROJECTS_ENABLED?: string;
	readonly VITE_CHAT_VISION_AS_PARTS_ENABLED?: string;
	readonly VITE_QUOTA_READOUT?: string;
	readonly VITE_QUOTA_READOUT_METRIC?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}
