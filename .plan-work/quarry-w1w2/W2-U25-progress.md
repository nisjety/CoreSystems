# W2-U25 progress: WS10.S3 (Quarry trace core: per-run seq, replay ring, resume, gateway events proxy)

Status: BUILT to the plan's scope on branch quarryv2-next (local, not pushed). Compile checks run where stated below. Tests WRITTEN and compile-checked (as part of the crate checks), NOT run (test phase at the end of the wave). Flags: `QUARRY_EDGE__TRACE_RING_EVENTS` default 512 (0 disables the ring), `QUARRY_EDGE__TRACE_RING_MAX_BYTES` default 1 MiB (must be at least 1). No migration, no proto change, no Cargo change.

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS10.S3 detail: Quarry trace core". Queue: W2-queue.md section 3, BATCH 5, U25. Depth: .plan-work/quarry-w1w2/depth/W2-U25.md. Runbook: docs/RUNBOOKS.md, section "Agent event trace: per-run seq, replay ring and resume (WS10.S3, unit W2-U25, built, NOT run, 2026-10-23)".

## Commits (in order, local)

| Commit | Subject | Content |
|---|---|---|
| f9f8ee3ab | feat(quarry-runtime): per-run event seq, replay ring and resume (WS10.S3) | events.rs (trace, ring, open/close_run, subscribe_with_replay, mint, 16 tests), event_bus.rs (publish_event default), nats_event_bus.rs (publish_event; publish_encoded shared with publish), runtime lib.rs (re-export line only). This commit stored the three events/event_bus/nats files with CRLF bytes (the repo stores LF). |
| 0c5423b6e | style(quarry-runtime): store the WS10.S3 event files with LF line endings (WS10.S3) | the same three files as LF; no code change. Net change against f9f8ee3ab^ is the feature. |
| 958fd00ca | docs(quarry-v2): events stream contract, seq and resume, RUNBOOKS section (WS10.S3) | openapi.yaml (events stream path), CONTRACTS.md (section 3 subsection), RUNBOOKS.md (built from an older parent: it dropped the WS9.C and WS13.4a sections). |
| 33344282a | docs(quarry-v2): restore the RUNBOOKS sections dropped by 958fd00ca (WS10.S3) | a second rebuild from the same wrong parent: did not restore the sections and appended the WS10.S3 section twice. Superseded by the next commit. |
| 9340a5eca | docs(quarry-v2): RUNBOOKS, restore the sections dropped by 958fd00ca (WS10.S3) | RUNBOOKS.md rebuilt from 958fd00ca^ (WS9.C and WS13.4a present) plus the WS10.S3 section, once. Against 958fd00ca^ the net change is the section, plus a CR-to-LF change in the WS9.C heading block (that parent mixed line endings). |
| 4addc8747 | feat(gateway): proxy the agent run events stream with Last-Event-ID (WS10.S3) | gateway domains/browser.rs (route, stream_agent_events, two tests) and SPA shared/api/browser-client.ts (streamBrowserEvents). Built from HEAD plus these hunks. |
| 16d27999c | feat(quarry-edge): trace ring configuration and the resume stream tests (WS10.S3) | trace_config.rs (NEW, 5 tests), tests/agent_events_replay.rs (NEW, 6 tests). |
| 0d05adc6c | feat(quarry-edge): resumable agent event stream and per-run trace (WS10.S3) | agent_routes/live.rs (agent_events_stream rewritten, replay_gap_event, last_event_id), agent_routes/close.rs (close_run in finish_run), agent_routes/lifecycle.rs (open_run before AgentStarted), agent_routes/queries.rs (doc), quarry-edge lib.rs (pub mod), main.rs (boot read, sink wiring). Built from HEAD plus these hunks. |

