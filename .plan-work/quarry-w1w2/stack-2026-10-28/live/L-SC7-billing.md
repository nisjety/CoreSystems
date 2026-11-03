# L-SC7-billing (live dev stack)

- Leg: L-SC7-billing. SC7 crawl clause (finite default page bound) and BILLING-IDEMPOTENCY (runbook section 4).
- Time window UTC: 2026-10-28 20:03 to 20:10.
- Code that runs: main + integration 7c082825a (live checkout, edge rebuilt at about 19:45 UTC). `GET /version` reports `revision: unverified` (no build arg on live).
- Auth: `Authorization: Bearer dev-evidence` (dev bypass, stub identity `org_placeholder` / `user_placeholder`).
- Flags: one change, logged in `flags-log.md`: `RUST_LOG` `info` -> `info,quarry_runtime::usage=debug` at 20:07:29Z, only to make the edge print the `usage metered` debug line (subject, metric, quantity; no secrets). Reverted at 20:09:17Z. `live-flags.env` is empty, the edge was recreated, `live.sh flags` matches the README baseline (`L-SC7-billing-flags-end.txt`), `RUST_LOG` is `info`, `/ready` 200, edge healthy.
- Usage meter (code): `NatsUsageMeter` in `crates/quarry-runtime/src/usage.rs` publishes `usage.<metric>` with JetStream `publish`. The event id is `quarry:<request_id>:<metric>`. The crawl event is `quarry.crawl.seed`, emitted in `crates/quarry-edge/src/routes.rs` (line 1107 on) keyed by `handoff::billable_job_id(&ack)` (`handoff.rs` line 299): no id for a replay (Control 200 with a key) and none for a failed handoff. The edge log says `usage meter: NATS (publishes to usage.>)` and `usage browser metrics mode` = `off` (legacy rule).
- How the events were observed WITHOUT credentials: the edge debug line `usage metered` (one line per publish the edge starts). The NATS server of the edge (`ingestion-nats`) needs a token, and the rule was to keep secrets inside the containers, so I did not subscribe to `usage.>`. The event id was therefore not seen on the wire (see BI.2).
- Raw responses (no secrets) next to this file: `L-SC7-billing-a.raw`, `-b.raw`, `-idem1.raw`, `-idem2.raw`, `-handofffail.raw`, `-handofffail-key.raw`, `-recover.raw`. The idempotency test value is in `L-SC7-billing-key.txt`.
- Jobs created by this leg, all `completed` in `quarry_v2.jobs` (org `org_placeholder`): `job_01M621YKMZRVE0X8F9VDXM3437`, `job_01M621YKRA17721WXA906CNY9J`, `job_01M6220EZC70FM2ZF8GTJ7FJ0Q`, `job_01M6221N7M2VT55HWFA3SBTR6M`. Nothing left open.

## Results

| id | what | result | evidence |
|---|---|---|---|
| SC7.1 | `POST /v1/crawl` with no `max_pages` gets a finite default and says so | PASS | Seed `http://fixture-a.crawl.test/`. HTTP 200, job `job_01M621YKMZRVE0X8F9VDXM3437`. `data.limits.max_pages` = `{"requested":null,"effective":1000,"clamped_by":["default"]}`; `data.limits.max_depth` = `{"requested":null,"effective":5,"clamped_by":["default"]}`; `data.effective.max_pages` 1000. Job row: `params.max_pages` 1000, `max_pages_requested` empty. |
| SC7.2 | `POST /v1/crawl` with `max_pages:0` gets the same finite default (not unlimited, not a 400) | PASS | HTTP 200, job `job_01M621YKRA17721WXA906CNY9J`. `limits.max_pages` = `{"requested":0,"effective":1000,"clamped_by":["default"]}`. Job row: `params.max_pages` 1000, `max_pages_requested` 0. |
| SC7.3 | `constraints` and `clamped[]` in the response | NOT-RUN (fields absent, same as qv2e) | The answer has `data.{job_id,accepted_at,limits,effective}` only. No `constraints`, no `clamped`. A clamp shows only in `limits.*.clamped_by`. `effective.warnings` and `effective.request_warnings` are `[]` for a default clamp. `GET /v1/crawl/<id>` of both jobs: `total` 1000, `total_kind` `budget`, `warnings` `[]`. Not a FAIL, see Depth 1. |
| SC7.4 | both jobs finish | PASS | `GET /v1/crawl/<id>` after 15 s: `completed` for both. No cancel needed. |
| BI.1 | same crawl twice with the same `Idempotency-Key`: second is a replay (same job id, Control 200) | PASS | Key `sc7-live-billing-1793218067`, body `{"url":"http://fixture-a.crawl.test/","max_pages":3}`. Both edge answers HTTP 200 with job `job_01M6220EZC70FM2ZF8GTJ7FJ0Q`. `docker logs quarry-control`: first `POST /v1/jobs` `status=201` at 20:07:47Z, second `POST /v1/jobs` `status=200` at 20:07:48Z. `quarry_v2.jobs` has exactly one row with that key. The edge answers 200 both times, so the replay is visible only in Control's log. |
| BI.2 | one usage event keyed `quarry:<job_id>:<metric>` for the first submit, none for the replay | PARTIAL | Edge log (`docker logs quarry-edge --since 20:07:47Z`): exactly one `usage metered` line (subject `usage.quarry.crawl.seed`, metric `quarry.crawl.seed`, quantity 1.0) at 20:07:47.44Z, none for the second submit at 20:07:48Z. So one publish for the pair, as expected. The event id was NOT seen on the wire (no credential for a subscriber); by code it is `quarry:job_01M6220EZC70FM2ZF8GTJ7FJ0Q:quarry.crawl.seed` with metadata `max_pages` 3. The wire test in `handoff.rs` covers the id rule. |
| BI.3 | handoff fails (control stopped): no usage event | PASS | `docker stop quarry-control` at 20:08:02Z. `POST /v1/crawl` without key: HTTP 502 in 4.1 s, `UPSTREAM_BLOCKED`, "control unreachable: error sending request", `retryable:true`. With the idempotency header set to the test value from `L-SC7-billing-key.txt` plus a fail suffix: HTTP 502 in 3.1 s, same body. `docker logs quarry-edge --since 20:08:02Z | grep usage` printed 0 lines (count 0). `quarry_v2.jobs`: 0 rows with key `sc7-live-handoff-fail-1`. Control started at 20:08:13Z, healthy at 20:08:19Z. |
| BI.4 | after recovery, billing path works again | PASS | A new crawl (`job_01M6221N7M2VT55HWFA3SBTR6M`, `max_pages` 2) gave HTTP 200, completed, and one `usage metered` line at 20:08:26Z. Control, edge and orchestrator healthy afterwards. |
| BI.5 | what billing-core does with the dev stub org (`org_placeholder`) | NOT-RUN (no event reaches billing-core on live) | See Depth 2: the edge's NATS is not billing-core's NATS. No refusal could be observed because billing-core never received the events. billing-core logs since 20:05Z: no lines. `billing_service.billing_usage_dedup`: 152 rows before and after, 0 with `event_id like 'quarry:%'`, 0 rows processed since 20:05Z. |
| BI.6 | corrected-rules path (`QUARRY_EDGE__USAGE_BROWSER_METRICS=on`) | NOT-RUN | Not asked for; no such flag change. Both branches take the id from `billable_job_id`. |
| FLAGS | flags back to baseline, edge healthy | PASS | See header. `flags-log.md` has the change and the revert. No `qlive-helper-*` container was started or exists. |

