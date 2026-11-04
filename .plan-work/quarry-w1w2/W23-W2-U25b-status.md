# W2-U25b status: agent events excluded from raw JetStream publish (D2-098)

## Built
Built. `is_agent_class(&EventType)` added to `crates/quarry-runtime/src/nats_event_bus.rs` (exhaustive match, no wildcard arm: a new `EventType` variant is a compile error until classified). The gate sits at the top of `publish_encoded`, which both `publish` and `publish_event` call, so agent-class events return Ok without any `js.publish`. Agent class: agent_started, action_started/completed/failed, observation_ready, agent_completed/failed/expired, agent_budget_exhausted, agent_attached/detached (11). Run, page, crawl, store, lease, profile, change, schedule, search, host, branding, control events (30) publish as before. Module doc updated.

Nothing existed before (grep for is_agent_class found nothing), so this is the whole unit.

## Commits
See `git log` on w23/lane-b (code commit, then this status file).

## Tests (qtest lane b, `cargo test -p quarry-runtime --lib -- nats_event_bus events:: event_bus agent_loop`)
38 passed, 0 failed. New (4, nats_event_bus::tests): agent_class_events_are_not_published_raw, run_and_crawl_events_are_published_as_before, the_two_lists_cover_every_event_type_without_overlap (11 + 30 = 41 variants, no duplicates), both_publish_entry_points_pass_through_the_agent_gate (source scan: gate precedes the first JetStream publish). ZDR path unchanged: existing events::tests (a_zdr_event_never_reaches_nats_or_mpsc_but_is_replayable_from_the_ring, emit_for_zdr_routes_restrictive_events_only_to_live_subscribers, emit_with_nats_fans_out_to_both) and agent_loop event-bus tests pass.

## Flag and default
No flag (leak fix, per the queue). Always on.

## Assumed decisions
- The exclusion applies in `publish_encoded`, not only `publish_event`, so `EventBus::publish` cannot leak either.
- Agent-class events are dropped for ZDR and non-ZDR alike (ZDR already never reached the bus).
- Attach/detach and budget_exhausted count as agent class (they carry tab/attach ids and spend detail).
- Dropping returns Ok (no error, no log): the event still reaches SSE and the replay ring.

## Model Plane asks
None. No consumer of `quarry.run.*.agent_*` / `quarry.events.agent_*` subjects was found in apps/Model Plane or Quarry-v2 (grep of event names). WS10.S4's redacted projection is the planned cross-plane path.

## Not verified
- The real JetStream publish path (needs a broker; the gate is checked by a source scan plus the pure classifier tests).
- That no external consumer relied on agent events from the aggregate subject (grep only, in-repo).
- Existing JetStream messages already published stay in QUARRY_EVENTS until the 7-day max_age expires.
