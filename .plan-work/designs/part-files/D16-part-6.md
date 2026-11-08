
---

## 5. Dependencies and ordering, merge-conflict hotspots

### 5.1 Dependencies

| Slice | Hard dependency | Soft dependency |
|---|---|---|
| P0 | none | D4 S-8 (`zdr_refusal.rs` helper and shared flag), D7 (`browser.profile.persistent` capability check on the same handlers) |
| S0 | D3 S1 (live Chromium lane and stub-egress helper) | none |
| P1 | none | D10 S1b (request-id codemod in `profile_routes.rs`) |
| E1 | P1 (`SaveMeta`, tombstones, `snapshot` nullable) | external review of the crypto module before `REQUIRED=1` |
| P2 | P1 (`owner_user_id`) | D2 (split of `agent_routes.rs`), D1 (delegated bearer: a Model-driven run must carry the user identity for the owner check to mean anything) |
| C1 | S0 | D3 S3 (deadlines table; `Op::ProfileCapture` 5 s -> 10 s) |
| C2 | C1, **D3 S2** (per-page emulation and `open_tab_page` restructure; audit 9.4 item 9) | D8 (attach to a shared session context: restore must stay first-tab-of-context only) |
| E2 | E1; Control Plane owner review; ownership-doc ratification (D16-3) | D7 (org-core migrations 021-023 claimed; E2 takes 024 or the next free) |
| E3 | E1 (and E2 for shredding); a backfill window | D4 (touches `QC/internal/store/gdpr_purge.go` and the erasure path for run tombstones) |
| S7 | P2 | none |

Wave dependencies from the audit hold: F-t2-007 part A (D3 S2) precedes the restore hooks; F-t2-035 (durable hosted sessions) and F-t2-058 (request context, profile pinning for scrape) depend on this item and should consume `SaveMeta`, the owner guard and the restore report instead of inventing parallel ones.

Recommended global order: P0, S0 (parallel), P1, E1, C1 (parallel with E1), P2, C2 (after D3 S2), E2, E3, S7 only on demand. Two engineers can run Track A and Track B concurrently; elapsed time is roughly 6 weeks for two engineers (46 engineer-days plus review and Control Plane coordination).

### 5.2 Merge-conflict hotspots (files and ranges shared with other workstreams)

| File | Range | Shared with |
|---|---|---|
| `QB/chromiumoxide.rs` [C] | `open_tab_page` 371-557 (restore block 507-545), `persist_current_page` 675-697, `capture_snapshot` 1771-1849, `release` 1944-1986, `SessionPages` 97-110 | D3 S2, S3, S5, S10, S6 (serial order S2, S3, S5, S10, S6, then F-t2-020/021/023/034), D8 attach (8.4), D12/D13 hunks near 3044; new modules (`profile_capture.rs`, `profile_restore.rs`) keep this file's diff small |
| `QE/profile_routes.rs` [C] | all handlers (102-453) | D4 S-8, D7 S3/S4, D10 S1b (mechanical `RequestKind::new()` replacement at 7 sites), P0/P2/C1. Merge order: D10 S1b, P0, D4 S-8, D7, P2 |
| `QE/agent_routes.rs` [C] | profile id and ZDR block 1415-1476, lease 1487-1508, `zdr_forbids_persistent_profile` 527-533 | D2 (file split into `agent_routes/*`), D1 (grant at start), D4 S-8 (`details.zdr_refusal`), D7 (policy gate at start) |
| `QE/main.rs` [M] | store wiring 551-600, durability gate 694-708, agent driver 1284-1288 | D3, D2, D6, uncommitted hunks elsewhere in the file |
| `QE/config.rs` [M] | appended fields; profile fields 243/250/21 | D3, D4, D6, D7 (silent-fallback deserializer, S9: assert security flags at boot) |
| `QR/lib.rs` [M] | exports near 166 | many |
| `QR/postgres_profile_store.rs`, `QR/cached_profile_store.rs` [C] | whole files | D16 only (P1, E1 rewrite them) |
| `QR/migrations/` | 0010 (D2), 0011 (D6 and D8 collide), 0012-0013 (D12); D16 takes 0014-0016 provisional | all quarry-runtime schema changes; sqlx needs unique versions, applied files immutable |
| `OC/migrations/` | 021-023 claimed by D7; D16 takes 024 provisional | D7 |
| `OC/internal/http/server.go`, `service_auth.go` [C] | server.go 158-181; service_auth.go 25-50, 348-375 | D7 (policy routes and scopes) |
| `apps/Control Plane/docker-compose.yml` [C] | org-core env and `ORG_CORE_SERVICE_CREDENTIALS` at 539 | D7, any new org-core principal |
| `apps/Ingestion Plane/docker-compose.yml` [M] | quarry-edge env block 53-114 (uncommitted hunks ~121-160) | D3 (sandbox default at ~110), D4, D6 |
| `QC/internal/gdpr/*`, `QC/cmd/control/main.go` [C] | 203-230 | D4 (tombstone purge) |
| `.github/workflows/quarry-v2-ci.yml` [C] | after 211 | D3 S1 (live lane job) |
| `docs/RUNBOOKS.md`, `docs/openapi.yaml` [M]; `docs/CONTRACTS.md` [C] | appended sections | D3, D4, D5, D12 |
| `QB/session.rs` [C] | `Cookie`, `SessionSnapshot`, `ProfileStore` | D8 (session context), F-t2-058 |

