# Product scenes (`src/demo`)

Development-only runtime that lets the **real chat workspace** play a scripted
8-second scene with no gateway and no sign-in, for the looping product films on
the marketing site. Binding spec: `apps/verevon-web/plans/product-scenes/CONTRACT.md`
(amends `PLAN_2026-10-11.md`).

Nothing in this folder ships. It is compiled in only when
`VITE_SCENE_MODE=1` (guards in `src/index.tsx`, `src/app/App.tsx`); a plain
`vite build` contains no `__verevonScene` and no `src/demo` chunk.

## Run

```sh
cd "C:/dev/CoresSystem/apps/Frontend Plane/verevonv3"
VITE_SCENE_MODE=1 pnpm exec vite --host 127.0.0.1 --port 5211 --strictPort
```

Then open `http://127.0.0.1:5211/scene/02-plan?autopilot=1` in a browser:
the scene starts by itself after `ready`, draws its cursor and performs the
click/hover with synthetic events. Without `?autopilot=1` nothing moves until a
driver calls `window.__verevonScene.start()` and dispatches the mouse itself
(that is how the capture harness and the preview script get a genuine
`:hover` and click).

## Preview at real time

```sh
node scripts/scene-preview.mjs --scene 02-plan --url http://127.0.0.1:5211 \
  --out "<scratch dir>/scene-preview" [--viewport 1280x800] [--scale 1]
```

Headless Chromium, locale `nb-NO`, timezone `Europe/Oslo`, fixed clock
2026-10-09 12:00. It waits for `ready`, calls `start()`, and every tick
evaluates `pointerAt(t)`, moves the real mouse, presses/releases on the pressed
edges and calls `setCursor`. Screenshots at t = 0, 1.0, 1.8, 2.6, 3.5, 4.4, 5.5,
6.3, 7.0, 7.8 s go to `--out`, with `document.body.innerText` beside each. It
prints the scene log, console errors, page errors, failed requests, every
`unmatched-api` entry, request counts per `/api` path, and fails if any
forbidden real-data string (`Ima`, `Aquatiq`, `Velion`, a non-`@fjordform.example`
e-mail) appears in any checkpoint. Exit code 0 means clean.

## Files

| File | Role |
|---|---|
| `scene-types.ts` | `VerevonScene` (the `window.__verevonScene` contract, verbatim from CONTRACT.md §2) and `SceneDefinition`. |
| `install.ts` | `fetch` interceptor installed before render. Answers every `/api/*` request from fixtures; plays the scene's SSE script for `POST /api/v1/chat/stream` through a `setTimeout`-driven `ReadableStream`; logs unknown `/api/*` paths as `unmatched-api`. |
| `fixtures/fjordform.ts` | The fictional world: Fjordform AS, Ida Berg, Nordlys Handel AS, order 4821. Scene 02: the Bring delay notice (the start-screen suggestion), the three sources (Bring tracking, customer agreement, e-mail), the status document, the plan rows, the approval. Norwegian only. |
| `fixtures/fjordform-undersok.ts` | Scene 03: the invoice notice (the start-screen suggestion), invoice 20471 against the framework agreement and order 4821 (120 stk at 1 380,00 against 1 240,00, a 16 800,00 kr difference), the three sub-tasks, the browser step, the comparison document, the «Opprett kreditnota» approval. |
| `fixtures/assets/faktura-20471.png` | The browser step's screenshot (fictional invoice in the accounting system, the unit price highlighted), drawn from `faktura-20471.source.html`. |
| `suggestions.ts` | The context-aware start screen (CONTRACT amendment F): builds the prompt suggestion card from the product's own starter markup and inserts it into the real start screen in place of the welcome text. A click only fills the composer; it never sends. |
| `choreography.ts` | Softens the two layout changes the product makes in a single frame: the composer gliding from the middle of the start screen to its dock, and the work surface growing in from the right edge. Web Animations on the real elements, driven by `document.timeline`. |
| `scene-timeline.ts` | `createSceneScheduler` (setTimeout only), `pointerAt` (pure: keyframes + time + lazy target resolver), `resolveSceneTarget`. Unit-tested in `scene-timeline.test.ts`. |
| `ScenePage.tsx` | Route `/scene/:id`. Mounts the real `ChatPage`, applies the `/chat` appearance tokens to the shell, prefills the composer through the real textarea + `input` event, resolves `ready` at frame 0, installs `window.__verevonScene`. |
| `SceneCursor.tsx` | 20 px arrow overlay with a soft shadow, driven by `setCursor`. |
| `scene.css` | Hides the global rail, top bar, feedback widget and appearance preview under `html.verevon-scene`; keeps the rounded app-card edge; no scrollbars. Never edits `global.css`. |
| `scene-config.json` | `viewport`: the default CSS viewport (1280x800) that `__verevonScene.viewport` reports and the harness uses without `--viewport`. `scenes.<id>.viewport`: the width chosen for that scene under CONTRACT amendment A (see "Viewport" below). |

