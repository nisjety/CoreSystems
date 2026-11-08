# WS06-audit-tamper-evidence - Audit hash chain, compliance API, SIEM export

Item: F-t3-004, after phase 0 (phase 0 = deploy publisher fix, replay 4,992 rows, alert rules on the Model terminal-row gauge: WS01). Wave W1. Date of reading: 2026-10-15. Worktree = commit afb83928 (clean for every file named here); live tree has no uncommitted change in audit-core, org-core, the gateway audit domain, `docker-compose.yml` (Control) or `run-control-plane.sh` (checked against `.plan-work/live-tree-status.txt`).

## 1. Scope, items resolved, owner

| | |
|---|---|
| Items | F-t3-004 (tamper-evidence half). Phase 0 stays WS01. |
| Owner plane | **Control Plane**: audit-core (chain, retention, compliance API, SIEM exporter), org-core (SIEM destination config). Frontend Plane gateway only proxies, admin-gated. |
| Model Plane | **No code change.** It keeps delivering flat `verevon.audit.v2.model.session-core.*` events through the outbox (`session-core/src/audit_publisher.rs:34-43, 298-329`). The chain is applied inside audit-core after the event arrives. Rows stay content-free (ZDR). |
| IN | Per-org SHA-256 hash chain written in the single insert chokepoint; signed head attestations; chain-safe retention with signed checkpoints; scheduled + on-demand verifier + offline verifier CLI; admin-only compliance read API with time-range export and signed manifest; SIEM fan-out (exporter process) whose destinations are configured in org-core; alert rules; egress declaration; evidence. |
| OUT | Phase 0 ops (WS01). Role-gating the *existing* `GET /v1/audit` and the admin audit view (F-t1-031, Space session). Per-org retention windows and legal hold (F-t3-062, W3). GDPR erasure of audit rows (no path exists today; see D8). Completeness proof against producers (chain proves integrity of persisted rows, not that every event arrived). KMS/HSM custody. A Channel Plane anything. |

**FINISH vs BUILD.** Almost nothing here is built; the FINISH part is small and ships first.

| Slice | Tag | One line |
|---|---|---|
| S0 | [FINISH] | Control Prometheus rules for audit-core metrics that already exist |
| S1 | [BUILD] | ADR, canonical encoding library with golden vectors, audit-core CI |
| S2 | [BUILD] | Serialised per-org chained insert (both insert paths) |
| S3 | [BUILD] | Signed head attestations and chain-safe retention |
| S4 | [BUILD] | Verifier (scheduled, API, offline CLI) and chain alerts |
| S5 | [BUILD] | Compliance read API in audit-core |
| S6 | [BUILD] | Gateway compliance routes (+ UI, conditional on D6) |
| S7 | [BUILD] | org-core SIEM destination config, sealed secrets, entitlement |
| S8 | [BUILD] | SIEM exporter service |
| S9 | [FINISH] | Egress declaration, ZDR/honest-limits/trust-claim evidence, runbook |

## 2. Verified current state

All paths relative to repo root, lines opened in the worktree.

**audit-core insert path (`apps/Control Plane/audit-core`)**
- One chokepoint: `internal/store/store.go:77-111` `insertAudit`. `InsertAudit` (43), `InsertAuditFromSource` (52, HTTP path), `InsertAuditFromStream` (64, JetStream path) all call it. It is a single `INSERT ... ON CONFLICT DO NOTHING RETURNING id` on the pool (94-106), with no transaction and no per-org lock. `payload_hash = sha256(json.Marshal(ev))` (88-92) is replay idempotency only; conflict resolver 113-139.
- Schema: `internal/store/schema.sql:11-33` (no chain column; `ingested_at DEFAULT NOW()` = transaction start time, so it is not commit-ordered); indexes 35-43; unique indexes in `migrations/006_v2_producer_authority.sql:18-20`. Migrations are an explicit `//go:embed` list + slice (`internal/store/migrate.go:12-28, 36-43`), applied once each in a tx with a `schema_migrations` ledger (54-102). `audit-core/migrations/001_init.sql` is not embedded (unreferenced).
- Retention: `Purge` hard-deletes by `ingested_at` (`store.go:243-269`), driven by `cmd/server/main.go:157-187` (once at boot, then daily, every replica), window from `AUDIT_RETENTION_DAYS` (default 365, `main.go:246-255`).
- Read: `ListAudit` (`store.go:299-346`); limit <=0 or >500 resets to **100** (audit text says "capped at 500": loosely wrong); `AuditRow` carries no ip/user-agent. HTTP: `internal/api/api.go:105-117` mount, `197-222` `listAudit`, `278-316` `ingestAudit`.
- Auth: `internal/api/service_auth.go`: scopes `audit:read:self` / `audit:write` parsed at 108-115; required registry 140-158; path to scope map 203-212; read requires v3 HMAC delegation incl. user, org and **role** (263-274, 283-351). The role is verified and carried in `serviceAuthorization.UserRole` (47) but **no handler consults it** (this is the F-t1-031 defect: any org member reads the whole org log through the gateway).
- Ingest authority: subscriber binds subject plane+producer+event to the payload (`subscriber.go:287-295`); malformed / authority-mismatch / exhausted events are dead-lettered, not stored (`handleAudit` 160-215). So the chain can only attest to persisted rows.
- Container: `Dockerfile:34-49` builds four binaries from one module (`server`, `nats-provisioner`, `control-event-bridge`, `nats-consumer-migrate`); compose `audit-core` service (`apps/Control Plane/docker-compose.yml:806-870`) has 128M / 0.25 CPU; `AUDIT_CORE_SERVICE_CREDENTIALS` literal at 829. A second audit-core-image service already exists as pattern (`audit-nats-provisioner`, 767-781).
- No CI runs audit-core Go tests (only path filters in `model-plane-rust-ci.yml:36,49`). `org-service.yml` **does** target org-core (`paths: apps/Control Plane/org-core/**`, line 17-24): resolves the sweep's open question.
- Metrics that exist: `audit_core_events_total{bus,kind,result}`, `audit_core_consumer_pending`, `audit_core_nats_connected` (`internal/metrics/server.go:22-70`). `apps/Control Plane/observability/prometheus.yml` has scrape jobs for audit-core and org-core and **no `rule_files`**; no rules directory exists in Control (only `apps/Data Plane v2/infra/prometheus/alerts.yml` repo-wide).

**Producer side (Model Plane, read-only here)**: body builder `audit_publisher.rs:298-329` emits `{event_id, occurred_at, org_id, user_id, plane, producer, event, subject, resource_id, outcome, details:{tool,data_category,zdr,phase}}` (content-free). `docs/ZDR.md:95` and `:259`: ZDR-derived envelopes are suppressed, no durable audit envelope for a ZDR run. [live-only] `session-core/src/retention_receipt.rs` adds a content-free `run_started_zdr_waived` event (const 34, body 410-438): it enters the chain like any other event; WS06 needs nothing landed from it.