## 6. Plane ownership, ZDR propagation, Model-proposes/Quarry-executes check

- **Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, entitlements.** Profile data stays in the Ingestion Plane; the only Control Plane addition is key management, a new responsibility that needs the ownership docs updated and user ratification (D16-3, Q1). Quarry never reads a Control Plane table: it calls org-core's HTTP API with a registered credential. Audit events for key operations are Control Plane's. No direct database crossing: quarry-control reaches `quarry_edge` data only through an edge internal route (D16-5), not through SQL.
- **Data Plane v2 owns documents and chunks.** Not involved. Profile snapshots are never ingested; cookie and storage values never flow to the Data Plane.
- **Ingestion Plane captures evidence and persists durable knowledge through Data Plane contracts only.** A browser profile is not knowledge; it is Quarry-owned operational state under Quarry's own database. Compliant.
- **Model Plane owns reasoning, sessions/runs, capabilities, browser grants, cost.** The Model Plane never sees cookie or storage values: service principals are refused on all `/v1/profiles*` routes, the read route returns structure only, snapshots do not appear in observations, receipts or events, and the key API is called only by quarry-edge. The residual path is `AgentAction::Evaluate` reading `document.cookie`; F-t2-002 (W1) gates it and P0 adds the canary test that a seeded value never appears in any `/v1/*` response or log.
- **Application Plane / Frontend Plane.** No Application Plane change. The gateway stays a normalizing BFF: it forwards the user's token, gains no profile logic beyond a list-shape fix; owner enforcement is inside Quarry, so a Space or the SPA cannot widen an ACL by header or body.
- **Model proposes, Quarry executes or rejects.** A Model-driven run may propose a `profile_id`; Quarry's attach check (P2) rejects another user's profile inside Quarry, and Quarry alone decides what is captured and restored. Restore applies stored state to the session's own isolated context only, never navigates to origins on its own and never widens the egress policy (cookies and seeding do not trigger network requests).
- **ZDR propagation, per boundary.** Run start: refused (existing, `agent_routes.rs` 1470-1476). `/v1/profiles*`: refused for ZDR callers except list and delete (P0, staged, shared with D4 S-8, ADR topic 4). Redis cache: holds ciphertext only, v2 prefix, invalidate-on-write. Postgres: sealed rows; ZDR callers cannot create rows, so no ZDR plaintext can exist. Tombstones: ids and a timestamp only, deleted with the org, cannot be created for ZDR. Key API: carries org id, purpose and profile id, never content; DEK material never in logs or error details. Logs and events: counts only (restore report, capture report, `would_deny` counters); no cookie names, domains or origins at INFO. Spans: no new spans; D10's sampler rules apply. Rules at risk: (1) ZDR on `/v1/profiles*` is observe-only until the flag is on; (2) the interim `env` provider cannot crypto-shred; (3) other `quarry_edge` tables are not erased on org erasure (named in E3, owned elsewhere).

