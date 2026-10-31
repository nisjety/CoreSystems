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
| `fixtures/fjordform.ts` | The fictional world of the films (stage 02): Fjordform AS, Ida Berg, a customer and its order. Scene 02: the start-screen notice (the start-screen suggestion), the three sources (shipment tracking, customer agreement, e-mail), the status document, the plan rows, the approval. Norwegian only. |
| `fixtures/fjordform-undersok.ts` | Scene 03 (stage 03): the invoice notice (the start-screen suggestion), the invoice against the framework agreement and the order (120 stk at 1 380,00 against 1 240,00, a 16 800,00 kr difference), the three sub-tasks, the browser step, the comparison document, the «Opprett kreditnota» approval. |
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

## Live demo (stage 04, `src/demo/live`)

The interactive demo on verevon.com runs the real `/dashboard` and `/chat` against an in-memory engine, instead of a film. The plan is `apps/verevon-web/plans/product-demo/PLAN_2026-10-11.md`. The code sits under the same `VITE_SCENE_MODE=1` guard, so the normal build contains none of it.

| File | Role |
|---|---|
| `live/index.ts` | `installDemoLive(options)`: wires the thread store, the responder, the routes, the live hook and the suggestions. Returns `dispose`, `reset` and `startScenario`. |
| `live/shared-types.ts` | The contract: `DemoRoute`, `ThreadRow`, `DemoLiveOptions`, `DemoLiveHandle`, the iframe messages, `DEMO_NOW_ISO`. |
| `live/responder.ts` | `POST /api/v1/chat/stream` becomes a scenario turn. It also keeps runs, approvals, decisions, queued input and stop. It has no timers of its own: the clock and the channel factory are injected. |
| `live/channel.ts` | One `text/event-stream` per request. Frames go out now or after a delay. The stream closes on request, after an idle timeout (pending approvals), or when the visitor aborts. |
| `live/frames.ts` | Pure frame builders with the product's real event names and payload keys, `paced()` (two-word deltas every 45 ms) and `encodeFrame()`. |
| `live/match.ts` | Deterministic matching: the exact suggestion text scores 100, otherwise all-of trigger groups matched by whole word. A trailing `*` marks a stem (`tilbud*` takes `tilbudet`). Free text answers a case only when it names it: a group is the customer's name or an order or series number alone, or at least two words, and a group of generic words needs three. Also `normalizeText`, `termMatches` and `titleFromQuestion`. No model, no network. |
| `live/routes.ts` | The route table for threads, the stream, invocations, runs, approvals and orchestration. Speech and audio answer 503, scheduling writes 403, both in Norwegian. |
| `live/thread-store.ts` | In-memory conversations, seeded from `threadSeed`. A seeded case thread opens with its question and answer. |
| `live/suggestions-live.ts` | The three cases as cards on the start screen and the dashboard. A `#demo:<id>` link fills the composer and presses send. A Do case chooses «Utfør» first. It also closes the Work canvas, as the films did. |
| `live/scenarios/` | Tilbud til Solhaug Hotell (Do, or Ask without «Utfør»), Reklamasjon fra Havnebrygga Kafé (Ask), Dokumentasjon til Nordhavn kommune (Ask), and the fallback. `index.ts` is the registry and the transcripts of seeded case threads. |
| `live/demo.css` | Every rule scoped to `html.verevon-demo-live`. |
| `fixtures/fjordform-tilbud.ts`, `fjordform-reklamasjon.ts`, `fjordform-anbud.ts` | The data of the three cases: question and suggestion card, sources, answer and follow-up text, the document by stage, the approval text and the step texts. |

### Behaviour

- Free text, attachments and images get the honest fallback: «Dette er en demo med forhåndsdefinerte svar», with the three cases as links. Free text answers a case only when it names it: the customer («Solhaug», «Havnebrygga», «Nordhavn»), the series 2609, or FSC, EPD and anbud together. A quote, warranty or tender question that does not name its case falls back, and nothing else is answered freely.
- Each case stops at a gate: the approval card («Godkjenn» or «Avvis»). A decision sends its outcome on the same open chat stream, adds it to the stored conversation and, for Do, updates the run.
- The dashboard's recent work is other, older work. A case's conversation joins the recent list only once a visitor has run it.
- A Do case without «Utfør» is an ask run: the same work as steps on the chat stream, and its gate is a step.
- Nothing leaves the page, and no draft is sent. The engine answers every `/api` request in memory.
- Timing: chat frames are scheduled from the POST, a decision's frames from the click. A run panel that connects late gets the frames it missed at once, in order.

### The install hook (`install.ts`)

- `setLiveHook({ routes, resolveImage })` is the only change to scene mode. The hook is consulted after the scene extras and before the shared fixtures. Its default is `null`, which makes it a no-op, so the films stay frame-identical.
- Requests resolve against the document's address (`window.location.href`), not `window.location.origin`. Inside the sandboxed iframe that origin is the opaque `"null"`, and it does not parse.
- A live route that throws becomes a Norwegian 500, and a `live-error` entry in the scene log.

### Tests

```sh
pnpm exec vitest run src/demo/live src/demo/install.test.ts src/demo/suggestions.test.ts
```

These run without a browser: pure functions, a manual clock, and the real SSE bodies read back as text. The browser check, `node scripts/demo-live-check.mjs --url <demo url>`, drives headless Chromium and fails on any leak. Its flows still belong to the retired cases: rewrite them for the three cases before it is run.

### Known limits

- A reload clears everything: the store is in memory, and the sandbox has no storage. A pending approval does not come back after a reload.
- The rung-only plan gate that a Do turn derives is hidden by CSS. Its endpoint answers as written, so nothing is granted by it.
- The product's English Work rows are not hidden: «Connect stream», «Compose response» and «Model selected», with details such as «Connected to the live agent stream.», «Streaming answer text.» and «Completed.». The client writes them on every turn (`chat-normalizers.ts`, `use-chat-controller.ts`). They share the generic step markup, so `demo.css` cannot select them, and the engine cannot stop them. This needs an owner decision: a written exception to the Norwegian rule for these labels, or a Norwegian change in those two files.
- The seeded conversations that are not cases open empty: the engine holds no transcript for them.
