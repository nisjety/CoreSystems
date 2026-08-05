# NATS Subject Tree — Phase 0 Freeze

**Source of truth:** [`rust/crates/mp-events/src/subjects.rs`](../../rust/crates/mp-events/src/subjects.rs)
**Go mirror:** `go/pkg/natsx` (must stay in sync).

## 1. Canonical prefix

All Model Plane v2 subjects live under:

```
mp.v1.*
```

Anything outside `mp.v1.*` is either a legacy compatibility mirror (see §4) or a service-local
subject that is **not** part of the public contract.

## 2. Canonical subjects

| Purpose | Subject pattern | Wildcard (consumers) | Builder |
|---------|-----------------|----------------------|---------|
| Run events | `mp.v1.run.{run_id}.event` | `mp.v1.run.*.event` | `run_event_subject(run_id)` |
| Session commands | `mp.v1.session.{session_key}.command` | `mp.v1.session.*.command` | `session_command_subject(session_key)` |
| Ingress events | `mp.v1.ingress.{kind}` | `mp.v1.ingress.*` | `ingress_subject(kind)` |
| Usage events | `mp.v1.usage.{org_id}` | `mp.v1.usage.*` | `usage_subject(org_id)` |
| Stream lifecycle | `mp.v1.stream.{kind}` | `mp.v1.stream.*` | `stream_subject(kind)` |
| Generic event | `mp.v1.{event_type}.{resource_id}` | — | `event_subject(event_type, resource_id)` |

### Constants exported

- `PREFIX = "mp.v1"`
- `RUN_EVENTS_WILDCARD`, `SESSION_COMMANDS_WILDCARD`, `INGRESS_WILDCARD`
- `USAGE_WILDCARD`, `STREAM_WILDCARD`

## 3. JetStream streams (binding)

| Stream name | Subjects | Retention | Consumer pattern |
|-------------|----------|-----------|------------------|
| `MP_RUN_EVENTS` | `mp.v1.run.*.event` | work-queue + limits | durable per consumer service |
| `MP_SESSION_CMDS` | `mp.v1.session.*.command` | work-queue | durable, queue group `session-core` |
| `MP_INGRESS` | `mp.v1.ingress.*` | work-queue | durable per ingress sink |
| `MP_USAGE` | `mp.v1.usage.*` | interest | durable per billing sink |
| `MP_STREAM` | `mp.v1.stream.*` | interest | fanout |

Stream definitions live in `deploy/nats/streams/*.json` and are the canonical binding —
this table is the contract they must satisfy.

## 4. Legacy compatibility

During cutover from the v1 prefix to `mp.v1.*`, the following legacy subjects MUST remain
routable. `mp-events::subjects` and `go/pkg/natsx` expose translation and subscription helpers.

| Legacy subject | Canonical equivalent |
|----------------|----------------------|
| `verevon.agent.run.{run_id}.event` | `mp.v1.run.{run_id}.event` |
| `verevon.session.{session_key}.command` | `mp.v1.session.{session_key}.command` |
| `aqencia.reasoning.reasoning.started` | `mp.v1.ingress.run_started_compat` |
| `aqencia.reasoning.reasoning.completed` | `mp.v1.ingress.run_completed_compat` |
| `aqencia.reasoning.usage.recorded` | `mp.v1.ingress.usage` |
| `aqencia.reasoning.decision.made` | `mp.v1.ingress.decision` |
| `aqencia.reasoning.quota.exceeded` | `mp.v1.ingress.quota_exceeded` |

### Compatibility modes

Controlled by env var `MP_COMPAT_MODE` (enum `CompatMode`):

| Mode | Publisher behaviour | Subscriber behaviour |
|------|---------------------|----------------------|
| `v1_only` (default) | canonical only | canonical only |
| `dual_write` | canonical + legacy mirror | canonical only |
| `dual_read` | canonical only | canonical + legacy mirror |
| `legacy_only` | legacy only | legacy only |

Cutover sequence (must be executed in order across the fleet):
1. Everyone on `v1_only` (pre-cutover steady state on old prefix — N/A for greenfield).
2. Publishers → `dual_write`; subscribers remain.
3. Subscribers → `dual_read`.
4. Publishers → `v1_only`.
5. Subscribers → `v1_only` (legacy streams can be deleted).

## 5. Invariants

1. All canonical subjects start with `mp.v1.`.
2. Subject tokens are lowercase ASCII; use `_` to separate words, `.` only to separate levels.
3. Resource IDs in subjects MUST be ULIDs (or other `mp-ids` string forms) with no `.` characters.
4. Publishers MUST call the builder helpers — never concatenate by hand.
5. Subscribers MUST request subjects through `subscriber_subjects(canonical, mode)` to get
   mode-correct sets.
6. `translate_legacy_subject` and `translate_new_to_legacy` MUST be lossless for every row in §4.

## 6. Review checklist

- [x] Every new event type has a subject rule in §2 before it can be published.
- [x] Every legacy subject used in production has a mapping in §4.
- [x] Rust `subjects.rs` and Go `natsx` pass parity tests (`compat_mode_parse_matches_go_contract`,
      `translate_legacy_subject_matches_go_contract`, `subscriber_subjects_match_go_subscriber_modes`).