## 7. Cross-session interfaces

**(a) Verevon Space.**
- Shared piece: profile ownership and sharing. Proposed owner: Quarry (`quarry_profiles.owner_user_id` plus `scope`), enforced in `profile_guard`. A Space must never widen a profile's owner ACL: a Space-shared profile, if wanted, is an explicit ACL row (`principal_kind`, `principal_id`) written by the owner through a gateway action, read by Quarry; Space services never pass a `space_id` that Quarry trusts.
- Shared piece: agent browser inside a Space (D8 shared jar attach) uses the session's context, not the profile; restore stays first-tab-of-context only and saves happen at release by the run owner.
- ZDR-safe telemetry: the restore and capture reports are counts only, safe to project into Space activity feeds; origins and cookie names are not.
- User must reconcile: whether Space members may use an `org_shared` profile today (yes by D16-7) or need Space-scoped sharing first (a new scope kind).

**(b) Model Plane and chat/agents.**
- Shared piece: the identity at run start. The owner check needs the user's identity: D1's delegated bearer must carry `user_id` to Quarry for Model-driven runs; an autonomous service run without a user owns its profile as `service:<id>` and should use `run_scoped`.
- BrowserBroker grants (F-t2-001) are unaffected: profiles are not grant material. execution-core and model-gateway must not call `/v1/profiles*` (now refused) and must not forward `profile_id` they did not receive from the user's request.
- Shared piece: ZDR vocabulary. D16 uses D4's refusal codes; new feature key `browser_profile` is a D4 S-9 follow-up in the Model Plane catalogue (owner: that initiative).
- User must reconcile: who adds the `profile_restore` summary to the chat browser UI (D9) and whether the planner should be told "profile restored (N cookies)" (content-free, safe).

**Frontend gateway** (not a sibling session but touched): fix `ingestions/profiles.rs` list shape (P0); `browser.rs` profile proxies need no change.

## 8. What can proceed without an ADR or decision, and what is blocked

| Proceed now (no decision) | Notes |
|---|---|
| P0 (service-principal refusal, redaction, import scope, ZDR observe mode) | Redaction answers Q13 with the recommended default; the ZDR enforcement step needs ADR topic 4 but ships in observe mode |
| S0, C1, C2 (Track B) | Independent of every decision; C2 waits only on D3 S2 |
| P1 (ordering and tombstones) | |
| E1 with the `env` provider, flag `off`/`compat` | `on` in production only as the documented interim |
| E3 backfill tooling | after E1 |

| Blocked | By |
|---|---|
| P2 `enforce` (not `observe`) | Q2: legacy ownerless rows policy |
| E2 (org-core key service) | Q1: key service ownership in Control Plane, ratified; Q3: master-key custody and escrow owner |
| Default-on ZDR enforcement on `/v1/profiles*` | ADR topic 4 (D4 S-8) |
| `REQUIRED=1` in production | external review of the crypto module; E3 backfill complete |
| Erasure of per-user profiles on user erasure | Q6 (counsel and user-core payload) |

## 9. Open questions for the user (real blockers only)

