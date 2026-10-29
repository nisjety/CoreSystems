# Scene contract v1 (binding for every agent working on scenes 02 and 03)

Read PLAN_2026-10-11.md first. This file amends it. Where they differ, THIS FILE WINS.

## 0. Repo layout and hard safety rules

- Real checkout: `C:/dev/CoresSystem/apps/Frontend Plane/verevonv3` (call it SPA). Marketing site: `SPA/apps/verevon-web` (call it WEB). Both on branch `main`, shared with other live sessions.
- The SPA tree has uncommitted edits from OTHER sessions. NEVER touch these files (other sessions own them):
  `src/features/chat/components/{ChatMessages.tsx,chat-normalizers.ts,chat-types.ts,use-chat-controller.ts,ChatMessages.failure-actions.test.tsx}`,
  `src/features/agents/components/AgentRunConsole*`, `src/features/spaces/components/SpaceRoomComposer*`,
  `src/shared/api/{chat-client.ts,http.ts,sse.ts}`, `src/shared/i18n/errors.ts`, `src/styles/global.css`.
  You may READ them. You may not edit them.
- NEVER run: git checkout, git stash, git reset, git clean, git commit, git add, git push. No commits. Everything stays uncommitted.
- NEVER edit existing SPA files except these three, and only with the smallest additive, build-flag-guarded change:
  `src/index.tsx` (guarded dynamic import of the scene runtime), `src/app/App.tsx` (guarded route), `src/vite-env.d.ts` (env typing).
  All other SPA work is NEW files under `src/demo/**` and `scripts/scene-*.mjs`.
- The scene runtime exists only when `import.meta.env.VITE_SCENE_MODE === '1'`. A normal `vite build` must contain none of it (no `__verevonScene`, no `src/demo` chunk). Guard with a literal `import.meta.env.VITE_SCENE_MODE === '1'` so Vite dead-code-eliminates it.
- Do not use the shared Browser pane tools (mcp__Claude_Browser__*). Other agents run in parallel and the pane is shared. Drive browsers with Playwright scripts (node; `@playwright/test` is installed in SPA/node_modules, Chromium 1243 is installed under ~/AppData/Local/ms-playwright). Headless.
- Dev servers: run host-side Vite with `cd SPA && VITE_SCENE_MODE=1 pnpm exec vite --host 127.0.0.1 --port <PORT> --strictPort`. Do NOT use ports 5173 (Docker app), 3100 (Next dev), 3185. Port assignments: runtime author 5211, capture-harness author 5212, scene-03 author 5213, scene-02 film producer 5214, scene-03 film producer 5215 (critics need no server), capture runs without --url use 5216. The site-wiring author uses WEB's own dev server on 3100 (already running; do not restart it; if it is not running, start `pnpm dev -p 3100` in WEB).
- Scratch directory for frames, screenshots, logs (NEVER inside the repo): `C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-OneDrive---Aquatiq-Dokumenter-CoresSystem-apps-Frontend-Plane-verevonv3/47011997-364a-4f6f-9d55-00ca4d86dbf2/scratchpad`. Use a per-agent subfolder.
- Windows host, Git Bash available. ffmpeg and ffprobe are on PATH. Prefer forward slashes. Do not run Docker commands. Do not restart or rebuild anything in Docker.
- GDPR: no real person, customer, order, e-mail address, company or token may appear in any frame. The real signed-in user is "Ima" at "Aquatiq AS". Those strings, "Velion", "Aquatiq", and any e-mail other than `@fjordform.example` must never be visible. Everything shown comes from `src/demo/fixtures/**`.
- All visible text Norwegian bokmål (nb-NO), including time and date formats. Fixed fictional date: Friday 9 October 2026 12:00 Europe/Oslo ("i dag").

## 1. Film spec

