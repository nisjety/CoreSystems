/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	readonly VITE_FLEET_CONSOLE?: string;
	readonly VITE_AGENT_BUILDER?: string;
	readonly VITE_BROWSER_OMNIBOX_SEARCH?: string;
	/** "1" shows the composer's `@tab` chips and sends `browserContext` (E3.3-c). Default off. */
	readonly VITE_BROWSER_TAB_MENTIONS?: string;
	/** "1" mounts the shared approval card in BrowserChrome and the chat deck (E3.2-b swap). Default off. */
	readonly VITE_BROWSER_APPROVAL_CARD?: string;
	/** "1" compiles in the product-scene runtime (src/demo). Never set in production. */
	readonly VITE_SCENE_MODE?: string;
	readonly VITE_PLAN_EXECUTE_ENABLED?: string;
	readonly VITE_CHAT_PROJECTS_ENABLED?: string;
	readonly VITE_CHAT_VISION_AS_PARTS_ENABLED?: string;
	readonly VITE_QUOTA_READOUT?: string;
	readonly VITE_QUOTA_READOUT_METRIC?: string;
	/** "1"/"0" forces the Space command palette on/off; unset is on in dev only (WS13-S6). */
	readonly VITE_SPACE_COMMAND_PALETTE_UI?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

/** Compile-time build id from vite.config.ts `define` (`dev` outside an image build). */
declare const __VEREVON_BUILD_ID__: string;
