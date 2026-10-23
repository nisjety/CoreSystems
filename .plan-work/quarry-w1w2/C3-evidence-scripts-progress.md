# C3 evidence scripts: what was built, how to run it, what each run must show

Unit W1-C3-evidence-scripts (items g, o1, o2). BUILT, NOTHING RUN against any stack. Docker, Redis,
NATS and every live or side-by-side stack were not touched. Each script has `bash -n` clean; the
helpers of each probe script have a `--self-test` on canned data (run, passing); the gated Rust test
compiles and skips cleanly without its env vars (run once: "1 passed", which is the skip path).

Order of an evidence run (one container at a time, under the shared docker lock):
1. WS4.B/C stack: scratch Redis (and scratch NATS for C).  2. WS4.A probes.  3. WS4.D/E image probes.
4. WS4.G token probe. Every script refuses the live container or a non-throwaway Redis.

## g. WS4.B and WS4.C (apps/Frontend Plane/verevonv3)

| Script | Purpose | Expected result |
|---|---|---|
| `scripts/gateway-cache-guard.lib.sh` (new) | shared guard sourced by both cache scripts | scratch-/throwaway-/sbs-/test- container or loopback host: accepted; anything else: exit 2 unless `--allow-non-throwaway` |
| `scripts/gateway-cache-forensics.sh` (edited) | read-only aggregates; now takes `--allow-non-throwaway` | seeded stack: legacy N, org-scoped M, aggregate JSON, no key or payload text |
| `scripts/gateway-cache-purge.sh` (edited) | dry run default, `--apply`, `--allow-non-throwaway`, either order, unknown arg exit 2 | dry run counts only the legacy keys; apply removes them in batches of 100; `:o:` keys and `verevon:gw:ratelimit:*` survive; a re-run says 0 |
| `scripts/tests/gateway-cache-scripts.test.sh` (edited) | self-test on a scratch Redis (db must be empty) | previous OK line, plus both scripts exit 2 for `docker exec -i verevon-redis redis-cli` and `redis-cli -h redis.prod.example` |
| `docs/runbooks/gateway-search-cache-tenant-incident.md` (edited) | guard section, side-by-side evidence run, WS4.C section | step 0 table row for the side-by-side gateway filled in by the run |
| `scripts/gateway-gdpr-nats-scratch.sh` (new) | `up`/`test`/`down`/`run`: scratch nats:2-alpine -js + redis:7-alpine, stream AQENCIA_CONTROLPLANE, durable verevon-gateway-gdpr-erasure-v1 (pull, filter verevon.gdpr.erasure.requested, deliver all, ack explicit, 30 s, max deliver 20) | `up` prints "durable ... provisioned (pull, filter ...)"; `test` prints "TEST PASSED"; a skip is a failure |
| `apps/gateway/src/gdpr_nats.rs` test `a_published_org_erasure_purges_result_cache_keys_through_a_real_jetstream` (new, gated by `GATEWAY_TEST_NATS_URL` + `GATEWAY_TEST_REDIS_URL`) | publishes `*` then org A erasure; runs `run_once` (the production loop) | org A's 40 keys (search-web, scrape-preview) gone inside 30 s; org B's 2 keys, org `...-more`'s key and an identity key survive; `*` purged nothing |

WS4.B run recipe (also in the runbook): seed legacy `verevon:gw:search-web:deadbeefdeadbeef` and
`...:scrape-preview:...` keys next to `:o:` keys; forensics (before); deploy; purge dry run, `--apply`;
forensics (after): legacy 0, org-scoped unchanged. Open question 3 is answered from deployment
history (step 0), not from these numbers. Real gateways: operator action with `--allow-non-throwaway`.

WS4.C record must say: scratch NATS on one host, no auth, not `control-shared-nats`.

## o1. WS4.A (apps/Frontend Plane/verevonv3/scripts/evidence)

| Script | Expected result |
|---|---|
| `ws4a-gateway-clippy-triage.sh` | runs CI's `cargo clippy --all-targets -- -D warnings`; exit status 0 = clean; else OWNED must be 0 (no finding in cache.rs, cache_admission.rs, zdr.rs, zdr_log.rs, observability.rs, gdpr_nats.rs, domains/search.rs, domains/knowledge/{quarry,products}.rs, the two WS4 test files) and the OTHER list (earlier record: 17 places) goes to its owners. Exit 0 if OWNED=0, 1 if not, 2 if clippy cannot run. |
| `ws4a-deploy-probes.sh metrics` | `/version` revision is not "unverified"; `gateway_result_cache_total` exported with labels ns and outcome only (no org/user/tenant label) |
| `ws4a-deploy-probes.sh ab` | (1) org A twice: search.upstream +1, keys o:<A> +1; (2) org B same query: upstream +1, keys o:<B> +1, A key string != B key string |
| `ws4a-deploy-probes.sh zdr` (after the operator flips org B to ZDR and 61 s pass; `ZDR_FLIPPED_AT` enforces it) | upstream +2, keys o:<B> +0, `bypass_zdr` +2, Quarry `search.result_shape` cache_hit false +2 |
| `ws4a-deploy-probes.sh outage` (after the operator stops org-core) | org A new query twice: 200 both, upstream +2, keys o:<A> +0, `bypass_zdr_unknown` +2 |

## o2. WS4.D, WS4.E, WS4.G (apps/Ingestion Plane/Quarry-v2/scripts/evidence)

| Script | Expected result |
|---|---|
| `ws4-image-probes.sh` | private edge at RUST_LOG=debug (precondition checked via docker inspect): ZDR scrape, ZDR scrape/stream, ZDR search with SearXNG dead, ZDR agent start + navigate: marker count 0 each; standard scrape control: marker >= 1; the dead-provider search logs a `search.upstream` error line; with `ZDR_TOKEN_FILE`: `change/latest?url=` marker 0, `/v1/search/suggest` reported only (Gap row, owner unassigned) |
| `ws4g-live-zdr-token-probe.sh` | ZDR-org bearer from a file: crawl, batch, persistent-profile agent start: 403 with a ZDR message; schedules: 200 today (Gap row) and the probe deletes what it made, or 403 `zdr_durable_mutation_forbidden` after WS4.H stage B; optional standard-org control for the profile start |
| `lib.sh` (both planes carry a copy) | PASS/FAIL/NOT-RUN lines and a `RESULT` summary; NOT-RUN is not evidence |

Owner for `/v1/search/suggest`: this is a decision (forward the posture or refuse under ZDR), not a script;
the matrix row says "unassigned". Written to depth/W1-C3-evidence-scripts.md as a decision; the probe
only measures what happens today.

## Commands run in this unit (the only ones)
- `cargo check --tests` in apps/gateway: rc 0 (1m40s).
- `cargo test a_published_org_erasure` in apps/gateway: 1 passed (the env-gated skip path; the real path is owed).
- `rustfmt --check` on gdpr_nats.rs: clean after one reformat.
- `bash -n` on every shell script; `--self-test` of ws4-image-probes.sh (5 PASS), ws4g-live-zdr-token-probe.sh (3 PASS), ws4a-deploy-probes.sh (3 PASS), ws4a-gateway-clippy-triage.sh (OK); guard function table and refusal exit codes (2) run on strings only.
