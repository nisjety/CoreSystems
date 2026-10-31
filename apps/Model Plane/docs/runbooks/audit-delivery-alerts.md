# Audit delivery alerts

Context: governed agent actions are recorded by session-core and published through an outbox to audit-core. Postmortem `docs/postmortem/0002-session-audit-trail-never-delivered.md` records how they went undelivered for weeks with no alarm. The rules live in `apps/Control Plane/observability/rules/audit-delivery.rules.yml` and are evaluated by `controlplane-prometheus` (UI on the host port `CONTROL_PROMETHEUS_HOST_PORT`, default 19095). There is no pager yet: an alert is visible only in that UI until a routing target is chosen.

All series are content-free (bounded `subject_class` and `reason` labels). Nothing below reads audit payloads.

## terminal-rows
`ModelAuditOutboxTerminalRows` / `ModelAuditOutboxTerminalIncreasing`: rows in `session_core.audit_outbox` gave up publishing. First read: `mp_session_audit_outbox_terminal_by_subject` by `subject_class`, then the reason counters `mp_session_audit_outbox_publish_failures_total{reason}`. Typical causes: the publisher image predates the fix of commit b9ed9197 (rows are stamped with a subject the stream does not carry), the JetStream stream is missing, or the publish credential lacks the subject. Replay of old terminal rows is a deliberate operator step (`requeue_session_audit_terminal`, migration 0043); see the postmortem procedure. Owner: Model Plane operator.

## gauge-absent
`ModelAuditOutboxGaugeAbsent`: the gauge series is missing for 10 minutes. Either the reporter stopped or Prometheus cannot scrape `model-session-core:8081`. Check the scrape target under Status > Targets and `docker network inspect inter-plane-bus` for the alias `model-session-core`.

## backlog
`ModelAuditOutboxBacklog`: the oldest unpublished event is older than 15 minutes (proposed threshold, tune after a week of data). Check the broker (`model-nats`) and the audit-core consumer lag (`audit_core_consumer_pending`).

## topology
`ModelAuditTopologyFailure`: publishing fails with `stream_not_found` or `permission_denied`. The `nats-provisioner` stream manifest or `nats.conf` publish permission for session-core is wrong. Fix the manifest, not the alert.