1. **Q1 (blocks E2): is org-core the owner of key management** (new Control Plane responsibility, update ownership docs), or should a separate `key-core` service be built (XL)? Recommendation: org-core behind an interface.
2. **Q2 (blocks P2 enforce): what happens to existing profiles with no recorded owner?** Recommendation: `user_private` rows are hidden from everyone until an operator assigns an owner (precedent migration 0008), `org_shared` rows stay org-visible. Cost: users of existing persistent profiles must re-login or be rescued by an operator.
3. **Q3 (blocks E2 production): who holds and backs up the master key**, and is an environment master key acceptable for the first production release (KMS later)? Loss destroys every profile.
4. **Q4 (confirm default): `GET /v1/profiles/:id` never returns cookie values** (open question 13). Recommendation: yes, never; a future "export my cookies" is a separate audited DSAR feature. Confirm no operator workflow needs raw values.
5. **Q5 (confirm default): profile restore does not override the user agent** (locale and timezone only), per the honest-identity posture (ADR topic 5).
6. **Q6 (low priority, can wait): should user erasure delete that user's profiles** (needs `subject_type: "user"` handling and counsel input)?

## 10. INTERFACE SUMMARY

**Files touched.** *Ingestion, quarry-browser:* `session.rs` (schema v2, trait `save/exists`), `chromiumoxide.rs` (capture, restore, journal), `kernel.rs`, new `profile_capture.rs`, `profile_restore.rs`. *quarry-runtime:* `postgres_profile_store.rs` and `cached_profile_store.rs` (rewritten to blob level), new `profile_crypto.rs`, `profile_keys.rs`, `profile_blob_store.rs`, `envelope_profile_store.rs`, `profile_backfill.rs`, migrations 0014-0016 (provisional), `Cargo.toml` (ring, zeroize, moka). *quarry-edge:* `profile_routes.rs`, new `profile_guard.rs`, internal erase route, `agent_routes.rs` (attach check), `main.rs`, `config.rs`, tests. *quarry-control (Go):* `internal/gdpr/*`, `cmd/control/main.go`. *Control Plane:* org-core `internal/orgkeys`, handlers, scopes, migration 024, compose credential; ownership docs. *Frontend:* one gateway list-shape fix. *Docs/CI:* CONTRACTS, RUNBOOKS, openapi, Postgres CI job.

**New contracts.** org-core `POST /internal/orgs/:orgId/keys/data-keys[/unwrap|/rewrap]`, `rotate`, `DELETE keys`, scopes `org:keys:{wrap,unwrap,admin,destroy}:any`; envelope `{enc_v,alg,key_id,wrapped_dek,snapshot_id,z,nonce,ct}` (AES-256-GCM, AAD org|profile|snapshot); redacted `GET /v1/profiles/:id`; edge `DELETE /v1/internal/orgs/:org/profiles`; snapshot v2; flags `PROFILE_ENCRYPTION=off|compat|on`, `PROFILE_KEY_PROVIDER`, `PROFILE_OWNER_ENFORCE`, `ZDR_REFUSE_DURABLE_RESOURCES`, `CAPTURE_V2`, `ORG_CORE_KEYS_ENABLED`.

**Dependencies.** Hard: D3 S2 (C2), D3 S1 (S0), E1 before E2/E3, P1 before E1/P2. Soft: D4 S-8, D7, D10 S1b, D1, D2.

**Effort.** S0 1.5, P0 3, P1 4, E1 8, P2 4, C1 7, C2 5, E2 8, E3 5, S7 3 (optional): about 46 engineer-days, L near XL.

**ADR gates.** Topic 4 (ZDR enforcement); topic 5 (no UA replay); new 9.3 item 11: key-management ownership, master-key custody, owner/ACL model.

**Parallelisable with.** Track A and Track B; D5, D13 (scrape formats). **Conflicts with.** D3 (`chromiumoxide.rs`), D2 (`agent_routes.rs`), D4/D7/D10 (`profile_routes.rs`), D7 (org-core migrations and scopes), D6/D8/D12 (quarry-runtime migration numbers).

<!-- COMPLETE -->