Later, 1eb88a6ff (the WS7.D unit, LF blob fix) rebuilt lifecycle.rs, lib.rs and main.rs from an older HEAD and removed three of this unit's hunks (open_run before AgentStarted, the trace_config mod line, the boot read and the sink wiring). 1935c2c8f (another unit) restored them on the current HEAD; checked against HEAD: open_run is in lifecycle.rs, the pub mod is in lib.rs, the mod line, the boot read and with_ring_limits are in main.rs. Reading the commit diffs confirmed the other WS10.S3 files were intact.

Commit hygiene, recorded for the next units: `qcommit-blobs.sh` stores the bytes it is given (`git hash-object --no-filters`), so give it LF content, and build blob sources from the CURRENT HEAD immediately before committing. Two blob commits here were built from an older HEAD and removed other units' RUNBOOKS sections; both were restored by later commits (no amend, no reset). `qcommit-blobs.sh` also failed on a NEW file (`update-index --cacheinfo ... cannot add`); new files went through `qcommit.sh` instead. Nothing was committed by the failed attempt.

## What the slice does (as built)

- `EventSink` (quarry-runtime `events.rs`): `open_run(run, start_seq)` gives a run its trace (dense seq from `start_seq`; 1 for an agent run). `mint` numbers the event, writes it to a bounded replay ring (`RingLimits`: events and estimated bytes, oldest evicted first) and broadcasts it, all under one per-run lock. `subscribe_with_replay(run, from_seq)` snapshots the ring and subscribes under that lock (no gap, no duplicate); it returns `ReplayGap {requested, oldest_retained}` when the ring no longer holds the events after `from_seq`, and `None` for a run without a trace. `close_run` drops the ring and the sender (live receivers see `Closed` after the events already sent). Runs never opened keep the global counter and have no entry. `emit_ephemeral` and the ZDR path keep events in the ring only. `with_ring_limits` sets the bounds.
- `EventBus::publish_event(Event)` (provided, defaults to `publish`). The NATS bus publishes the minted event (the same event id and seq as the SSE path) through `publish_encoded`, which `publish` shares. NATS still receives every non-ZDR event, as before (the WS10.S4 exclusion is not built; see depth).
- Edge `trace_config.rs` (NEW, env only): the two knobs; an unparsable value, or a zero byte bound, aborts boot naming the variable.
- Edge `agent_routes/lifecycle.rs`: `open_run(&run_id, 1)` after the slot is committed and before `AgentStarted`. Edge `agent_routes/close.rs`: `close_run(&rid)` after the terminal event in `finish_run` (owner close and sweeper expiry). The drain path (`drain_runs`) does not close traces (depth).
- Edge `agent_routes/live.rs`: `agent_events_stream` resumes with `?from_seq=` (wins) or `Last-Event-ID`; sends `replay_gap` first when needed; replays the ring, then the live tail; a `seq <= last_seen` guard drops repeats (the guard is off for a live-only connection); on `Lagged` it re-snapshots from the ring after the last seq sent; `Closed` ends with `done` / `run_closed`. The doc comment no longer names a `quarry_event_history` table.
- Gateway `domains/browser.rs`: `GET /api/v1/browser/sessions/{session_id}/events/stream` (route beside frames/stream), `stream_agent_events` (the owner check is Quarry's projection call; the stream is proxied with `Last-Event-ID`, the query and the ZDR flag).
- SPA `shared/api/browser-client.ts`: `streamBrowserEvents`, the `BrowserTraceEvent` and `BrowserReplayGap` types and the handler contract. No UI uses it, so no `shared/actions` contract was added.
- Contracts: openapi.yaml path `/v1/agent/runs/{run_id}/events/stream` (additive, GET, `streamAgentRunEvents`); CONTRACTS.md section 3 subsection (SSE id = seq, replay_gap, done, envelope type versus SSE names).

## Checks run (compile only; no test binary was run)

- `cargo check --tests -p quarry-runtime` (qtest, quarry-rust-env:2, working tree at the time): rc 0; no error and no unused-warning lines in the filtered output.
- `cargo check -p quarry-edge --tests` (qtest, working tree, default features incl. browser-agent): rc 0; zero `error` lines; no warning in any file of this unit (the warnings in the log are in other units' or pre-existing files: run_live.rs, zdr_tombstone.rs, browser_grants_prod.rs, profile_guard.rs).
- Gateway `cargo check --tests` (native, in `apps/Frontend Plane/verevonv3/apps/gateway`, working tree): two type errors in `sanitize_action` (the `evaluate` arm), from another unit's uncommitted change (HEAD has the two-element tuple). None in this unit's handler or tests.
- Not run: any test binary, `cargo test`, `pnpm typecheck`, the gateway tests, the SPA tests, `go`. Blob-built commits were not compiled in isolation (checked by diff against HEAD only).

## Tests written (NOT run)

- quarry-runtime `events::tests` (16): `emit_without_nats_only_hits_mpsc`, `emit_with_nats_fans_out_to_both`, `emit_ephemeral_reaches_subscriber_without_durable_channels`, `emit_for_zdr_routes_restrictive_events_only_to_live_subscribers`, `emit_continues_when_nats_publish_fails`, `agent_run_seq_starts_at_one_and_is_dense`, `two_runs_have_independent_seq`, `a_run_that_was_never_opened_keeps_the_global_counter_and_adds_no_map_entry`, `ring_is_bounded_by_events_and_by_bytes`, `replay_returns_only_events_after_from_seq`, `replay_then_live_has_no_gap_or_duplicate_under_concurrent_emit` (8 emitters, 400 events, multi-thread runtime), `replay_reports_a_gap_when_from_seq_is_older_than_the_ring`, `a_lagged_receiver_recovers_from_the_ring`, `close_run_closes_subscribers_and_frees_state`, `a_zdr_event_never_reaches_nats_or_mpsc_but_is_replayable_from_the_ring`, `nats_receives_the_same_event_id_and_seq_as_the_sse_path` (with a recording bus; the real JetStream path is not exercised). `event_bus::tests` unchanged (2).
- quarry-edge `trace_config::tests` (5): defaults, zero events disables, zero bytes refused (names both knobs), unparsable names the variable, blank is the default.
- quarry-edge `tests/agent_events_replay.rs` (6, `browser-agent` feature): `reconnect_with_last_event_id_replays_missed_events_then_tails`, `from_seq_wins_over_last_event_id`, `replay_gap_is_sent_first_when_the_ring_no_longer_holds_the_resume_point`, `a_non_owner_gets_404_not_a_replay`, `a_closed_run_stream_ends_with_done_run_closed`, `events_stream_doc_comment_names_quarry_event_history_absent` (source scan).
- Gateway `domains::browser::tests` (2): `browser_events_stream_proxies_last_event_id_and_query_to_quarry`, `browser_events_stream_refuses_a_non_owner`.
- SPA: none written (`pnpm typecheck` is the plan's named check and is not run).

## Hand-over: what the next batch needs to know

1. The trace seq is per process. A resumed run (`resume_run_id` after an edge restart) opens at seq 1 again; WS10.S4 adds `trace_seq` to the checkpoint. Until then a client should treat a resumed run's stream as new.
2. WS10.S4 must add the `is_agent_class` exclusion in `nats_event_bus.rs publish_event` (D10). Until then agent events reach NATS raw for non-ZDR runs (unchanged from before this unit).
3. Only `start_run` opens a trace. A new agent-run entry point must call `open_run` before its first event, or its events get the global counter and no replay.
4. `EventBus` has a new provided method `publish_event`. An implementor that overrides `publish` keeps working through the default; one that wants the sink's id and seq must override `publish_event`.
5. The gateway route is `/api/v1/browser/sessions/{session_id}/events/stream`; WS10.S8 and the browser timeline work consume it through `streamBrowserEvents`.
6. The `shared/actions` contract for a UI consumer is not written: add it with the first UI that calls `streamBrowserEvents`.
7. The gateway forwards the events-stream query string unchanged. Quarry reads only `from_seq` and `max_events`; an allowlist is a depth item.
