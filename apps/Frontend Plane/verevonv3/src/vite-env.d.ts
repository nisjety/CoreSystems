/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly VITE_VEREVON_GATEWAY_URL?: string;
	readonly VITE_ALLOW_DEV_ACTOR_HEADERS?: string;
	readonly VITE_ALLOW_DEV_AUTH_BYPASS?: string;
	readonly VITE_TURNSTILE_SITE_KEY?: string;
	readonly VITE_FLEET_CONSOLE?: string;
	/** "true" shows the customer-gateway settings section and the "Your gateway" model group (W4-BYO-6). Default off. */
	readonly VITE_BYO_GATEWAY?: string;
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
	/** "true" offers a read-only side question about the open conversation, in a drawer that can be spun off into a new draft chat (F-t3-069). Default off. */
	readonly VITE_CHAT_SIDE_CHAT?: string;
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
	/** "true" lets an MCP app frame take the server's network policy once the client re-validates it (W4-M2). Default off. */
	readonly VITE_MCP_APPS_NETWORK?: string;
	/** "true" lets an inline widget quote a passage of its own text as a chip the person clicks into the composer (EP-4 W4-IO-2). Default off. */
	readonly VITE_WIDGET_SELECTION?: string;
	/** "true" draws a generated image's tool call as an image card: the model and provider that made it, and any typed refusal (W4-IMG-4). Default off. */
	readonly VITE_CHAT_IMAGE_TOOL?: string;
	/** "true" mounts the changed-files card under each answer: the files the run saved to its Space workspace (F-t3-076). Default off. */
	readonly VITE_CHAT_CHANGED_FILES?: string;
	/** "true" shows the image results of a settled web-search answer under the answer, each linked to its page (F-t3-044, W4-IO-1). Default off. */
	readonly VITE_CHAT_IMAGE_STRIP?: string;
	/** "true" mounts the shell toast stack and lets `showToast` raise notices from any route (F-t3-078). Default off. */
	readonly VITE_SHELL_TOASTS?: string;
	/** "true" words a budget, quota, run-ceiling or rate refusal by the reader's role, so a member never sees the organisation's spend (F-t3-078). Default off. */
	readonly VITE_CHAT_LIMIT_NOTICES?: string;
	/** "true" lets two bare Escape presses within 600 ms stop the streaming chat answer (F-t3-077). Default off. */
	readonly VITE_CHAT_DOUBLE_ESCAPE_STOP?: string;
	/** "true" shows the Fast chip under the composer on a model that declares the priority tier, so the turn asks for speed priority (W5-095-b). Default off. */
	readonly VITE_CHAT_SPEED_TIER?: string;
	/** "true" mounts the catalogue overlay editor (hide, order, aliases) on the router-policy page (F-t3-065). Default off. */
	readonly VITE_CATALOGUE_OVERLAY?: string;
	/** "true" offers importing a conversation export as a new conversation, beside the export on the latest answer (EP-12 S15). Default off. */
	readonly VITE_THREAD_IMPORT?: string;
	/** "true" shows the plugin supply-chain controls in settings: source pin, manifest digest, catalogue approval, kill state and the install consent dialog (F-t3-059, W4-PL-4). Default off. */
	readonly VITE_PLUGIN_LOADER?: string;
	/** "true" shows the browser walkthrough in the run proof bundle: one filmstrip frame per browser action, from the screenshots the run recorded (F-t3-050, W4-R3-b). Default off. */
	readonly VITE_RUN_PROOF_WALKTHROUGH?: string;
	/** "true" shows the run's support reference (run id and request id, copyable) and a download of its support bundle beside the proof bundle (W5-096-b). Default off. */
	readonly VITE_RUN_SUPPORT_BUNDLE?: string;
	/** "true" sends a content-free long-task beacon: the build id and one duration bucket, at most once a minute per bucket (F-t3-096). Default off. */
	readonly VITE_LONG_TASK_BEACON?: string;
	/** "true" shows the chat connection pill above the composer when the chat is offline or cannot reach its service, after a short hold (F-t3-078). Default off. */
	readonly VITE_CHAT_CONNECTION_PILL?: string;
	/** "true" shows an environment pill in the navbar when the gateway reports a non-production environment (F-t3-098). Default off. */
	readonly VITE_ENV_PILL?: string;
	/** "true" applies the organisation accent, solved for contrast in OKLCH, as `--org-accent` and `--org-accent-on` (F-t3-098). Default off. */
	readonly VITE_ORG_ACCENT?: string;
	/** "true" mounts the organisation permission rules section in Customize: deny or ask rules on a tool, for a web domain, a mail recipient's domain or a command prefix (W5-074-c). Default off. */
	readonly VITE_PERMISSION_RULES?: string;
	/** "true" mounts the share dialog for conversation snapshots beside the export, and the `/chat/shared/:snapshotId` viewer page a granted member opens (W4-SH-6). Pairs with the gateway's `CHAT_SNAPSHOTS_ENABLED`. Default off. */
	readonly VITE_CHAT_SNAPSHOTS?: string;
	/** "true" mounts the Knowledge view that reviews the wiki proposals agents file: accept saves the proposal, reject writes nothing (W5-094-b). Default off. */
	readonly VITE_WIKI_PROPOSALS_REVIEW?: string;
	/** "true" offers "Transcribe to document" on an audio attachment: the recording's text is saved as a private transcript document (W5-090-b). Default off. */
	readonly VITE_CHAT_MEDIA_TRANSCRIPTS?: string;
	/** "true" mounts the event triggers section under the automations schedule list: a webhook event can start a run of one of the person's own approved schedules, with a dry run. Hidden for zero-retention organisations. Default off. */
	readonly VITE_EVENT_TRIGGERS?: string;
	/** "true" mounts the custom connector section in workspace settings: a stored key for a REST API or a static-token MCP server, sealed by the integration service (W4-CC-8). Hidden for zero-retention organisations. Default off. */
	readonly VITE_CUSTOM_CONNECTORS?: string;
	/** "true" offers Review on a text artifact in a chat: one read-only review of the revision on screen, shown as a card below it. Never offered in a temporary chat. Default off (F-t3-086, W5-086-b). */
	readonly VITE_REVIEW_RUN?: string;
	/** "true" offers Compare models on the Agents playground: two to four catalogue models answer one question with no tools, after the cost is shown (F-t3-092, W5-092-b). Default off. */
	readonly VITE_MODEL_COUNCIL?: string;
	/** "true" offers "Save these steps as a skill" after a browser give-back in a live run, for a turn that keeps content (F-t3-081, W5-081-d). Default off. */
	readonly VITE_TEACH_MODE?: string;
	/** "true" offers Translate on a turn's reasoning in the Details popover, for a turn that keeps content and reasoning in the other language from the reader's (F-t3-098, W5-098-c). Default off. */
	readonly VITE_REASONING_TRANSLATE?: string;
	/** "true" offers a Problem mode in the feedback widget: the person ticks which content-free diagnostics go with the note (recent error ids, the app build, the chat run; never a transcript). Default off (W5-096-a). */
	readonly VITE_REPORT_PROBLEM?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}

/** Compile-time build id from vite.config.ts `define` (`dev` outside an image build). */
declare const __VEREVON_BUILD_ID__: string;