- Each film is 8.000 s of content, 60 fps, output 1280x800 (16:10), silent, h264 + vp9 + webp poster, seamless loop.
- LOOP DESIGN: the harness renders exactly 480 frames (8.000 s). The scene holds its final state (approval button hovered) from about 7.4 s to 8.0 s. The encoder then dissolves the last 30 frames (450..479) linearly into the first 30 frames (0..29) of the same render, so output frame 479 equals render frame 29, which is the static resting state, and equals output frame 0. The first 0.5 s of every scene is therefore completely static (resting composer, nothing animating, cursor hidden). Do not invent an in-page reset animation.
- POSTER: a webp of the completed result state (render frame 420, t = 7.0 s), not the opening frame, because it is what a reduced-motion visitor sees. Override with --poster-frame.
- Output files: `WEB/public/product-scenes/<id>.mp4`, `<id>.webm`, `<id>-poster.webp`. Ids: `02-plan`, `03-undersok`.
- Motion: 250-450 ms ease-out (cubic-bezier(0.16, 1, 0.3, 1) or 1-(1-p)^3), slides 8-16 px with fade, no bounce, no overshoot. Cursor moves 350-600 ms ease-out. Static camera, no zoom.
- AMENDMENT A (crop, overrides the plan's "full chat shell"): the films show a focused SLICE of the real dashboard, not the whole application. Render the real chat WORKSPACE: the conversation column (messages + composer) and the right-hand work surface (Resultat / steps / sources). Hide the global left icon rail, the top bar and the search bar, and any feedback button, appearance preview or dev badge. Real components, real CSS, real tokens, real icons. Only the surrounding page chrome is omitted. Keep a faint rounded app-card edge so it reads as a window slice. A tile on the website is only ~700 CSS px wide, so the UI must be large: choose the SMALLEST CSS viewport (16:10, width between 960 and 1280) at which the real layout still shows conversation and right surface side by side without collapsing. Record it in `src/demo/scene-config.json` as `{ "viewport": { "width": W, "height": H } }`. The harness captures at device scale factor 2560/W so the raw frame is 2560x1600, then downsamples to 1280x800.
- AMENDMENT B (aspect): the site frame for stages 02 and 03 becomes 16:10 so nothing is cropped. Keep all essential UI inside the central safe area (inset 3% sides, 4% top and bottom).
- AMENDMENT C (loop): the last content state is the hovered approval button. Frame 0 is the resting state with the composer prefilled. Between them the encoder crossfade bridges. Do not invent an in-page reset animation.
- AMENDMENT D (scene 02 real-component mapping, 2026-10-11 round 2): where PLAN section 2 describes UI the real components render differently, scene 02 shows the real rendering. (1) Source chips: the three sources are the real source cards in the Kilder canvas (opened by the controller on the first `citation`), not chips under the bubble. (2) Status line: the assistant placeholder reads the product's «Tenker»; the product has no «Sjekker kilder» label, and a `tool_call` to produce an activity line would open Arbeid instead of Kilder. (3) Approval target: the real approval card under the answer («Godkjenning · Forbered oppfølging», buttons «Godkjenn» / «Avvis»), fetched by the controller when a step pauses with `awaiting_approval`; the hovered button is «Godkjenn». The right panel's plan rows have no button, because no real component renders one there. (4) Viewport: scene 02 is captured at 1024x640 (search recorded in `src/demo/README.md`). (5) The right work surface's column is reserved from frame 0 with the canvas's own surface (scene CSS), so the conversation column never reflows when the canvas mounts.

## 2. Runtime contract: `window.__verevonScene`

Installed by `src/demo/ScenePage.tsx` when route `/scene/:id` mounts (the route only exists when VITE_SCENE_MODE=1). TypeScript source of truth: `src/demo/scene-types.ts`, which the runtime author creates FIRST and must match this text exactly.

```ts
interface VerevonScene {
  readonly id: string;                       // "02-plan" | "03-undersok"
  readonly durationMs: number;               // 8000
  readonly viewport: { width: number; height: number };   // CSS px, from scene-config.json
  // Resolves when: fixtures installed, route mounted, document.fonts.ready, two rAFs after first paint,
  // composer prefilled, UI is exactly at frame 0. The timeline has NOT started.
  readonly ready: Promise<void>;
  // t = 0 now. Schedules every timeline event with setTimeout relative to this call. Throws if called twice.
  start(): void;
  // Where the pointer is at scene time tMs. Pure function of tMs and current layout (targets resolved lazily by selector).
  pointerAt(tMs: number): { x: number; y: number; visible: boolean; pressed: boolean };
  // Draws the in-page cursor overlay at CSS px (x,y). Called by the harness each frame.
  setCursor(x: number, y: number, visible: boolean, pressed: boolean): void;
  // Chronological log of fired events: { atMs, kind, note }. For assertions.
  readonly log: ReadonlyArray<{ atMs: number; kind: string; note: string }>;
}
```

Harness protocol (so :hover and click are genuine):
1. `page.goto(url)` with viewport and device scale factor set, then `await page.evaluate(() => window.__verevonScene.ready)`.
2. Pause virtual time (`Emulation.setVirtualTimePolicy` pause), call `start()`.
3. For frame i = 0..479: t = i*1000/60 ms. Evaluate `pointerAt(t)`. Dispatch `Input.dispatchMouseEvent` mouseMoved to (x,y). On pressed false to true dispatch mousePressed (left, clickCount 1). On true to false dispatch mouseReleased. Call `setCursor(...)`. Advance virtual time by 16.6667 ms, wait for it to expire, screenshot.
4. The runtime's own timers (setTimeout only; never Date.now-based scheduling) and the SSE stream must advance under paused/advancing virtual time. The mock `fetch` streams the chat answer with setTimeout-scheduled chunk enqueues relative to the instant the intercepted POST arrives.
5. CSS transitions and animations must visibly advance per frame. The harness author proves this with the synthetic acceptance page in section 4 and picks the working mechanism (CDP virtual time + screenshot, or Web Animations seeking, or a timer/rAF shim). The mechanism is the harness author's call, but the protocol above and determinism (section 4) are fixed.

Scene definitions live in `src/demo/scenes/<id>.ts`, export a `SceneDefinition` (type in `scene-types.ts`) and are registered in `src/demo/scenes/index.ts`. Fixtures live in `src/demo/fixtures/` (shared base `fjordform.ts`; scene 03 additions in `fjordform-undersok.ts`).

## 3. How fixtures enter (no component edits)

- Install a `window.fetch` interceptor before the app renders (from the guarded hook in `src/index.tsx`). It answers every `/api/*` request from fixtures: auth session (`/api/v1/auth/session`), `/api/v1/session/current`, `/api/v1/navbar`, `/api/v1/models`, `/api/v1/integrations/connections`, composer settings, thread context, knowledge, memory, org GDPR status, and so on. Unknown `/api/*` requests return an empty but shape-valid body and are logged to `window.__verevonScene.log` with kind `unmatched-api` so they can be reviewed. Non-API requests pass through.
- The chat answer comes from the real transport: `POST /api/v1/chat/stream` answered with a `text/event-stream` body of the real named events (`connected`, `chunk`, `grounding`, `citation`, `step`, `artifact`, `tool_call`, `tool_result`, `plan_proposed`, `done` and so on; see `dispatchEvent` in `src/shared/api/chat-client.ts` for the exact payload shapes).
- Run-console style events (sub-agents, browser observation, approval pause) enter through the same fetch/SSE endpoints the components already call (`src/shared/api/run-console-client.ts`, `browser-run-client.ts`).
- Fallback ladder for scene 03 ONLY, if a real panel cannot be driven by fixtures without editing components: (1) wide-research panel + live-run browser frame, (2) the steps surface with tool_call/tool_result events plus a compact evidence block, (3) artifact-only. Document the rung used in the scene file header and in the agent report. Never fake it with hand-drawn HTML that is not a real component.
- Language: the scene sets the app language to Norwegian before first render through whatever mechanism the app uses (check `src/shared/i18n`). Prefill the composer by setting the real textarea value and dispatching a real `input` event.

## 4. Capture harness acceptance (author: harness agent)

Script: `WEB/scripts/capture-scene.mjs`, helpers under `WEB/scripts/scene-capture/`. CLI: `node scripts/capture-scene.mjs --scene 02-plan [--url http://127.0.0.1:5211] [--out <dir>] [--keep-frames] [--frames 480] [--poster-frame 420] [--viewport WxH]`. Without `--url` it starts the SPA dev server itself (port from `--port`, default 5216), waits for it, and stops it afterwards. It pipes PNG frames into ffmpeg through stdin (no 1 GB of frames on disk) unless `--keep-frames`. It always writes every 30th raw frame as PNG into `<out>/samples/` for review, plus `<out>/samples/text-<frameIndex>.txt` holding document.body.innerText at that frame (used by reviewers to scan for English leakage and forbidden real-data strings). It writes `<out>/<id>.mp4`, `.webm`, `-poster.webp`, `<out>/frame-hashes.json`, `<out>/capture-report.json`.

Proof the harness is correct, on a synthetic page the harness author writes (NOT part of the repo; keep it in the scratch dir, serve it with a tiny node static server) that mimics the contract (`__verevonScene` with ready/start/pointerAt/setCursor/log):
1. A 300 ms ease-out CSS transform transition triggered by a setTimeout at 500 ms: the sampled progress must be monotonic and span 18 +/- 1 frames; the class change appears at frame 30 +/- 1.
2. A button with a `:hover` style that the pointer reaches at 400 ms: the hover style is visible from the first frame the pointer is inside it.
3. A fetch() answered by a setTimeout-driven ReadableStream emitting 3 chunks at 200/400/600 ms: DOM updates at frames 12/24/36 +/- 1.
4. Determinism: two consecutive runs produce identical sha256 for every frame (log how many differ; target 0; if sub-pixel text or GPU noise makes a few differ, state it and the cause).
5. Output checks with ffprobe: 480 frames, 60 fps, 1280x800, yuv420p, h264 and vp9, poster is webp. Seam: SSIM(output frame 479, output frame 0) >= 0.995, and the dissolve is linear over exactly frames 450..479.
Also report seconds per frame, total wall time, and the exact Chromium flags used.

## 5. Site wiring acceptance (author: wiring agent)

Files: `WEB/src/components/home/sections/ProductLoopSection.tsx`, `ProductLoopProductDemos.tsx`, new tests next to them. NEVER edit the stage 01 / 04 / 05 content or the GSAP timeline logic beyond what is stated here.
- `loopStages[1]` (02 Plan) and `[2]` (03 Undersøk) become `kind: "media"` with `media: { kind: "video", src: "/product-scenes/<id>.mp4", webm: "/product-scenes/<id>.webm", poster: "/product-scenes/<id>-poster.webp", alt: <Norwegian description> }` and the new stage copy from PLAN section 2. Stage `loopStages[3]` (04 Følg opp) keeps `ProductDemo` for now.
- `ProductLoopVideo`: muted, playsInline, `loop`, `disablePictureInPicture`, `disableRemotePlayback`, `<source>` webm first then mp4, poster; object-fit contain over the frame background so nothing is cropped; plays only while its layer is visible and in viewport and the page is visible (existing logic); never calls `play()` when `prefers-reduced-motion: reduce` (poster stays); `preload="metadata"`; `aria-label` from alt.
- Frame markers for stages 02 and 03 become exactly 16:10 (use `aspect-[16/10]` with the existing width formulas; derive height) without overlapping their copy markers at viewports 1280x720, 1440x900, 1920x1080. Verify with Playwright screenshots of the pinned section at the three sizes.
- Mobile fallback (`ProductLoopMobileFallback`): stages 02/03 render the same video tile (aspect 16:10, rounded 20px), no autoplay off-screen.
- Existing tests in WEB must still pass: run the test command in WEB/package.json. Add tests for the new media entries and the reduced-motion rule.
- The wiring must tolerate the media files not existing yet (poster/video 404 must not throw; the layer just shows the frame background). The final check with real media happens after the films exist.

## Amendment F (2026-10-11, round 5): context-aware start, Bring, invoice story

Binding for scenes 02 and 03. Overrides the beat sheets in PLAN_2026-10-11.md section 2 and the fixture text in section 4 where they differ.

- **Start state.** The film opens on the product's own centred chat start screen: brand mark, greeting, composer, and (for the context-aware assistant) one prompt suggestion in place of the general welcome text. No work surface, no empty right column, no "Ingen steg ennå" / "Ingen kilder ennå". The work surface opens only when there is real evidence to show, and it grows in from the right edge instead of appearing in one frame. The general welcome text («Spør om noe dere jobber med. …») is for a fresh start with nothing to report and does not appear in these films.
- **Suggestion.** Each suggestion says what happened, shows its source and a time, and proposes a next action. A click fills the composer so the person can read and edit it; it never sends and never acts. The person then sends it.
- **Scene 02.** Starts on a fictional delay notice from Bring about the Nordlys delivery. Suggestion: «Bring melder at Nordlys-leveransen er forsinket. Skal jeg sjekke konsekvensene og lage en plan for oppfølging?». Tracking status comes from **Bring**, not from Visma.net. Then status with sources, next steps, and an approval gate before anything is sent on.
- **Scene 03.** Starts on a fictional notice that an invoice deviates from the agreed price. Suggestion: «Denne fakturaen ser ut til å avvike fra avtalen. Skal jeg undersøke hvorfor?». Verevon sets agreement, order and invoice side by side, explains the deviation with traceable sources and prepares a credit note. It ends on human approval. The story is a customer problem (Fjordform's invoice 20471 to Nordlys Handel AS written at list price), not a fault in Verevon. This replaces the export/mapping story.
- **Fictional organisation.** All names, amounts and dates come from `src/demo/fixtures/**` (Fjordform AS, Nordlys Handel AS, ordre 4821, faktura 20471). The real organisation's name never appears.
- **Product dependency.** The suggestion card is a film-side build (`src/demo/suggestions.ts`) from the product's own starter markup and tokens, because the product's `EmptyChatState` was still being extended with its own activity suggestions (uncommitted, another session) and could not yet show source and time or the brief's wording. When the product's card does, switch the films to drive it through fixtures for `GET /api/v1/notifications` and `GET /api/v1/inbox/conversations` and delete `suggestions.ts`.
