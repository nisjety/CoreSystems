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
	/** "true" draws a tool call's terminal output and workspace file in the computer panel's viewers (EP-1 W4-B P1). Default off. */
	readonly VITE_CHAT_COMPUTER_PANEL?: string;
	/** "true" folds settled agent turns to one row (EP-14 S1). Default off. */
	readonly VITE_CHAT_SETTLED_FOLD?: string;
	/** "true" shows the reader's outline and dyslexia-friendly font, and applies the stored text size (F-t3-098). Default off. */
	readonly VITE_CHAT_READER_POLISH?: string;
	/** "true" shows thumbs and reasons on agent run answers and scheduled results (F-t3-078). Default off. */
	readonly VITE_AGENT_RESULT_FEEDBACK?: string;
	readonly VITE_QUOTA_READOUT?: string;
	readonly VITE_QUOTA_READOUT_METRIC?: string;
	/** "1"/"0" forces the Space command palette on/off; unset is on in dev only (WS13-S6). */
	readonly VITE_SPACE_COMMAND_PALETTE_UI?: string;
	/** "true" shows row checkboxes and "Ingest selected" on a run's fetched pages (re-fetch, then ingest). Default off. */
	readonly VITE_INGEST_SELECTED?: string;
	/** "true" offers `/learn` in the composer's slash menu: a request to learn from a saved conversation (EP-11 S7). Default off. */
	readonly VITE_LEARN_COMMAND?: string;
	/** "true" lets the agent presets carry owner actions the org admits (EP-6 S11). Default off. */
	readonly VITE_PRESET_AGENT_ACTIONS?: string;
	/** "true" shows "What this agent remembers" on each agent a room owner or admin governs (EP-6 P3-c). Default off. */
	readonly VITE_SPACE_AGENT_MEMORY?: string;
	/** "true" shows the Space memory tab (EP-8 S7) in a shared room's details, to its owner or a manager. Default off. */
	readonly VITE_SPACE_MEMORY_TAB?: string;
	/** "true" makes the MCP add flow probe a server first, then offer a tool picker before it is saved (W4-CC-2). Default off. */
	readonly VITE_MCP_PROBE?: string;
	/** "true" lets an inline widget quote a passage of its own text as a chip the person clicks into the composer (EP-4 W4-IO-2). Default off. */
	readonly VITE_WIDGET_SELECTION?: string;
	/** "true" draws a generated image's tool call as an image card: the model and provider that made it, and any typed refusal (W4-IMG-4). Default off. */
	readonly VITE_CHAT_IMAGE_TOOL?: string;
	/** "true" mounts the changed-files card under each answer: the files the run saved to its Space workspace (F-t3-076). Default off. */
	readonly VITE_CHAT_CHANGED_FILES?: string;
	/** "true" mounts the shell toast stack and lets `showToast` raise notices from any route (F-t3-078). Default off. */
	readonly VITE_SHELL_TOASTS?: string;
	/** "true" lets two bare Escape presses within 600 ms stop the streaming chat answer (F-t3-077). Default off. */
	readonly VITE_CHAT_DOUBLE_ESCAPE_STOP?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

/** Compile-time build id from vite.config.ts `define` (`dev` outside an image build). */
declare const __VEREVON_BUILD_ID__: string;
