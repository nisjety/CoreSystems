# Group: Quarry docs, docs-contract test and Ingestion compose

All eight files are resolved, have no conflict markers and are staged with git add. Nothing was committed.
Line endings: the five .md files are CRLF in the working tree (core.autocrlf=true, index is LF); I kept CRLF.
openapi.yaml, docs-contract.test.mjs and docker-compose.yml are LF (eol=lf attribute); I kept LF.
Quarry node tests: `node --test scripts/tests/*.test.mjs` gives 53 pass, 0 fail, 0 todo.

## CROSS_PLANE_AUTH.md (one hunk, the TL;DR table)
- Branch: data-plane row lists the four scopes; model-plane row says "not granted ... (D16, QB-T17)".
- Live: data-plane row explains each scope per route and the 2026-10-13 live check; model-plane row says "None".
- Merge: data-plane row is the live text (more complete, also names all four scopes).
  Model-plane row is the live "None" text plus the branch's "D16, QB-T17" reference and "every model-gateway call degrades".

## CROSS_PLANE_INTEGRATION.md (one hunk, the `granted` bullet)
- Branch: bullet that says quarry-edge holds `data-plane` and `browser-broker` (P2) audiences.
- Live: shorter bullet (switches default off, 2026-10-06 check) plus a new "Semantic rerank calls only web-rerank" bullet.
- Merge: the branch's audience sentence (keeps browser-broker) with the live "switches default off (QB-T17)" and
  2026-10-06 check wording, followed by the live rerank bullet unchanged.
  I dropped "the semantic reranker" from the list of Model Plane callers: option B calls the Data Plane, and the
  docs-contract test forbids that phrase on the "refuses the three Model Plane rows" line.
  The branch's `browser-grant-validate` table row and its new "Contract" bullet (Redis in CI) merged cleanly and stay.

## ONBOARDING_LIVE_CRAWL.md (one hunk, Enable/disable bullet)
- Branch: default `true` when a Model Plane URL is set, plus a note that the compose sets 0 (decision 3.12).
- Live: default `false` (QB-T17).
- Merge: default `false` (matches `config.rs`, where `page_title_enrich` is a plain bool defaulting to false, and
  the new `model_plane_gate.rs`), plus the branch's compose and decision 3.12 note, extended with "a granted model path".

## SELF_HOST.md (one hunk, PAGE_TITLE_ENRICH row)
- Same pair as above. Merge: default `false`, "the Ingestion compose also sets `0` (decision 3.12)", live explanation and pointer
  to the Model Plane switches table. The live option-B rows (SEMANTIC_RERANK, TOP_N, DEADLINE_MS, SHADOW, VECTOR_RETRIEVAL,
  budget), the live "Model Plane switches (QB-T17)" section and the branch's MODEL_PLANE_URL opt-in, browser grant, web policy
  rows and model-gateway paragraph all merged cleanly and stay.

## RUNBOOKS.md (one hunk, both sides appended at the end of the file)
- Branch: its 1810 added lines (from "Chromium will not start" to "Profile owner ... D16 Q2").
- Live: one new section, "Semantic rerank (Data Plane /v1/rerank)".
- Merge: branch sections in their order, one blank line, then the live section. No line edited. Append-only kept.

## openapi.yaml (one hunk, end of SearchResponse properties)
- Branch: `policy_downgrades` property (WS7.E). It also moved internal routes to openapi-internal.yaml and made
  ProfileCookieView.value nullable; those parts merged cleanly and stay.
- Live: `score_kind`, `rerank` properties and the new `RerankReport` schema. Its other changes (400 response on /v1/search,
  `zdr`, `score_kinds` on SearchRequest, `score` on the result item) merged cleanly.
- Merge: `policy_downgrades`, then `score_kind`, `rerank`, then the `RerankReport` schema. No generated file is tied to
  openapi.yaml here (the openapi tests and the route-coverage test pass).

## scripts/tests/docs-contract.test.mjs (add/add, 13 hunks)
- Branch: a copy of the live test that wraps 12 tests in `pending(...)` (TODO marker) with constants OPTION_B and QB_T17, because
  option B and QB-T17 were not on the branch; it also adds two whole-file YAML tests using `../yaml-subset.mjs`.
- Live: the same tests as plain `test(...)`.
- Merge: all tests plain `test(...)` (option B and `model_plane_gate.rs` land with this merge, and the branch's own comment said to
  drop the marker then). Kept the `parseYamlSubset` import and both whole-file YAML tests. Removed the now-unused `pending` helper
  and the OPTION_B / QB_T17 constants. Every test now runs for real and passes.

## docker-compose.yml (one hunk, quarry-edge environment)
Defaults chosen (owner decisions 2026-10-28, DECISIONS-ROUND 3.12):
- `QUARRY_EDGE__MODEL_PLANE_URL`: `${MODEL_PLANE_URL:-}` (opt-in, empty). Branch value kept. The live value defaulted to
  `http://model-gateway:8080` (as main's committed compose did); the owner decision overrides it.
- `AUTOPROMPT`, `LLM_CLASSIFY_INTENT`, `PAGE_TITLE_ENRICH`: `0`. `AUTOPROMPT_MODEL` kept (empty default).
- `ANSWER_SYNTHESIS`, `AI_FORMATS`, `SEARCH_SUGGEST`, `MODEL_AUDIO`: new from the live work (QB-T17), default `0`.
  The branch comment saying these four "have no switch" is gone, since the switches now exist.
- `SEMANTIC_RERANK`: `0`. In main's committed compose it was `1` (the old Model Plane reranker). Now it is the option-B Data Plane
  rerank, and decision 3.12 lists SEMANTIC_RERANK among the four flags that default 0, so 0 wins. Live comment block kept in full.
- `SEMANTIC_RERANK_TOP_N` `10` and `RERANK_DEADLINE_MS` `2000`: kept from live (TOP_N is the same on both sides).
- `RERANK_SHADOW`: kept at `1` (live default). It is a safety mode, inert while SEMANTIC_RERANK is 0 and, once rerank is on, it
  measures only (no reorder, no score). RUNBOOKS.md and SELF_HOST.md state "sets it to 1" and the docs-contract test ties those docs
  to the compose value. UNSURE: if the owner wants shadow 0 by default, change the compose and those two doc sentences together.
- `SEMANTIC_RERANK_MODEL` dropped: a removed identifier of the old Model Plane reranker; the docs-contract test fails if the
  compose names it. It was in main's committed compose and in the branch.
- `QUARRY_EDGE__VECTOR_RETRIEVAL` is not set in the compose on either side (binary default false); nothing to add.
- Every other env key of both sides is still present (checked by diffing both sides' added keys against the result).

## Open points for other groups
- The test reads `config.rs`, `main.rs` and `cross-plane-token-requests.json`; it passes against the tree as it is now. If the
  Rust groups change `page_title_enrich`, `rerank_deadline_ms` or the field names, re-run the node tests.
