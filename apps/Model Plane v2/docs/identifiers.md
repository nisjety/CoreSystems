# Canonical Identifiers — Model Plane v2

**Status:** Accepted (Phase 0 Contract Lock)
**Owner:** Platform / agent-core
**Last updated:** 2025

This document is the source-of-truth for every canonical ID used across Model
Plane v2. All services MUST conform to these formats, regexes, and ownership
rules. Any new identifier must be added here before being emitted in events,
stored in Postgres, or returned from a public API.

---

## 1. Rules

1. **UUIDv7 by default.** All new runtime IDs are UUIDv7 (time-ordered, 128-bit).
   Use ULIDs only if explicitly noted.
2. **Prefix-tagged strings for human-visible or externally-issued IDs** (e.g.
   `sk_…` for skills, `twk_…` for workspaces). Prefixes are lowercase snake and
   bounded to `[a-z_]{2,8}` followed by `_`.
3. **Length limits.** Prefix-tagged IDs are at most 64 chars. UUIDs are the
   canonical 36-char hyphenated form in JSON, stored as `uuid` in Postgres.
4. **Generators.** The service listed as *Owner* is the only service allowed to
   mint the ID. Every other service treats it as opaque.
5. **Case sensitivity.** All IDs are case-sensitive. Never lowercase on
   ingestion.
6. **No reuse.** IDs are never reused, even after deletion / soft-delete.
7. **Logging.** Every log line emitted inside a run MUST carry `run_id`,
   `session_id`, and `org_id` at minimum (see `event-envelope.md`).

---

## 2. Canonical ID catalog

| Name | Format / Regex | Generator | Owner Service | Example | Notes |
|---|---|---|---|---|---|
| `org_id` | UUIDv7 | control plane provisioning | tenant-admin (external) | `01936f3e-8b5a-7f2c-a1d0-6e9d4c2b1a38` | Tenant root. Present on every event and row. |
| `user_id` | UUIDv7 | auth / identity provider | session-core (proxy) | `01936f3e-9c21-7aab-bb47-112233445566` | Maps to identity provider subject. |
| `session_id` | UUIDv7 | session-core | session-core | `01936f40-1122-7b00-9a81-aabbccddeeff` | One per authenticated client session. |
| `thread_id` | UUIDv7 | agent-core | agent-core | `01936f41-aaaa-7c11-80c2-112200330044` | Conversation thread; may span runs. |
| `research_thread_id` | UUIDv7 | research-core | research-core | `01936f42-bbbb-7d22-9001-556677889900` | Distinct from `thread_id`; owned by research-core only. |
| `run_id` | UUIDv7 | agent-core | agent-core | `01936f43-cccc-7e33-88aa-ddeeff001122` | Root of execution state machine. Parent of all actions. |
| `parent_run_id` | UUIDv7 | agent-core | agent-core | `01936f43-cccc-7e33-88aa-ddeeff001122` | Set on sub-runs (tool graph, delegation). NULL on root runs. |
| `action_id` | UUIDv7 | agent-core | agent-core | `01936f44-dddd-7f44-80bb-223344556677` | One per model-requested action inside a run. |
| `tool_call_id` | String, provider-issued, max 128 chars, regex `^[A-Za-z0-9_\-:]{1,128}$` | LLM provider | llm-worker (captured) | `call_abc123XYZ` | Opaque echo-back to provider. Stored as received; never minted locally. |
| `approval_id` | UUIDv7 | agent-core | agent-core | `01936f45-eeee-7aaa-8800-334455667788` | One per human-in-the-loop gate. |
| `connector_delivery_id` | UUIDv7 | capability-core | capability-core | `01936f46-ffff-7bbb-9911-445566778899` | One per outbound tool invocation dispatched to a connector. |
| `workspace_id` | Prefix `twk_` + 26-char Crockford base32 (ULID body), regex `^twk_[0-9A-HJKMNP-TV-Z]{26}$` | execution-core | execution-core | `twk_01HQZ5R9T2M3N4P5Q6S7V8W9X0` | Identifies an ephemeral or persistent sandbox workspace. |
| `sandbox_id` | UUIDv7 | execution-core | execution-core | `01936f47-1111-7ccc-aa22-556677889900` | One per concrete sandbox instance inside a workspace. |
| `skill_id` | Prefix `sk_` + 16-char Crockford base32, regex `^sk_[0-9A-HJKMNP-TV-Z]{16}$` | capability-core | capability-core | `sk_01HQZ5R9T2M3N4P5` | Logical skill identity, version-agnostic. |
| `skill_version` | SemVer 2.0.0, regex `^\d+\.\d+\.\d+(-[\w\.\-]+)?$` | capability-core | capability-core | `1.4.2` | Pinned alongside `skill_id` in every invocation. |
| `trajectory_id` | UUIDv7 | agent-core | agent-core | `01936f48-2222-7ddd-bb33-667788990011` | Ordered sequence of actions taken within a run; 1:1 with `run_id` today but reserved for future branching. |
| `query_depth` | Integer `0..8` | agent-core | agent-core | `3` | Recursion depth for nested/delegated runs. Hard cap 8; enforced at agent-core. |

---

## 3. Forbidden patterns

- ❌ Auto-incrementing integers as public IDs.
- ❌ Reusing `thread_id` as `run_id`.
- ❌ Minting `tool_call_id` locally (must come from the provider).
- ❌ Embedding `org_id` or user info inside another ID's payload.
- ❌ Using UUIDv4 for new resources. Existing v4 IDs stay; new ones are v7.

---

## 4. Cross-references

- Event envelope — see `event-envelope.md` (uses every ID above as a typed column).
- Run state machine — see `run-state-machine.md` (keyed on `run_id`).
- Ownership — see `adr/ADR-001-service-ownership-matrix.md`.