## Compared with qv2e

Source: `../L-SC7-billing.md`.

- SC7.1, SC7.2, SC7.3, SC7.4: same results as qv2e. Same limits shape (`requested`/`effective`/`clamped_by`), same absent `constraints` and `clamped[]`.
- BI.1 and BI.3: same results (same control status codes 201 then 200; 502 with `UPSTREAM_BLOCKED` when control is down; no job row).
- BI.2: different in strength. On qv2e a plain NATS subscriber saw the event id `quarry:<job_id>:quarry.crawl.seed`. On live I only saw the edge's own `usage metered` line (count, subject, metric), so the id is PARTIAL. The count result is the same: one event for the keyed pair, none for the replay, none for the failed handoff.
- New on live (not on qv2e): the edge's usage events do not reach billing-core. See Depth 2.

## Depth

1. **No `constraints` or `clamped[]` in the crawl answer** (same as qv2e). The answer reports a clamp only through `limits.*.clamped_by`. A default clamp is not repeated as a warning. A client that wants to know that `max_pages:0` became 1000 reads `limits.max_pages.requested` and `clamped_by`. Not a FAIL.
2. **On live dev, quarry-edge usage events cannot reach billing-core (wiring finding).**
   - The edge publishes to `QUARRY_EDGE__NATS_URL=nats://nats:4222`, which is container `ingestion-nats` (alias `nats` on `ingestion-net`). That server has one JetStream stream, `QUARRY_EVENTS` (`quarry.>`), and no stream for `usage.>`, and no subscriber on `usage.*` (checked with the monitoring port 8222: `/jsz`, `/subsz`).
   - billing-core subscribes to `usage.>` on `NATS_URL=nats://controlplane-nats:4222` (`apps/Control Plane/billing-core/internal/nats/subscriber.go` line 41). That server has the stream `CONTROL_PLANE_EVENTS` with `usage.>`. Its last message is from 2026-10-27/28 16:44Z, before this leg. `quarry-edge` is not among its 9 connections (`/connz`). billing-core is on `controlplane-net` and `inter-plane-bus`, not on `ingestion-net`.
   - So every `quarry.crawl.seed` event goes to a NATS server where nothing stores or reads it. The publish still shows as "metered" in the edge log, because the edge drops the ack future (`usage.rs`, `Ok(_ack)` branch): a publish with no stream is never reported as a failure. No warning appears. The same holds for the other usage metrics.
   - Effect: the edge is correct (one event per created job, none for a replay or failed handoff), but nothing is billed on live. Whether this is intended for dev or a real gap is the owner's call. A fix is wiring, not this leg's job: publish to the Control Plane NATS (the edge has the inter-plane-bus network), or bridge the subject. I did not check for a bridge outside the repo grep that timed out; no container named bridge/leaf runs.
3. **A replay is invisible on the edge wire** (same as qv2e). Both answers are HTTP 200 with the same body shape. Only Control's status (201 then 200) and the missing usage event show it. `HandoffAck.replayed` is not serialised.
4. **Control-down failure is clean**: 502 in 3 to 4 s, no job row, no usage event, same for keyed and unkeyed requests.
5. **Dropped publish acknowledgement hides failures** (see Depth 2): `NatsUsageMeter::meter` logs a failure only when the publish call itself fails, never when the server has no stream for the subject. A debug-level line is the only trace, and it carries no event id.

## Not run

- BI.2 event id on the wire: needs a NATS token for a subscriber on `ingestion-nats`. Secrets stay in the containers, so the lead can run it with the user if wanted (subscribe to `usage.>` on `ingestion-nats`, repeat the keyed pair).
- BI.5 billing-core's answer to `org_placeholder`: no event reaches it (Depth 2).
- BI.6 usage mode `on`: not asked for.
- SC7.3: the fields do not exist.
