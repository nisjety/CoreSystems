# D-B3-openapi-sdk: findings not fixed here (2026-10-27)

Every finding of the unit that stays open, with why and the owner's next step. Commits are on `quarryv2-next`.

## 1. docs-contract gate: 12 assertions pending (D2-027)

- What: `scripts/tests/docs-contract.test.mjs` is the owner's live-tree file (C:/dev/CoresSystem, branch main, untracked), restored unchanged plus a whole-file YAML check. 12 of its 17 assertions are marked `todo` (run, printed as TODO, not failing the suite): 11 pin the Data Plane rerank wire and config ("option B"), 1 pins the QB-T17 compose defaults.
- Why pending: option B exists in the live tree only as uncommitted edits (`config.rs`, `docker-compose.yml`, `openapi.yaml`, `CROSS_PLANE_AUTH.md`, `cross-plane-token-requests.json`, all `M` in C:/dev/CoresSystem). On this branch the compose still sets `QUARRY_EDGE__SEMANTIC_RERANK_MODEL`, the config has no `rerank_deadline_ms`, and the OpenAPI has no `RerankReport`.
- Owner decision: commit option B onto quarryv2-next (then drop the `pending(...)` markers), or keep the gate on the live tree and let this branch keep the TODO markers. Not decided here.

## 2. QB-T17 compose defaults (the 12th pending assertion)

- `apps/Ingestion Plane/docker-compose.yml` sets `QUARRY_EDGE__AUTOPROMPT` to `${QUARRY_EDGE_AUTOPROMPT:-1}` (on by default), and does not set six other Model Plane switches (`LLM_CLASSIFY_INTENT`, `PAGE_TITLE_ENRICH`, `ANSWER_SYNTHESIS`, `AI_FORMATS`, `SEARCH_SUGGEST`, `MODEL_AUDIO`). The gate expects every one present with default 0. The plan says the flags stay off (depth plan: "Flags stay off").
- Not changed here: the compose is outside this unit's files and a default change alters the stack's configuration on the next deploy. Owner decision, then a compose commit.
- Note: each Model Plane call is refused today anyway (see 4), so turning the flags off changes the logs, not the behaviour.

## 3. SDK regeneration (WS13.8 / U56): blocked, nothing committed

- Ran: `sdks/generate.sh` (openapi-generator-cli v7.22.0, docker, under `with-lock.sh`, `MSYS_NO_PATHCONV=1`). Logs: `scratchpad/D-B3-openapi-sdk/sdk-generate-2.log`, `sdk-ts-gen.log`.
- Python: aborts in pydantic codegen ("Codegen Property not yet supported in getPydanticType"), on the cookie `value` property `type: 'null'` added by WS17.P0 (b068c33bc). The spec has exactly one bare `type: 'null'` schema (line 3198, ProfileCookieView.value); the other `'null'` hits are type arrays, which 7.22.0 handles. (An earlier version of this note said "13 places"; that was wrong, corrected 2026-10-28 by D2-A-openapi-null, which fixed the one place and regenerated.)
- TypeScript: generates, but `src/models/ProfileCookieView.ts` imports `./Null`, which is not generated; `tsc` (TypeScript 5.9.3, the version the SDK declares) fails with two TS2307 errors. The regenerated output is kept at `scratchpad/D-B3-openapi-sdk/sdk-regen-ts-output/`.
- Reverted: the 25 tracked docs were restored to HEAD and the 38 new docs removed. Nothing under `sdks/` is committed.
- Side effect to know: `sdks/typescript/src/` is gitignored (Quarry-v2 `.gitignore` line 50), so the regeneration overwrote this worktree's local generated `src/`. It does not compile now. Regenerate it once the spec fix lands, or delete it.
- Owner decision: how to express the constant-null cookie `value` in a shape the generator accepts (for example `nullable: true` with `enum: [null]`, or leave the field's type out), or upgrade the generator. Then rerun `sdks/generate.sh` and `tsc -p sdks/typescript/tsconfig.json` with TypeScript 5.9.3 (the TypeScript 6.0.x in the verevonv3 store rejects this tsconfig with TS5107 and TS5011, a pre-existing config problem).
- Python SDK tests: not run (the generator did not produce them).

## 4. WS16.S0 items not done here

- `crates/quarry-edge/src/model_plane_gate.rs` (header lines 1-16 and the manifest notes of `model-invoke`/`model-stream`): the file is untracked in the live tree and is not on this branch (no module declares it). When it lands, correct the header: it says flipping `granted` is "the one change a feature needs"; that is wrong, because `authorize_principal_route` (`apps/Model Plane/rust/services/model-gateway/src/auth.rs`, about line 1300) refuses service principals on `/v1/invoke` and `/v1/invoke/stream` (only `POST /v1/ai/chat` and `/v1/ai/embeddings`, and two internal routes, are open to them).
- `apps/Model Plane/docs/decisions/ledger.md` (new top entry DR-15.1) and the `ExtractStructuredResponse.extracted_json` comment in `apps/Model Plane/proto/model_plane/v1/gateway.proto`: the Model Plane session's files.
- The docs-contract's two model-row assertions stay as they are (they pass).

## 5. D1-024 (challenge classifier): what is still open

- `RespondDialog` and `MouseWheel` on an exhausted host are allowed by design (`reject_if_exhausted`, and RUNBOOKS line ~379 says "scroll and mouse wheel, answering a dialog that is already open" stay allowed). The finding calls them a gap. Changing it is a policy decision: owner.
- A redirect into an exhausted host is not checked: the classifier sees only the navigation action, not the redirect chain. Needs browser-side redirect data. Not done.
- `UploadRef`/`DownloadRef` were already refused at HEAD (checked in `reject_if_exhausted`).
- Fixed in 10e7a406c (challenge.rs) and 85967a31d (observation.rs, one line): title cues as whole words, `pardon` narrowed; only attempts count toward the third strike (`observe_after`); the status is bounded to the request that started the navigation.
- Enforce default unchanged (D1-068 stays open, owner).

## 6. D2-037 (GAP.md)

- The rows already said "Not wired" (73604c4d8, WS14.S0). Fixed in 04cd5b1d9 the remaining wording: the crawl accepts only `sitemap: skip`, and `/v1/map` reads `/sitemap.xml` without seeding a crawl.

## 7. Route coverage (D2-003)

- Three internal routes (`/v1/internal/orgs/{}/profiles`, `/v1/internal/queues/{}/enqueue_batch`, `/v1/internal/schedules/preflight`) are on the allowlist with the internal-route reason, not in `openapi.yaml`. Documenting them is a decision about the public contract; they are described in RUNBOOKS and ZDR_MATRIX.

## 8. Scrape schema golden (D2-289)

- `cargo test -p quarry-core --features jsonschema --test scrape_schema`: 2 passed. The golden matches the Rust type; the "not regenerated" note in D2-289 is stale.

## 9. Other notes

- WS13.5 `data.actions[]` schema: added in 50c567dd7, from D-B3-scrape-actions' hand-over (its depth file, item 1). The SDK docs and models for it wait for item 3.
- YAML whole-file check (`scripts/yaml-subset.mjs`): a strict subset. It refuses anchors, tags and flow mappings with content (26 of 135 tracked YAML files use them). It never accepts a file yaml.v3 rejects, on the tracked set. Extend it or use a real parser when a gated file needs those constructs.
- `docs/RUNBOOKS.md` and `docs/ERROR_REASONS.md` (hot files): not edited; no statement in them contradicts the new strike rule.