## Viewport (CONTRACT.md amendment A)

Amendment A asks for the smallest 16:10 CSS width between 960 and 1280 at
which the real workspace still shows the conversation and the right work
surface side by side. Searched for scene 02 on 2026-10-11 (real time,
`?autopilot=1`, widths 960 to 1280 in 32 px steps, height = width x 0.625,
layout measured at t = 7.4 s):

| Width | Canvas | Conversation column | Composer |
|---|---|---|---|
| 960x600 | 440 px, docked | 490 px | model label clipped to «Verev…» under the context chip; the thread title runs into the «Resultat» button |
| 992x620 | 440 px, docked | 522 px | model label clipped («Verevon Bal…») |
| **1024x640** | **440 px, docked** | **554 px** | **everything fits; the product's own compact toolbar (icons only)** |
| 1056 to 1280 | 440 px, docked | 586 to 810 px | fits |

The canvas never collapses in this range (it becomes a sheet only below
760 px), so the limit is the composer: below 1024 the real model label is cut.
**Scene 02 is captured at 1024x640**, device scale factor 2560/1024 = 2.5, so
the raw frame is still 2560x1600 and the UI renders 25 % larger in the 1280x800
film than at 1280 CSS px. Recorded as `scenes["02-plan"]` in
`scene-config.json`.

**Scene 03 is also captured at 1024x640** (searched 2026-10-11 round 3,
measured at t = 3.5 s with the log frame open and t = 7.4 s with the approval
card): at 960 the live split and the Resultat canvas still sit side by side
with the conversation, but the limit is the same composer as in scene 02 (its
model label is clipped below 1024, table above); at 1024 the
conversation column is 554 px, the canvas 440 px, and the whole final
exchange fits above the composer without scrolling. Recorded as
`scenes["03-undersok"]`. `ScenePage.tsx` now reports the per-scene value as
`__verevonScene.viewport` (falling back to the top-level 1280x800), so the
harness needs no `--viewport` flag for either scene.
| `scenes/02-plan.ts`, `scenes/03-undersok.ts`, `scenes/index.ts` | Scene definitions and the registry. |

## How a beat reaches the screen

Fixtures never touch a component. The chat answer is a real
`text/event-stream` with the real event names (`connected`, `citation`,
`chunk`, `title`, `artifact`, `step_update`, `done`), parsed by
`src/shared/api/chat-client.ts` and applied by the real controller:

- `citation` → the controller summons **Kilder**; the canvas slides in with the real source cards.
- `chunk` → the assistant text streams in the transcript.
- `artifact` (same id, re-emitted with more content) → **Resultat** opens and the document grows line by line.
- `step_update` → the **Arbeid** badge counts up; one with `status: "awaiting_approval"` makes the controller fetch `GET /api/v1/orchestration/runs/<run>/approvals`, which renders the real approval card («Godkjenning · Godkjenn / Avvis») under the answer.
- `done` → the turn settles; the pointer glides to «Godkjenn» and the harness's real mouse move produces the `:hover`.

## Add a scene

1. Add fixtures to `fixtures/` (scene-03 additions go in `fixtures/fjordform-undersok.ts`).
2. Create `scenes/<id>.ts` exporting a `SceneDefinition`: composer prefill, the
   SSE `frames` (ms after the POST arrives), pointer keyframes (targets by CSS
   selector or accessible name; `pin: true` for a target that moves after the
   pointer reaches it), the hover target and the log events.
3. Register it in `scenes/index.ts`.
4. Run the preview script and look at every checkpoint before handing it to the harness.

### Optional scene extras (used by scene 03)

All optional; a scene without them behaves exactly as before.

| Field | What it does |
|---|---|
| `prepare(helpers)` | Runs inside `ready`, after the composer mounts and before the prefill, with `click(target)` and `waitFor(probe)`. Scene 03 turns on «Dyp research» with the composer's own button. |
| `routes` | Scene-specific JSON answers, matched before the shared routes (approvals, run detail, plans). |
| `runStreams` | `GET /api/v1/runs/:run/events` as an event stream. Frame times are ms after the chat POST, the same clock as `stream.frames`; a panel that subscribes late gets the frames it missed at once, in order. Stays open unless `closeAfterMs` is set (a run paused at a gate has not finished). |
| `images` | Request pathname → asset URL for evidence the app loads with a plain `<img src>` (browser step screenshots from `/api/v1/browser/sessions/:run/artifacts/:id`, which `fetch` never sees). The src is rewritten as it is set, so no request leaves for `/api`. |

`install.test.ts` covers the route order, the run-stream catch-up and the image mapping.
Scene 03 (`scenes/03-undersok.ts`) documents in its header which real panels it drives, the fallback rung, and every deviation from the beat sheet.
