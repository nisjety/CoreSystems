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
	/** "true" offers dock position choices (bottom/right/left) for the GlobalAssistant dock. Default off. */
	readonly VITE_ASSISTANT_DOCK_POSITION?: string;
	/** "true" shows the browser tab context menu (select/new/close/open-outside). Default off. */
	readonly VITE_BROWSER_TAB_MENU?: string;
	/** "true" shows the bookmarks star and bar in BrowserChrome. Default off. */
	readonly VITE_BROWSER_BOOKMARKS?: string;
	/** "true" enables annotation mode on the live browser view. Default off. */
	readonly VITE_BROWSER_ANNOTATE?: string;
	/** "true" shows zoom controls and a zoom-scaled live view. Default off. */
	readonly VITE_BROWSER_ZOOM?: string;
	/** "true" enables browser keyboard commands (new/close/focus/run). Default off. */
	readonly VITE_BROWSER_KEYMAP?: string;
	/** "true" shows the new-tab start page. Default off. */
	readonly VITE_BROWSER_NEWTAB?: string;
	/** "true" offers the split chat/browser workspace view. Default off. */
	readonly VITE_WORKSPACE_SPLIT?: string;
	/** "true" offers a floating assistant composer. Default off. */
	readonly VITE_ASSISTANT_FLOATING?: string;
	/** "true" redacts secret-looking text in assistant submissions. Default off. */
	readonly VITE_ASSISTANT_REDACT?: string;
	/** "true" shows interactive-only element filtering. Default off. */
	readonly VITE_BROWSER_ELEMENT_FILTER?: string;
	/** "true" shows the in-page find overlay (searches loaded DOM text). Default off. */
	readonly VITE_BROWSER_FIND?: string;
	/** "true" shows the replay slider and auto-play over the browser timeline (F-t3-050 R1). Default off. */
	readonly VITE_BROWSER_REPLAY_SCRUBBER?: string;
	/** "1" compiles in the product-scene runtime (src/demo). Never set in production. */
	readonly VITE_SCENE_MODE?: string;
	readonly VITE_PLAN_EXECUTE_ENABLED?: string;
	readonly VITE_DEEP_RESEARCH_PLAN_TURN?: string;
	readonly VITE_CHAT_PROJECTS_ENABLED?: string;
	readonly VITE_CHAT_VISION_AS_PARTS_ENABLED?: string;
	/** "true" folds settled agent turns to one row (EP-14 S1). Default off. */
	readonly VITE_CHAT_SETTLED_FOLD?: string;
	/** "true" shows thumbs and reasons on agent run answers and scheduled results (F-t3-078). Default off. */
	readonly VITE_AGENT_RESULT_FEEDBACK?: string;
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
