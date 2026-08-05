# Edge-side answer streaming + Convex persistence — execution plan (all local)

Status: **in progress.** Foundation landed; remaining steps below are precise +
investigated. Branch `feat/ai-search-platform`.

Done so far (committed `b3318da`):
- `ModelPlaneClient::invoke_stream` (quarry-runtime/src/mp_client.rs) — POSTs to
  `model-gateway /v1/invoke/stream`, parses SSE frames, yields `delta` strings.
  `async-stream` added to quarry-runtime deps. `cargo check`: clean.

---

## Part A — Edge-side streaming (max answer quality: full-page grounding + streaming + cache)

Goal: a `POST /v1/answer/stream` SSE endpoint on quarry-edge that runs the SAME
high-quality pipeline (hybrid search → full-page fetch → context) and streams
the synthesis token-by-token, then caches the full answer.

Facts (investigated):
- model-gateway SSE: `event: …\ndata: {json}\n\n`; `data` JSON has a `delta` string.
- `AnswerPipeline::answer()` (quarry-runtime/src/answer.rs ~129-240): search →
  concurrent `fetch_markdown` of top-K → build `combined` context + `citations`
  → `AiFormatRunner::query(combined, query, zdr)` (blocking synth) → AnswerResult.
- `AiFormatRunner::query` (ai_formats.rs ~149) builds the grounding prompt then
  `client.invoke`. `AiFormatRunner.client: Arc<ModelPlaneClient>` (private).
- edge deps have `axum`, `async-stream`, `futures-util`; AppState has
  `answer_pipeline: Option<Arc<AnswerPipeline>>` + `redis` + (new) cache helpers.

Steps:
1. **ai_formats.rs** — add
   `pub fn build_query_prompt(&self, source: &str, question: &str) -> String`
   (extract the existing prompt string from `query`), and
   `pub async fn query_stream(&self, source, question) -> QuarryResult<impl Stream<Item=QuarryResult<String>>>`
   that builds the prompt + returns `self.client.invoke_stream(req)`. Expose
   `pub fn client(&self) -> &Arc<ModelPlaneClient>` if simpler.
2. **answer.rs** — refactor `answer()` to call a new
   `pub async fn prepare(&self, req: &AnswerRequest) -> QuarryResult<AnswerContext>`
   returning `{ citations: Vec<Citation>, combined: String, sources_used, sources_skipped }`
   (everything up to synthesis). `answer()` then calls `prepare` + `formats.query`.
   Add `pub fn formats(&self) -> &AiFormatRunner` accessor (route needs it to stream).
3. **answer_routes.rs** — add `pub async fn answer_stream(State,Extension<Claims>,Json<AnswerHttpRequest>) -> Sse<...>`:
   - `pipeline.prepare(req)` → emit `event: citations\ndata: {citations}`.
   - `for await delta in formats.query_stream(combined, query)` → emit
     `event: delta\ndata: {"delta": "..."}`; accumulate into `full`.
   - on end: write `full` to the **SearchCache** (key incl. org + query + `answer=true`)
     so repeat hits serve instantly; emit `event: done\ndata: {"model":…}`.
   - Build the stream with `async_stream::stream!` + `axum::response::sse::{Sse, Event}`.
   - 501 when `answer_pipeline` is None (mirror `answer`). Meter `ANSWER_SYNTH` usage.
4. **lib.rs / main.rs (edge)** — register `.route("/v1/answer/stream", post(answer_routes::answer_stream))`.
5. **Cache-hit fast path (optional, nice):** before prepare, check SearchCache;
   on hit, emit citations + the cached answer as one delta + done (instant, no LLM).
6. **Rebuild edge** (`docker compose -f "apps/Ingestion Plane/docker-compose.yml" build quarry-edge` → `up -d --no-deps quarry-edge`). Cache-warm now, so faster.

BFF + UI:
7. **BFF** `apps/Frontend Plane/verevonv2/src/app/api/v1/search/answer/stream/route.ts`
   — POST {query}. Mint Quarry audience token; open SSE to
   `${QUARRY_EDGE_URL}/v1/answer/stream`; re-stream frames to the browser
   (pattern: copy `app/api/chat/stream/route.ts` pipe loop).
8. **UI** `SearchAnswerView.tsx runSearch` — replace the current chat-stream
   turn-0 streaming with this route: read the `citations` event → dispatch
   `results-loaded` citations; read `delta` events → `answer-delta`; `done` →
   `answer-stream-done`. (Reducer actions already exist from Phase 3.) Keep
   `includeAnswer:false` on `/api/v1/search/web` (results only). This restores
   full-page grounding + caching WHILE streaming.

Verify: `cargo check` + `tsc --noEmit` (cap heap: `NODE_OPTIONS=--max-old-space-size=3072`,
the container OOMs otherwise); rebuild; live `/v1/answer/stream` returns SSE with
a non-empty streamed answer + citations.

---

## Part B — Convex persistence (local convex-backend)

Goal: persist search/answer threads + saved searches to the **local**
`convex-backend` container; real-time reads in verevonv2.

Investigate first (not yet done):
- `apps/Frontend Plane/verevonv2/convex/` (schema.ts, functions) — does it exist?
- How verevonv2 connects: `ConvexProvider` / `ConvexReactClient(process.env.NEXT_PUBLIC_CONVEX_URL)`.
  The local deployment URL = the `convex-backend` container (check its published
  port + `convex dev`/self-hosted env). "Everything local" → point
  `NEXT_PUBLIC_CONVEX_URL` at the local convex-backend, NOT cloud.
- `docker inspect convex-backend` for the deploy URL/port + admin key.

Steps:
1. **Schema** `convex/schema.ts` — tables:
   `searchThreads { orgId, userId, query, answer, citations, createdAt }` (index
   by `["orgId","userId"]` + `["orgId","createdAt"]`);
   `searchTurns { threadId, role, text, createdAt }` (index by `threadId`).
2. **Functions** `convex/searches.ts` — `createThread` (mutation), `appendTurn`,
   `listThreads` (query, org+user scoped), `getThread`. **Org-scope every query**
   (pass orgId from the authed session; never trust client).
3. **Client wiring** — ensure `ConvexProvider` wraps the app (likely in a shell
   layout); `NEXT_PUBLIC_CONVEX_URL` → local convex-backend. Deploy the schema/
   functions to the local backend (`npx convex dev`/self-hosted push against the
   local URL).
4. **SearchAnswerView** — on a completed search/answer, `createThread` + persist
   each follow-up turn via `appendTurn`; add a "recent searches" surface backed
   by `listThreads` (real-time `useQuery`).

Tenancy/ZDR: org-scope all reads/writes; honor ZDR (skip persistence when the
org/query is zero-data-retention).

Verify: a search persists a row visible via Convex dashboard / `useQuery`;
reload shows recent threads; another org cannot read them.