**org-core**: gin router `internal/http/server.go:99-179`; machine auth `internal/http/service_auth.go` with a scope allowlist (27-45; `:self` scopes only for `verevon-gateway`, `:any` never for it, 141-146), required-principal check (184-240), path to scope maps (248-375). Precedent for a narrow internal read consumed by another service: `GET /internal/orgs/:orgId/retention-posture` (`retention_posture_handler.go`, scope `org:retention:read:any`, `server.go:168`), whose real compose block is read by a contract test (`internal/http/retention_posture_contract_test.go:1-60`, golden vendored in `testdata/`). Migrations `NNN_x.up.sql/.down.sql` with `GRANT ... TO org_core_app` and RLS on `app.current_org` (`migrations/019_interactive_retention_outbox.up.sql`); org-scoped tx helper `database.WithOrgScope` (`internal/database/database.go:97`). `feature.audit_logs` entitlement exists, default enabled (`internal/org/types.go:269`); a computed plan-gated entitlement precedent exists (`InteractiveZdrEntitlementKey`, `types.go:276`). **No at-rest encryption primitive and no outbound webhook or SSRF guard anywhere in Control Plane Go** (grep for aes/gcm/secretbox/ssrf returned nothing).

**Gateway**: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs:28-105`: session-only (`require_session`), org from the live membership (`upstream.rs:324-329`), `proxy_json` to audit-core with delegation headers incl. role (`upstream.rs:348-433, 633-687`); `control_service_audience` maps the audit-core and org-core origins (`upstream.rs:955-965`). Role of the caller is the **membership** role (`middleware.rs:241-250`: `auth_role` overwritten); helper `has_authorized_org_role` (`middleware.rs:112-124`) and the `["owner","admin"]` pattern (`domains/mcp.rs:71-74`). `proxy_json` decodes JSON only (`upstream.rs:1099-1163`): no NDJSON/streaming proxy fits, so the export is paged JSON. Router merge `main.rs:99`, module list `domains/mod.rs:7`. Test pattern: wiremock + `build_router` (`main.rs:2241-2307`). SPA: `src/shared/api/audit-client.ts` (Trust Center, any member, `event=tool_action`), no compliance or export client, no action descriptor for audit.

**Secrets bootstrap precedent**: `apps/Control Plane/scripts/run-control-plane.sh:171-205` generates an Ed25519 keypair (`CONTROL_SPACE_DECISION_*`) with `persist_if_missing`; token list at 421-456; `scripts/validate-release-credentials.sh:19-42` and `scripts/control-service-env-contract-test.sh:49` enumerate required keys.

**Declared egress** = `docs/compliance/evidence/tools/scan-egress.mjs` (VENDORS catalogue, `--strict`, drift test `trust-data.test.ts`; README 103-123). The audit's "declared egress file" is this scanner, not a registry.

**Where the audit is stale or wrong**: (1) `audit_publisher.rs:432-480` cites gauges, the producer fix is at 135-179 and 298-340 (sweep). (2) "capped at 500 rows" is a reset to 100. (3) "events arrive from several buses": true, plus HTTP ingest; both go through `insertAudit`, so one change covers all. (4) The audit implies the chain lives "in audit-core's insert path" with a verifier there: correct, but `ingested_at` is tx-start so retention must prune by contiguous `chain_seq`, not by timestamp. (5) The audit lists "reuse the gateway audit.rs proxy": it has no role gate; a compliance surface cannot reuse it unchanged. (6) audit-core builds four binaries, not one.

**Must land before this stream**: nothing in code. Operationally WS01 phase 0 should be proven first (otherwise the chain attests to a log known to be incomplete); the chain can be enabled before or after replay without breakage because replays append like any event.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | Trust anchor and key custody | (a) Ed25519 key in env, generated by `run-control-plane.sh`, public key served; external witness = SIEM copy + customer export; (b) KMS/HSM; (c) no signing (hash chain only) | (a). Chain alone is forgeable by anyone who can write the DB and recompute; signature key outside the DB plus an external witness is the minimum that is honest. (b) deferred; (c) rejected. | S3, S4 (S1, S2 proceed) |
| D2 | Legacy rows | backfill into chain vs report as unchained | Do not backfill. Rows before activation are reported `legacy_unchained` with a count. Rows written while the flag was off are chained later by catch-up with `chain_mode='catchup'`. | none (S2 default) |
| D3 | Who may read, and plan gating | owner/admin only; add "auditor" role; plan-gate | owner and admin of the org (membership role, double-checked in gateway and audit-core), any plan that has `feature.audit_logs`. SIEM destinations plan-gated by a computed `feature.siem_export` (enterprise), same shape as ZDR. No new role. Needs owner ruling (commercial). | S5 (role), S7 (gating) |
| D4 | SIEM runtime | (a) separate binary `siem-exporter` in the audit-core module, own compose service; (b) goroutine inside audit-core; (c) worker inside org-core; (d) NATS fan-out | (a). Egress to customer URLs stays out of the integrity-critical 128M process; no new module or Dockerfile; cursor on `chain_seq` is the natural ordered at-least-once mechanism, so no second outbox and no nats-provisioner manifest change (this removes the audit's "nats-provisioner stream manifest" dependency for WS06). | S8 (S7 proceeds) |
| D5 | Destination secret custody | seal in org-core (AES-256-GCM, key from env, key id) vs external secret manager | Seal in org-core; write-only API; exporter receives plaintext over the authenticated internal call (same trust level as existing service tokens). No manager exists in the repo. | S7 |
| D6 | Overlap with F-t1-031 (Space) | WS06 also builds the admin UI vs Space owns it | Space session lands the role-gate fix on `GET /v1/audit` first (S-size) and owns the admin audit **view**; WS06 owns APIs, chain, export, SIEM and provides the client + action descriptors; S6 UI part is built here only if Space has not by then. | S6 UI only |
| D7 | Content guard on `details` | enforce now / observe first / none | Observe first: size cap and key deny-list counted in a metric at ingest (S2); enforce only after a week of zero hits across producers. The chain hashes `details` verbatim, so redaction at export would break external verification; the guard must act at ingest. | none |
| D8 | GDPR erasure of an org's audit rows | out of scope vs design now | Out of scope; design leaves the hook: erasure = purge-through-head with checkpoint `reason='erasure'`. Owner: F-t3-062 / org-core. No audit-core erasure path exists today (sweep, confirmed). | none |
| D9 | ADR home | Series A `apps/<TOPIC>_ADR_<date>.md` (next ADR-0005, inferred) | Series A, because it is a cross-plane product decision. Model Plane ledger not used (no Model-internal tradeoff). | none |

**Ledger reversals: none.** Grep of `apps/Model Plane/docs/decisions/ledger.md` finds no entry on hash chain, SIEM, or compliance API; nothing settled or rejected is reopened. The only existing statement touched is `docs/ZDR.md:95` (extended, not reversed). Postmortem 0002 follow-up 1 (dual ownership of `VEREVON_CONTROL_OBSERVABILITY` on the Model bus) stays open and is noted in S9, not decided here.

Open: whether the gateway can read the org entitlement cheaply for the `feature.audit_logs` check (not determined; audit-core does not call org-core on the read path).

## 4. Slices

Conventions: audit-core test command (all Postgres tests skip without a DSN): `docker run --rm -d --name ac-pg -p 55432:5432 -e POSTGRES_PASSWORD=t postgres:16` then `cd "apps/Control Plane/audit-core" && TEST_DATABASE_URL="postgres://postgres:t@localhost:55432/postgres?sslmode=disable" go vet ./... && go test ./...` (no `-race`: needs cgo, unavailable on this host). Migration names below are purpose names; the integrator assigns numbers (audit-core's next is after 006, org-core's after 020).

### S0 [FINISH] Control Prometheus rules for existing audit-core metrics
- Goal: turn the metrics that already exist into alerts (postmortem 0002 follow-up 3: `authority_mismatch` alert "would have caught root cause 2") and create the rules mechanism the later slices append to.
- Files:
  - `apps/Control Plane/observability/prometheus.yml` - add `rule_files: ['/etc/prometheus/rules/*.yml']`.
  - NEW `apps/Control Plane/observability/rules/audit-core.rules.yml` - `increase(audit_core_events_total{result="authority_mismatch"}[10m])>0`, `result="store_error"`, `result="malformed"`; `audit_core_consumer_pending>0` for 15m; `audit_core_nats_connected==0` for 5m.
  - NEW `apps/Control Plane/observability/rules/audit-core.rules.test.yml` - `promtool test rules` cases.
  - `apps/Control Plane/docker-compose.yml` (~1075, `controlplane-prometheus` volumes) - mount `./observability/rules:/etc/prometheus/rules:ro`.
- Flag: none (inert until Prometheus loads it). Rollback: remove mount line. ZDR: none (metrics carry no org label).
- Tests: `docker run --rm --entrypoint promtool -v "$PWD/apps/Control Plane/observability:/o" prom/prometheus:v2.54.1 check rules /o/rules/audit-core.rules.yml` and `promtool test rules`.
- DoD: rules load in the running Prometheus (`/api/v1/rules` lists the group); each alert has a unit test.
- Evidence: rebuilt stack via `bash scripts/run-control-plane.sh up -d controlplane-prometheus`; publish one forged-subject event to the Control bus (authority mismatch) and capture `/api/v1/alerts` showing it pending/firing; capture `/api/v1/rules`.
- Effort S (1-2 d). Deps: none. Overlap: WS01 owns the Model-side terminal-row rule; if it chooses this Prometheus it adds `rules/model-plane-audit.rules.yml` and a scrape job (the mp_* metrics are not scraped here today). Risk: none.

### S1 [BUILD] ADR, canonical encoding library, audit-core CI
- Goal: fix the byte format of the chain once, test-vectored, before any DB code.
- Files:
  - NEW `apps/AUDIT_TAMPER_EVIDENCE_ADR_<date>.md` - context, decision (below), alternatives (hash-only, KMS, NATS fan-out), honest limits, release gates.
  - NEW `apps/Control Plane/audit-core/internal/chain/chain.go` + `chain_test.go` + `testdata/vectors.json`.
  - NEW `.github/workflows/audit-core-ci.yml` - Postgres 16 service, `go vet ./...`, `go test ./...` with `TEST_DATABASE_URL`, `promtool check rules` (S0), path filter `apps/Control Plane/audit-core/**`; model on `org-service.yml:1-125`, use `go-version-file: go.mod` (module says go 1.25.0; org-core CI pins 1.24).
- **Format v1** (the ADR text): `row_hash = SHA256("VAC1\x00" || prev_hash || F(org_id) || U64(chain_seq) || I64(occurred_at_micros) || I64(ingested_at_micros) || F(user_id) || F(actor_role) || F(plane) || F(event) || F(subject) || F(resource_id) || F(outcome) || F(event_id) || F(request_id) || F(host(ip_address)) || F(user_agent) || F(source_producer) || F(source_bus) || F(payload_hash) || F(details_canonical))`, `F(x)=uvarint(len)||bytes`, NULL encodes as empty. `details_canonical` = decode with `UseNumber`, re-encode sorted keys, no HTML escaping. Genesis `prev = SHA256("VAC1/genesis/"+org_id)` (per-org, blocks cross-org splice). `id`, `source_subject`, `source_stream_sequence` are not hashed (infrastructure, differ on replay). All hashed values are taken from the row **as stored** (read back from the insert), never from the in-memory event, so timestamp precision and JSONB normalisation cannot cause false breaks.
- Signed payloads (used in S3): attestation `"VAH1\n"+org+"\n"+seq+"\n"+hex(hash)+"\n"+signed_at`, checkpoint `"VAK1\n"+org+"\n"+through_seq+"\n"+hex(hash)+"\n"+pruned_total+"\n"+created_at`.
- Flag/migration/ZDR: none (pure library; no data). 
- Tests: `go test ./internal/chain/...`: golden vectors (unicode, floats `1.0`/`1e3`, empty optionals, IPv6, key-order permutations produce one hash), length-prefix ambiguity (`("ab","c")` vs `("a","bc")`), fuzz of canonicaliser idempotence.
- DoD: vectors file is the single source for S2, S4 and the offline CLI; CI workflow green on a PR.
- Evidence: CI run URL; sha256 of `vectors.json` recorded in the ADR.
- Effort S-M (2-3 d). Deps: D9 (non-blocking). Risk: canonicalisation edge cases (very large numbers: PG normalises `1e21`); mitigated by hashing the read-back value.

### S2 [BUILD] Serialised per-org chained insert
- Goal: every persisted audit row (stream, HTTP, direct) gets `chain_seq`, `prev_hash`, `row_hash` in one transaction under a per-org lock.
- Files:
  - NEW `internal/store/migrations/<next>_audit_chain.sql` (in `apps/Control Plane/audit-core/`): `ALTER TABLE audit_events ADD COLUMN chain_seq BIGINT, prev_hash BYTEA, row_hash BYTEA, chain_version SMALLINT, chain_mode TEXT CHECK (chain_mode IN ('insert','catchup'))` (nullable, metadata-only); `CREATE UNIQUE INDEX uq_audit_events_org_chain ON audit_events (org_id, chain_seq) WHERE chain_seq IS NOT NULL`; `CREATE TABLE audit_chain_heads (org_id TEXT PRIMARY KEY, head_seq BIGINT NOT NULL, head_hash BYTEA NOT NULL, activated_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), state TEXT NOT NULL DEFAULT 'ok')`; trigger `audit_events_guard` (BEFORE UPDATE OR DELETE): allows setting the chain columns once from NULL, blocks every other UPDATE and any DELETE unless `current_setting('audit.maintenance', true)='on'`. The trigger is a guard rail against application bugs, not a security boundary (the DB owner can drop it).
  - `internal/store/migrate.go` - add `//go:embed` var (12-28) and slice entry (36-43).
  - `internal/store/store.go:77-111` `insertAudit` - restructure: `Begin`; `INSERT INTO audit_chain_heads ... ON CONFLICT DO NOTHING`; `SELECT head_seq, head_hash ... FOR UPDATE`; **spot-check**: row `chain_seq=head_seq` has `row_hash = head_hash` (the "verifier in the insert path"; on mismatch set `state='broken'`, bump metric, log, **still append**: detect and alert, never drop evidence); `INSERT ... ON CONFLICT DO NOTHING RETURNING` all hashed columns; if no row, commit (nothing changed) and fall through to `resolveAuditConflict` unchanged (113-139); else compute hash (S1), `UPDATE audit_events SET chain_*`, `UPDATE audit_chain_heads`. Use a pgx batch to keep round trips low. `New(pool)` gains functional options (`WithChain`) so the call sites at `cmd/server/main.go:57`, `extra_nats.go`, tests keep compiling.
  - NEW `internal/store/chain.go` - `appendChained`, `CatchUp(ctx, orgID)` (chains rows with `chain_seq IS NULL AND ingested_at >= activated_at` in `(ingested_at,id)` order under the lock, `chain_mode='catchup'`; makes flag-off windows and rollbacks recoverable).
  - `internal/metrics/server.go` - `audit_core_chain_appends_total{result}`, `audit_core_chain_head_lock_wait_seconds`, `audit_core_chain_broken_total`, `audit_core_details_guard_total{result}` (D7 observe mode). No org label.
  - `cmd/server/main.go:202-277` - `AUDIT_CHAIN_ENABLED`; boot-time catch-up goroutine.
  - `apps/Control Plane/docker-compose.yml:~825` (audit-core env) - `AUDIT_CHAIN_ENABLED: ${AUDIT_CHAIN_ENABLED:-0}`; `audit-core/.env.example`; update compose-contract tests that read compose text (`cmd/server/main_test.go:49-134, 787`).
- Flag: `AUDIT_CHAIN_ENABLED`, code default 0, compose default 0, flip to 1 only after the evidence below. Rollback: set 0 and restart; inserts continue unchained (columns ignored), catch-up chains the gap on re-enable. Image rollback is safe (old code ignores the columns; the guard trigger only blocks UPDATE/DELETE, which old code does only in Purge: S3 must ship its maintenance-GUC change in the same image or Purge breaks; see S3).
- ZDR: no content stored or copied; only hashes of existing content-free columns. Golden: NEW `internal/testdata/export-content-free.golden.json` asserting the session-core wire fixtures (`internal/testdata/session-core-wire/`) hash and export with only `details` keys `tool, data_category, zdr, phase`. The shared posture matrix (`zdr-posture-matrix.json`) evaluates route decisions, not stores, so no matrix row is added unless its owner adds a "stores" section (not determined).
- Tests: NEW `internal/store/chain_insert_test.go`: sequential contiguity; 50 goroutines x 3 orgs yields gap-free `chain_seq` and valid linkage; replay of the same event does not advance the head; changed payload still `ErrAuditEventConflict`; same assertions via `InsertAuditFromStream` and `InsertAuditFromSource`; catch-up after flag off/on; guard trigger blocks UPDATE/DELETE without the GUC. Update `store_test.go` cleanup/backdating (`TestPurge_DeletesOnlyExpiredRows` backdates `ingested_at` by UPDATE) and add chain assertions to `session_core_replay_test.go:26`. `jetstream_inbox_contract_test.go` text checks stay.
- DoD: all tests green; with the flag on, 100% of new rows have a chain; `/readyz` unchanged.
- Evidence: rebuilt audit-core image through `bash scripts/run-control-plane.sh up -d --build audit-core`; flag on; run one governed tool action in the SPA (real session-core outbox to NATS to audit-core); `psql` shows contiguous `chain_seq`, `prev_hash` = previous `row_hash`, head matches last row; restart audit-core mid-stream and show no gap; capture lock-wait histogram.
- Effort L (5-7 d). Deps: S1. Risks: per-org lock serialises a hot org (audit volume is low: 4,992 rows in 4 weeks); mitigated by short tx and the histogram; compose-contract tests are text-sensitive.

### S3 [BUILD] Signed head attestations and chain-safe retention
- Goal: retention can delete old rows without breaking the chain or destroying tamper evidence; heads are signed with a key that is not in the database.
- Files:
  - NEW `internal/store/migrations/<next>_audit_chain_checkpoints.sql`: `audit_chain_checkpoints (id, org_id, through_seq, through_hash, pruned_rows_total, reason, key_id, signature, created_at, UNIQUE(org_id, through_seq))`; `audit_chain_attestations (org_id, chain_seq, row_hash, key_id, signature, signed_at, PK(org_id, chain_seq))`; `audit_chain_incidents (id, org_id, kind, chain_seq, detail, detected_at, resolved_at)`; embed in `migrate.go`.
  - NEW `internal/chain/sign.go` + test - Ed25519 signer and key ring (`AUDIT_CHAIN_KEY_ID`, `AUDIT_CHAIN_PRIVATE_KEY_BASE64`, `AUDIT_CHAIN_PUBLIC_KEY_BASE64`, optional `AUDIT_CHAIN_VERIFY_KEYS` JSON of retired public keys).
  - `internal/store/store.go:243-269` `Purge` - per org, in batches (5k rows), each batch: take `pg_try_advisory_xact_lock(hash('audit-purge',org))` (several replicas run the sweep, `main.go:157-187`); `X = min(chain_seq of rows with ingested_at >= cutoff) - 1` (contiguous prefix, because `ingested_at` is not commit-ordered); **verify the prefix being deleted** (linkage from the previous checkpoint); on failure write an `audit_chain_incidents` row, delete nothing, count `OrgsBlocked`; on success `SET LOCAL audit.maintenance='on'`, delete `chain_seq <= X`, insert signed checkpoint. Rows with `chain_seq IS NULL` are purged by `ingested_at` as today. Signature `Purge(ctx, days) (PurgeResult, error)` kept; `PurgeResult` gains `CheckpointsWritten`, `OrgsBlocked`. Retention never destroys evidence of tampering: a tampered org stops purging and alerts.
  - NEW `internal/store/attest.go` - signs the head of every org that advanced since its last attestation.
  - `cmd/server/main.go` - key loading and `runAttestor` (`AUDIT_CHAIN_ATTEST_INTERVAL`, default 60s); **fail boot** when `AUDIT_CHAIN_ENABLED=1`, `ENVIRONMENT=production` and no key (same posture as `ALLOW_INSECURE_DEV_DEFAULTS` in `docker-compose.production.yml:199-206`).
  - `apps/Control Plane/scripts/run-control-plane.sh` (mirror the Ed25519 block 171-205; add names to the list at 421-456), `scripts/validate-release-credentials.sh:19-42`, compose env, `audit-core/.env.example`. Never write keys into `apps/Control Plane/.env` by hand (secrets-ledger rule: generated values live in the persisted store).
- Flag: same `AUDIT_CHAIN_ENABLED` plus `AUDIT_CHAIN_PRUNE_ENABLED` default 0 (until 1, Purge keeps the legacy behaviour for unchained rows and skips chained rows, so enabling chain never causes data loss). Rollback: set prune off; checkpoints remain valid.
- ZDR: signatures and hashes only.
- Tests: NEW `internal/store/purge_chain_test.go`: insert 20, backdate 10 (GUC), purge writes a checkpoint, verify (S4) still `verified`; tampered prefix is not deleted and an incident row exists; concurrent insert while purging; two purgers racing; all rows expired leaves the head usable; NEW `internal/chain/sign_test.go` (tampered checkpoint, wrong key id, rotation). Adjust `TestPurge_*` in `store_test.go:21, 74`.
- DoD: a purge on the dev stack leaves `verify` green; boot fails without a key in production mode.
- Evidence: seed backdated rows via psql with the GUC, run the sweep (restart audit-core), capture checkpoint rows and verifier output before and after; capture the public key endpoint (S5) matching `AUDIT_CHAIN_PUBLIC_KEY_BASE64`.
- Effort L (5-7 d). Deps: S2, D1. Risks: purge logic is the most dangerous code in the stream (deletes evidence); mitigated by verify-before-delete, batch txs, the incident stop, and a dry-run mode (`AUDIT_CHAIN_PRUNE_DRYRUN=1` logs the plan only).

### S4 [BUILD] Verifier, offline CLI, chain alerts
- Goal: anyone can prove or disprove integrity, in the service on a schedule, on demand, and offline.
- Files:
  - NEW `internal/store/verify.go` - `VerifyOrg(ctx, org, opts{FromSeq, MaxRows})`: loads latest signed checkpoint, checks signature, first row `prev_hash == checkpoint.through_hash`, contiguous `chain_seq`, recomputes every `row_hash`, final row == `audit_chain_heads`, head `>=` latest attested seq (**truncation**), attestation signature. Report `status` in `verified | verified_with_legacy | broken | partial | key_unavailable`, `first_broken_seq`, `reason`, `legacy_unchained_rows`, `checkpoint`, `head`, `attested`.
  - NEW migration `<next>_audit_chain_verification_state.sql` (`audit_chain_verifications`: `verified_through_seq`, `verified_through_hash`, `last_full_at`, `last_status`).
  - NEW `internal/verifier/verifier.go` - daily incremental from `verified_through_seq`, weekly full; `cmd/server/main.go` wiring (`AUDIT_CHAIN_VERIFY_INTERVAL`).
  - NEW `cmd/audit-verify/main.go` + test - offline: input export pages + manifest (S5) + public key file, exit 0 verified / 2 broken / 3 unverifiable; uses `internal/chain` only (no DB). `Dockerfile:34-49` builds and copies it.
  - `apps/Control Plane/observability/rules/audit-core.rules.yml` - `increase(audit_core_chain_verify_failures_total[1h])>0`, `audit_core_chain_broken_total`, verify stale > 36h.
  - `internal/metrics/server.go` - `audit_core_chain_verify_failures_total`, `audit_core_chain_last_verify_timestamp_seconds`.
- Flag: `AUDIT_CHAIN_VERIFY_ENABLED` default 1 when chain is on. ZDR: none.
- Tests: NEW `internal/store/verify_test.go` table of attacks, each must be detected with the right `first_broken_seq`: edit a field, edit `details`, delete a middle row, delete the tail, swap two `chain_seq`, forge a head, replay an old attestation, drop a checkpoint, forge a checkpoint with a wrong key, rewrite the whole chain without the key (detected via attestation/checkpoint signature). `cmd/audit-verify/main_test.go` against `chain/testdata/vectors.json`.
- DoD: all attacks detected; verifier is O(rows) with bounded memory (stream rows; 128M limit).
- Evidence: on the dev stack mutate one row with psql (GUC), capture verify `broken` with seq, the metric increment and the firing alert; restore from a pre-saved copy and show `verified`; run `audit-verify` on exported pages.
- Effort M (4-5 d). Deps: S3. Risk: verify cost on large orgs (bounded by `MaxRows` + resumable cursor).

### S5 [BUILD] Compliance read API in audit-core
- Goal: scoped, admin-only, cursor-paged read with time-range export, signed manifest, verify and key endpoints, each read itself audited.
- Files:
  - `internal/api/service_auth.go` - new scope `audit:compliance:read:self` (accept at 108-115 only for principal `verevon-gateway`), `requiredAuditScope` (203-212) maps `/v1/compliance/audit/*`, `ValidateRequiredServiceCredentialRegistry` (140-158) requires it on the gateway. Same HMAC delegation path (org binding) applies.
  - NEW `internal/api/roles.go` - `requireComplianceRole(serviceAuthorization)`: `UserRole` in `{owner, admin}` else 403 `{error:{code:"compliance_role_required"}}` (F-t1-031 imports this helper rather than writing its own).
  - NEW `internal/api/compliance.go`; `api.go:109-116` mounts `r.Route("/compliance/audit", ...)`; `eventStore` interface (30-36) gains the new methods. Routes: `GET /events` (filters `event, user_id, plane, outcome, since, until`, `limit` default 200 max 1000, opaque keyset `cursor` on `chain_seq`; envelope `{data, meta:{count,next_cursor,limit,head_seq}, error:null}`), `GET /manifest?since&until` (signed: `first_seq,last_seq,count,first_prev_hash,last_row_hash`, checkpoint, latest attestation), `POST /verify` (S4), `GET /keys` (public keys), `GET /export-status` (S8 cursors; empty until then).
  - NEW `internal/store/compliance.go` - range start resolved with `(org_id, ingested_at)` index (`idx_audit_events_org_ingested`), scan by `(org_id, chain_seq)`; rows carry every preimage field (ip, user-agent, `source_*`, `payload_hash`, hashes in hex) so the offline verifier needs nothing else. Time basis `ingested_at`, documented.
  - Self-audit: every call writes `compliance_audit_read` / `compliance_audit_verify` via the normal insert path (producer `audit-core`, plane `control`, `event_id` from request id; details = route, filters, count, cursor seq; no row content).
- Flag: none at API level; reachable only if the compose credential block grants the scope (deploy order: audit-core image first, since an old image rejects the unknown scope and would not boot; compose and image roll together). ZDR: rows content-free by producers + D7 guard metric; export golden from S2.
- Tests: NEW `internal/api/compliance_test.go` (pattern `api_test.go:132`): member and viewer 403, owner/admin 200, org mismatch 403, delegation replay 403, limit cap, cursor stable under concurrent inserts, manifest signature verifies with `/keys`, self-audit row exists; `service_auth_test.go` additions for the new scope and its principal restriction.
- DoD: an owner can page the full org log for any time range and the pages verify offline.
- Evidence: curl through the gateway (S6) as owner/admin/member; capture 200/403, manifest, `audit-verify` over saved pages.
- Effort M (4-5 d). Deps: S2 (S3/S4 for manifest signature, verify). Hot spot with F-t1-031 on `api.go`, `service_auth.go`: see section 5.

### S6 [BUILD] Gateway compliance routes (+ UI per D6)
- Files:
  - NEW `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit_compliance.rs` (not `audit.rs`, to keep Space's edits separate); register in `domains/mod.rs` and `main.rs:99`. Routes `GET /api/v1/audit/compliance/events`, `/manifest`, `/keys`, `/export-status`, `POST /verify`, plus proxies for SIEM destinations to org-core (`GET|POST /api/v1/audit/siem-destinations`, `PATCH|DELETE /:id`) via `proxy_json` (audience mapping `upstream.rs:955-965`). Gate: `has_authorized_org_role(&user, &["owner","admin"])` (`middleware.rs:112`), org from `authorized_org_id` (`upstream.rs:324`), client `org_id` never forwarded, query allow-list, `rate_limit_middleware` (pattern `domains/mcp.rs:55-60`), 403 envelope with a typed code.
  - NEW `src/shared/api/audit-compliance-client.ts` + `.test.ts`; action descriptors in `src/shared/actions/action-registry.ts` for verify, export, destination create/update/delete, **human actors only** (an agent must never read or administer audit); UI `src/features/settings/components/AuditComplianceSection.tsx` only if Space has not delivered the view (D6).
- Flag: none (admin-gated, audit-core scope-gated); rollback = previous gateway image. ZDR: pass-through of content-free rows.
- Tests: `#[cfg(test)]` in the new file with wiremock (pattern `main.rs:2241-2307`): member gets 403 and the mock receives **zero** requests; admin request carries `x-user-role: owner` and v3 delegation; forged `org_id` ignored. Commands (from `apps/Frontend Plane/verevonv3`): `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `pnpm vitest run src/shared/api/audit-compliance-client.test.ts`; `pnpm typecheck` and `pnpm lint` compared against the HEAD failing set (known baseline failures).
- DoD: owner/admin can verify and export from the SPA or API; member cannot.
- Evidence: rebuild gateway image per the Frontend compose, run as owner and as member, capture both responses and the audit-core self-audit rows.
- Effort S-M (3 d, +2 d UI). Deps: S5, D6. Risk: Space overlap on admin view.

### S7 [BUILD] org-core SIEM destination config
- Files (all `apps/Control Plane/org-core/`):
  - NEW `migrations/<next>_siem_destinations.up.sql` + `.down.sql`: `siem_destinations (id, org_id, kind CHECK in ('ndjson_webhook','splunk_hec'), url, secret_ciphertext, secret_key_id, enabled, event_filter JSONB, revision, created_by, created_at, updated_at)`, max 3 per org, `GRANT ... TO org_core_app`, RLS on `app.current_org` (pattern `019_interactive_retention_outbox.up.sql`).
  - NEW `internal/org/siem_destinations.go`, `internal/org/siem_seal.go` (AES-256-GCM, `ORG_CORE_SIEM_SEAL_KEY`, key id), tests. URL rules: https only, no userinfo, no IP literal in private ranges (static check; the dial-time guard is in S8).
  - NEW `internal/http/siem_handlers.go`; `internal/http/server.go:99-179`: `/orgs/:id/siem-destinations` CRUD behind `guard` + owner/admin check (reuse `authorizeOrgErasure` shape, `gdpr_handlers.go:75`); secrets write-only (`secret_set:true`), ciphertext never serialised; internal `GET /internal/orgs/siem-destinations` (exporter, enabled only, `?updated_since=`), registered like `by-tenant` (`server.go:170`).
  - `internal/http/service_auth.go`: allowlist (27-45) gains `org:siem:manage:self` (gateway only) and `org:siem:read:any` (not gateway); scope maps (307-375); `ValidateRequiredServiceCredentialRegistry` (184-240) requires an `audit-core` principal holding `org:siem:read:any`.
  - `internal/org/types.go:276` area + `service_enhanced.go:168-176` - computed `feature.siem_export` (plan-gated, D3).
  - Durable audit of every create/update/delete (`org_siem_destination_changed`; details: id, kind, host only, action; no URL query, no secret) through an org-core audit outbox (follow `017_gdpr_audit_outbox`; whether that outbox is generic is not determined: verify, else add a sibling table).
  - `apps/Control Plane/docker-compose.yml:539` `ORG_CORE_SERVICE_CREDENTIALS` (add `audit-core` principal with `${AUDIT_ORG_CORE_SERVICE_TOKEN:?...}`; gateway gains `org:siem:manage:self`), org-core env `ORG_CORE_SIEM_SEAL_KEY`, `scripts/run-control-plane.sh` (421-456 + persist), `scripts/validate-release-credentials.sh`, `scripts/run-isolated-control-container-e2e.sh:217`, `org-core` `.env`-contract. org-core must ship before or with the compose block (an image that predates a scope refuses the whole registry; same note as `docker-compose.yml:~528-538`).
- Flag: `ORG_SIEM_DESTINATIONS_ENABLED` default 0 (routes 404). ZDR: config holds no content; destination URL and key are the customer's.
- Tests (`cd "apps/Control Plane/org-core" && go test -v -p 1 ./...`, needs `CONTROL_LIFECYCLE_TEST_DATABASE_URL` as in `org-service.yml:74-110`): seal round trip and wrong-key; RLS cross-tenant negative test; member cannot create (403), admin can; max-3; secret never in any response or log; scope tests; NEW `internal/http/siem_destinations_contract_test.go` replaying a vendored golden `testdata/audit-core-org-core-siem.contract.json` (+ `.sha256`) through the real middleware and the real compose block, exactly like `retention_posture_contract_test.go`, and the audit-core client test (S8) replays the same golden. This is the guard against the failure that file documents (a mocked client that enforced no auth).
- DoD: owner creates a destination via gateway; the exporter principal can list it; a gateway or member principal cannot read `:any`.
- Evidence: rebuilt org-core, curl as owner/member, `psql` shows ciphertext only.
- Effort M (5-6 d). Deps: D3, D5. Independent of the chain, parallel with S2-S4.

### S8 [BUILD] SIEM exporter service
- Goal: ordered, at-least-once, resumable per-org delivery of chained rows, plus periodic signed heads, to destinations configured in org-core.
- Files (`apps/Control Plane/audit-core/` unless stated):
  - NEW `cmd/siem-exporter/main.go`; NEW `internal/siem/{runner,orgcore_client,egress,format_ndjson,format_hec,cursor,backoff}.go` + tests. Runner: poll org-core list every 30s; per destination `SELECT ... WHERE org_id=$1 AND chain_seq > cursor ORDER BY chain_seq LIMIT 500` (<= ~900 KiB), POST, advance cursor on 2xx only; backoff 5s to 15min; one in-flight batch per destination. Each event carries `org_id, chain_seq, prev_hash, row_hash` (dedup key `(org_id, chain_seq)`); after each batch an `audit_chain_head` record with the newest attestation, so the SIEM becomes an independent witness. `ndjson_webhook` adds `X-Verevon-Signature: sha256=HMAC(secret, timestamp+"."+body)` and `X-Verevon-Timestamp`; `splunk_hec` uses `Authorization: Splunk <token>`.
  - **Egress guard** `egress.go`: https only, DNS resolved then validated **at dial time** (`net.Dialer.Control`) against loopback, RFC1918, link-local incl. 169.254.169.254, CGNAT, ULA, IPv4-mapped IPv6; no redirects; 10s timeout; 1 MiB cap. Dev-only `AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS=1`, refused when `ENVIRONMENT=production`.
  - Gap honesty: if the cursor is older than the oldest retained `chain_seq`, send `{"type":"audit_export_gap","from_seq":..,"to_seq":..}` and advance.
  - NEW migration `<next>_audit_export_cursors.sql` (`audit_export_cursors`: `destination_id, org_id, last_seq, last_success_at, consecutive_failures, state, last_error_class`); exporter does not migrate, it waits for the server's migration.
  - `internal/api/compliance.go` `GET /export-status` reads cursors (lag, state, last error class; no URL, no secret).
  - `Dockerfile:34-49` build + copy; NEW compose service `audit-siem-exporter` in `apps/Control Plane/docker-compose.yml` modelled on `audit-nats-provisioner` (767-781): same build, `entrypoint: ["/usr/local/bin/siem-exporter"]`, profile `siem`, `controlplane-net` only (no `inter-plane-bus`), 64M; `docker-compose.production.yml` build args like 199-206; `observability/prometheus.yml` scrape job; metrics `audit_siem_export_lag_seconds{destination_kind}`, `audit_siem_export_batches_total{result}`, `audit_siem_destinations{state}`; rules appended: lag > 15 min, failure ratio.
- Flag: service absent unless compose profile `siem` is enabled; per-destination `enabled`; `AUDIT_SIEM_EXPORT_ENABLED` default 0 (binary idles). Rollback: stop the service; cursors persist.
- ZDR: exports rows as stored (content-free by producers; D7 guard); logs never contain URL query, headers or body. Secrets only in memory.
- Tests: `go test ./internal/siem/...`: egress table (all blocked ranges, DNS rebinding via a resolver that flips to a private address, redirect to private, http refused); at-least-once (receiver fails twice then ok: batch redelivered, cursor advances once); ordering under concurrent inserts; gap marker; HMAC verifies; log-capture test proves no secret/URL leaks; org-core client replays the S7 golden; `cmd/siem-exporter` main test for production refusal of the dev flag.
- DoD: destinations drain in order; lag alert works; backlog after outage delivered with no loss.
- Evidence: run a local receiver on the host (dev flag on), configure a destination via the gateway as owner, perform governed actions, capture the receiver's ordered NDJSON, signature check and `audit-verify` over the received rows plus head record; stop the receiver, show lag growth and the pending alert; restart, show drain and dedup by `(org_id, chain_seq)`.
- Effort L (7-9 d). Deps: S2 (chain_seq), S7 (config), S3 (attestations, optional first cut), D4. Risks: SSRF (mitigated above), memory limit, secret handling.

### S9 [FINISH] Egress declaration, evidence, runbook
- Files: `docs/compliance/evidence/tools/scan-egress.mjs` (VENDORS entry "customer-configured SIEM destination", class customer-directed, gating variable `AUDIT_SIEM_EXPORT_ENABLED`; run `node docs/compliance/evidence/tools/scan-egress.mjs --strict` and refresh the summary baseline), `docs/compliance/evidence/subprocessors-from-code.md`, `apps/Model Plane/docs/ZDR.md:95` (extend the "NATS / audit envelopes" row: chained rows and exports content-free), `docs/compliance/evidence/honest-limits.md` (DB-owner attacker detected only via signature + external witness; completeness not proven by the chain; legacy rows unchained; SIEM witness only when configured), `docs/compliance/evidence/trust-claims-pending-reverification.md` section 6 (new item: tamper-evidence, claim stays narrowed until evidence), NEW `apps/Control Plane/docs/AUDIT_CHAIN_RUNBOOK.md` (verify, incident, key rotation, purge dry-run, rollback), `apps/Model Plane/MODEL_PLANE_STATUS.md:448` and `MODEL_PLANE_ROADMAP.md:417` status lines. docs/compliance files `claims-vs-code.md`, `data-flow-map.md`, `honest-limits.md` are modified in the live tree: edit last and rebase on whoever lands that work.
- Tests: `node docs/compliance/evidence/tools/scan-egress.test.mjs`; the trust drift test (needs git and node).
- DoD / evidence: claim wording only broadens to what a fresh read on the claimed stack shows; evidence cites the S2/S4/S8 captures. Effort S (2-3 d). Deps: S8.

## 5. Interfaces

PROVIDES
- **All workstreams that add audit events (WS13 approvals decisions, WS14, WS25 policy changes, WS10 entitlement denials, WS05 tool class denials)**: an append-only, per-org chained sink. Contract: publish a flat `verevon.audit.v2.<plane>.<producer>.<event>` body with stable `event_id` and content-free `details`; no registration step. Rows are chained automatically; `details` size/key guard is observe-only (D7) then enforced.
- **WS01**: Control-side rules mechanism (`observability/rules/*.yml`, S0); chain activation evidence for the trust claim.
- **F-t3-062 (WS later) and F-t1-051 (Space)**: `Purge` is the only delete path and is chain-safe; per-org cutoff and legal hold plug into S3 (`retentionFor(org)` seam) without a new delete path.
- **Space session**: `requireComplianceRole` helper, compliance client and action descriptors; `GET /v1/audit/compliance/events` as the data source for its admin view.
- **Frontend**: `AuditComplianceSection` contract (if built).

CONSUMES
- WS01: publisher deployed, requeue migration, 4,992 rows replayed, Model-side alert rule location.
- Control: org-core (destinations, entitlement), run-control-plane.sh secrets, auth/gateway delegation (membership role).
- Model Plane: nothing new (existing outbox/NATS). Quarry/Ingestion: any new audit producer uses integration-corev2's `audit:write` principal (`service_auth.go:146-155`), unchanged.

Conflict hot-spots (rule: first to land owns the shape; the other rebases)
| File | Shared with | Rule |
|---|---|---|
| `audit-core/internal/api/api.go`, `service_auth.go`, `store/store.go` | F-t1-031 (Space role-gate on `GET /v1/audit`) | Space's S-size gate lands first; WS06 S5 adds `roles.go` and new files, edits `api.go`/`service_auth.go` minimally |
| `apps/Control Plane/docker-compose.yml` (audit-core env 825-829, org-core credentials 539, Prometheus 1075) | any Control change | one compose PR per slice, rebuild per script, update compose-contract tests |
| `scripts/run-control-plane.sh` credential lists (421-456) | all new tokens | append alphabetically; never hand-edit `.env` |
| `observability/rules/` | WS01 | S0 creates directory + `rule_files`; each owner has its own file |
| gateway `domains/audit.rs` | Space | WS06 uses new `audit_compliance.rs` |
| `docs/compliance/evidence/*` (live-modified) | WS01, others | S9 last |

## 6. Cross-session interfaces

- **(a) Verevon Space (F-t1-031, F-t1-051)**: overlapping words ("SIEM export, usage ledger export", "audited export token"). Proposed split: Space owns the access fix on the existing list, the admin audit view, usage-ledger export and Space/agent attribution fields; WS06 owns chain, compliance/export API, SIEM. Space's `space_ref`/`agent_ref` details ride inside `details` (content-free; D7 guard). Space's retention TTL (`space_effect_policies`) must call the chain-safe purge, not a new DELETE. Assume Space lands the gate first.
- **(b) Quarry/browser (F-t2-036 audit decision record)**: Quarry events enter through integration-corev2's HTTP `POST /v1/audit` (`audit:write`, plane pinned to `ingestion`) or a future bus producer; both reach `insertAudit` and are chained with no WS06 change. Assume Quarry emits content-free details (no page text, no selectors with content).

## 7. Regression guards

| Audit section 8 strength | How WS06 touches it | Guard |
|---|---|---|
| ZDR by construction | adds hashes/exports of already content-free rows; no new content store | `internal/testdata/export-content-free.golden.json` (S2); D7 guard metric; `ZDR.md` row (S9); exporter log-leak test (S8); `go test ./internal/store/... ./internal/siem/...` |
| Honest outcomes | verifier reports `verified_with_legacy` / `partial` / `key_unavailable`, never a bare "ok"; export gap markers; completeness not claimed | verify attack table (S4); gap test (S8); honest-limits text (S9) |
| Credential isolation | destination secrets sealed, write-only, never logged or returned; AES key and signing key only in persisted secrets store | S7 tests (no secret in response/log), S8 log capture, `validate-release-credentials.sh` |
| Multi-tenant server-side runtime / lease-based outboxes | publisher and outbox untouched; per-org isolation in chain, cursors, RLS | `audit_publisher.rs:1038` fixture test and `session_core_wire_fixtures_test.go` stay green; S2 multi-org concurrency test; S7 RLS negative test |
| Signed fail-closed capability gate / two loops | untouched; audit events are an output, not a tool path | no change to execution-core or model-gateway (check `git diff --stat` shows none) |

Existing guards that must stay green: `TestReplayedSessionCoreAuditEventIsStoredExactlyOnce` (`session_core_replay_test.go:26`), `TestLogicalAuditIdentityIsBoundToProducerAndRejectsConflictingReuse` (`store_test.go:243`), `TestAuthorizeAuditReadRejectsTamperingExpiryAndReplay` (`service_auth_test.go:91`), `jetstream_inbox_contract_test.go`, nats-provisioner `manifest_contract_test.go` (unchanged by design).

## 8. Effort and risks

| Slice | Effort | Days |
|---|---|---|
| S0 | S | 1-2 |
| S1 | S-M | 2-3 |
| S2 | L | 5-7 |
| S3 | L | 5-7 |
| S4 | M | 4-5 |
| S5 | M | 4-5 |
| S6 | S-M | 3 (+2 UI) |
| S7 | M | 5-6 |
| S8 | L | 7-9 |
| S9 | S | 2-3 |

Total about 38-52 engineer-days (upper L, near XL; the audit's L holds only for the chain + API, SIEM is the other half). Critical path: S1 -> S2 -> S3 -> S4 -> S5 -> S6 (about 24-32 d); S7 -> S8 runs in parallel and joins at S2 (about 12-15 d). S0 and S7 can start immediately.

Top risks
1. **Purge destroying evidence or breaking the chain** (S3). Mitigation: verify-before-delete, incident stop, dry-run flag, batch txs, checkpoint signed per batch, tests for racing purgers.
2. **False sense of security**: a DB-owner attacker can rewrite the chain. Mitigation: signed attestations with a key outside the DB, SIEM/customer export as external witness, honest-limits text, no claim broadening before evidence (S9).
3. **Deploy-order lockouts**: an image that predates a scope refuses the whole credential registry (audit-core at S5, org-core at S7). Mitigation: image and compose roll together via `run-control-plane.sh`, contract tests read the real compose block, never bare `docker compose build`.
4. **SSRF / secret leakage via SIEM egress** (S8). Mitigation: dial-time guard, no redirects, separate process on `controlplane-net`, sealed secrets, log-leak test, production refusal of the dev flag.
5. **Ownership collision with Space (F-t1-031)** on `api.go`/`service_auth.go`/admin view and on retention. Mitigation: order in section 5, new files for WS06 code, D6 ruling.
