# Web-rerank gate: operator runbook (QB-T14)

Status on 2026-10-13: the runner, the 80-query set and its frozen search results are built and tested (43 unit tests); the gate reports `pending` (exit 2) and cannot pass until the labels, a measured baseline and calibration exist. Nothing here has been run against the live Data Plane. The runner's own usage text is `quarry-web-rerank-gate help`; this page records the owner's choices and the facts an operator needs.

## Decided parameters (owner, 2026-10-13)

- Rerank window: 10 (`--window 10`, the runner's default and the production `QUARRY_EDGE__SEMANTIC_RERANK_TOP_N`). A gain measured at 20 would not predict what production sends. Changing the window needs a new recording and a new baseline.
- Judged set: the 80 queries (50 Norwegian, 30 English, six categories) for the first run. The gate already requires a powered comparison (minimum detectable effect at most 0.04). If the first report is underpowered, extend the set to about 150 queries before any default-on decision; do not extend it up front.
- Arm D (candidate host in the text, diagnostic, never gated): off. Add `--arms C,D` only when the gate fails and you need to see why (80 more paid calls per run).
- Quarry compose keeps `QUARRY_EDGE_SEMANTIC_RERANK=0` until the gate passes; the flip to 1 is the last step of the Data Plane deploy order (`apps/Data Plane v2/docs/web-rerank-v1.md`, step 4).

## Where the frozen search results come from (verified read-only on 2026-10-13)

- The freeze (`queries.v1.json`, `results.v1.jsonl`: 80 queries by 20 candidates = 1,600 rows, frozen 2026-10-12 23:35 UTC) was taken from the self-hosted SearXNG federation, image `searxng/searxng:2026.7.12-c19d86faa`, the compose tag, with `count=20` and `safesearch=1`.
- Engines that contributed rows (a row can carry several): `google cse` 452, `abcnyheter` 432, `yandex` 353, `bing` 317, `seznam` 292, `naver` 194, `wiby` 15. Unresponsive on every query: `brave`, `duckduckgo`, `privacywall`, `startpage`.
- `google cse` is not Google's paid API. SearXNG's `google_cse.py` scrapes Google's public Custom Search embed with a third party's token (`partner-pub-...`, the blackle.com search engine), with `use_official_api: False` and `require_api_key: False`; the repo configures no key or engine id. So the freeze costs nothing and bills no account. The risk is different: it depends on a third party's token and on search-engine terms of service, and production Quarry search uses the same engines (the SearXNG settings say `google cse` and `bing` carry the query).
- The fixtures commit third-party titles and snippets. Ask counsel, together with the Article 28 question, whether that is acceptable, or keep the frozen results out of git and commit only query ids, urls and labels.

## Freeze quality (found by the QB-T14 review, 2026-10-13): do not label the current frozen results

- While the results were frozen, brave, duckduckgo, privacywall and startpage did not answer for any of the 80 queries, wikipedia for 59, google cse for 8 and seznam for 2. The rows therefore come only from bing, google cse, yandex, seznam, naver, abcnyheter and wiby. The live SearXNG statistics show upstream suspensions (brave 100% for 180 s, privacywall HTTP 403, startpage CAPTCHA for 3600 s, duckduckgo CAPTCHA or 403, rate limits on google cse and wikipedia). The four engines are enabled in the configuration, so this is not a configuration error; the freeze pacing of 250 ms per query is a likely cause but is not proven.
- A freeze is refused once labels exist, so the re-freeze must happen BEFORE any labelling. Re-freeze after the suspensions clear, with slower pacing and in batches (`freeze --start N --count M`), and check the engine set in the report. Refusing a freeze in which a primary engine is missing for most queries, or pinning the engine set explicitly, is a follow-up for the runner.
- Every gate report now states the freeze provenance (image, time, engines seen, unresponsive-query counts per engine).
- Open choice for the owner: the lab freezes 20 results and reranks a window of 10, while production `/v1/search` defaults to a limit of 10, so its default pool is the 10 it reranks; nDCG over 20 is a harsher target than the default production pool. The lab's client timeout is 12 s, production cuts the rerank call at 2 s plus a 0.5 s transport allowance (low severity).

## Order of work

1. Prerequisites: counsel's Article 28 confirmation and `WEB_RERANK_PROCESSOR_APPROVED=true` on the Data Plane; the Data Plane image with `/v1/rerank` deployed and `WEB_RERANK_ENABLED=1`, `RERANK_PROVIDER_MAX_INFLIGHT` above 2 and `RERANK_PROVIDER_RPS` above 0; a web-rerank bearer in `QUARRY_RERANK_BEARER`; the Cohere price for rerank v4.0 pro (the repo has none, so the cost of the run is unknown until the owner supplies it). Check with `bash "apps/Data Plane v2/scripts/qb-preflight.sh" --live --ship-qb-t09`.
2. Re-freeze the search results if they were not taken with all primary engines healthy (see above), then `quarry-web-rerank-gate check` validates the fixtures and the baseline.
3. The paid live passes need no labels, but they must run on the final frozen results (a re-freeze changes the request inputs and invalidates a recording): one pass with `--soak 200` (`--run-id r1`), then two more arm-C passes (`r2`, `r3`), all with the same `--window`. That is 80 + 200 + 80 + 80 = 440 paid requests. An optional 10-call smoke (`--smoke`) adds 10.
4. Labelling (operator step OPS-LABEL) starts only after the final freeze and can run in parallel with step 3. The runner's constants show an agreement threshold (kappa at least 0.6) and an adjudicated state, so plan for two labelers and an adjudication pass; `check` names what is missing.
5. After the labels are complete: `write-baseline --recordings r1,r2,r3`, then `gate --recording r1 --report-out report-r1.json`. Pass: exit 0 with all six checks passing (nDCG@10 gain at least 0.04 with the 95% lower bound above zero, no category losing more than 0.03, added p95 at most 900 ms, soak not-ok rate at most 2%, minimum detectable effect at most 0.04), and no regression against the baseline.
6. Commit the labels and the baseline; add `gate --smoke --recording <committed r1>` to the Quarry CI once a recording is committed.

## Exit codes and hazards

- 0 pass; 1 a check failed; 2 pending or invalid input (a named input is missing); 3 the Data Plane refused a live call or broke its contract (the partial recording is kept; there is no resume, rerun from scratch).
- A bearer that expires mid-run ends the run with exit 3: mint it with enough lifetime, or split the run.
- The degraded rate is measured against the shared deployment (the earlier eval-gate note saw about 19% of queries lost to 429s): use dedicated provider capacity before judging a default-on.
- The real provider response golden of QB-T05a is not captured by this runner (it stores scores only); the Data Plane owns that capture.
