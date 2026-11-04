# W2-CP-U37 status (lane a)

Unit: usage transport to billing-core, Lago codes, usage_test pin (U37). Planes: Control (billing-core) and Ingestion (quarry-runtime, quarry-edge, compose).

## Built

Built to scope, code only, not compiled, no test run. Before this unit: the edge meter published to the Quarry bus only (`QUARRY_EDGE__NATS_URL`, stream `QUARRY_EVENTS` over `quarry.>`, ack future dropped, so `usage.>` events never reached billing-core); billing-core subscribed `usage.>` as a literal; no Lago codes or usage_test pin for the seven metrics existed. The id rule (`validate_event_id`, keyed ids) and the seven-metric list already existed in usage.rs (U37 Quarry half) and were not rebuilt.

- quarry-runtime `usage.rs`: `UsageBusConfig` (`QUARRY_EDGE__USAGE_BUS_URL`, `_USER`, `_PASSWORD`; unset or blank URL = `None`; half-set credentials refused; Debug redacts the password and URL userinfo), `NatsUsageMeter::connect_usage_bus` (no stream creation, inbox `_INBOX.QUARRY_CONTROL`), `await_ack` mode: `Nats-Msg-Id = event_id`, JetStream ack awaited, 3 attempts with 250 ms x attempt back-off, the flush guard held across retries. `from_bus` unchanged (unacked). Tests `usage_bus_tests` (7).
- quarry-edge `main.rs`: the usage bus meter wins when configured; a connect failure falls back to today's path with a warning; a config error stops startup. The URL is never logged.
- Ingestion `docker-compose.yml`: `QUARRY_EDGE__USAGE_BUS_URL`, `_USER`, `_PASSWORD` passed through, empty by default. `QUARRY_EDGE__USAGE_BROWSER_METRICS` stays unset (off).
- billing-core `usage.go`: `UsageSubjectFilter = "usage.>"`, `UsageSubjectForMetric`, `QuarryUsageSource`, `QuarryBrowserMetrics` (the seven, Rust order), `LagoBillableMetricCode` (code = metric name, because the Lago adapter sends `usage.Metric` as `code`). `subscriber.go` subscribes `billing.UsageSubjectFilter` (same value, no behaviour change).
- Tests: `TestUsageSubjectAndQuarryMetricsArePinned`, `TestUsageEventIDPatternAcceptsQuarryRunEvents` (usage_test.go); `TestDecodeUsageMessageAcceptsQuarryEdgeUsageBusEvent`, `TestSubscriberUsesThePinnedUsageSubject` (usage_subscriber_test.go).

## Commits (lane a, local)

- e16fb43d6 feat(quarry-runtime): acked usage bus meter for billing-core (W2-CP-U37)
- 8cc39aab3 feat(quarry-edge): route usage to the Control usage bus when configured (W2-CP-U37)
- be980fb2f feat(billing-core): pin the usage subject, Quarry ids and Lago codes (W2-CP-U37)
- status and depth rows: the commit that adds this file

Code only, not compiled.

## Flag and default

`QUARRY_EDGE__USAGE_BUS_URL`, default unset = today's behaviour (usage on the Quarry bus, unacked). `QUARRY_EDGE__USAGE_BROWSER_METRICS` stays off in compose.

## Assumed decisions

- Queue decision 22 (option b): the edge publishes to the Control NATS billing-core subscribes and awaits the ack; billing-core keeps its plain `usage.>` subscription.
- Subject namespace: the controlplane.* cutover moved the shared bus subjects only; on the Control NATS billing-core still holds `usage.>` and stream `CONTROL_PLANE_EVENTS` still covers `usage.>` (audit-core provisioner), so the edge publishes `usage.quarry.<metric>` unchanged.
- Credentials: separate `_USER` / `_PASSWORD` env (Control NATS uses one user and password per service); async-nats 0.49 URL userinfo was not relied on.
- Lago code = metric name (Quarry BILLING.md: codes must match exactly; the Lago adapter sends `usage.Metric`). No adapter change.
- Bounded retry (3 attempts) rather than retry-forever, so the 5 s shutdown flush still ends; a lost event after 3 attempts is logged (warn) and recoverable through the billing-core HTTP usage endpoint.
- Connect failure falls back to today's path instead of stopping the edge (same as the Quarry NATS connect).

## MP asks

None.

## Not verified / outside the unit

- Not compiled, no cargo/go test run (go test ./internal/billing ./internal/nats; qtest -p quarry-runtime usage are for the integration step).
- Control `nats.conf` has no `quarry-edge-control` user: it needs publish `usage.quarry.>` and subscribe `_INBOX.QUARRY_CONTROL.>`, plus a password in the Control secrets ledger. Not in this unit's files (depth row).
- Network: quarry-edge must reach `controlplane-nats` (inter-plane-bus or a Control network); not checked against the Control compose.
- Lago must register the seven billable-metric codes before `USAGE_BROWSER_METRICS=on` reaches invoices.
- Live proof (D2-241 evidence steps) not run.
