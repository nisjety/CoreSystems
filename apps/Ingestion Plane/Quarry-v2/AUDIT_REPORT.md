# Quarry V2 Contracts Audit Report

**Audit Date:** 2026-05-03  
**Scope:** `pkg/quarrycontracts` vs `docs/CONTRACTS.md` + `lab/README.md`  
**Status:** ⚠️ **INCOMPLETE — 3 MAJOR GAPS**

---

## Executive Summary

The `pkg/quarrycontracts` package implements **Phase 0 frozen contracts** but is **incomplete**:

- ✅ **Done:** ID scheme, REST envelope, event types (partial), run policy, output basics
- ❌ **Missing:** 9 schema types (lease, profile, cache, request formats, action, driver plan, TLS profile, webhook payload, artifact outputs)
- ❌ **Incomplete:** OutputFormats struct missing 7 fields
- ❌ **Undocumented:** 3 new ID kinds not in CONTRACTS.md
- ❌ **Inconsistent:** Event types missing 8 event categories

**Recommendation:** Fix before Phase 1 completes. Current contracts are **not frozen** — they are still scaffolding.

---

## 1. ID Kind Mismatch

### Documentation vs. Code

| Documented (CONTRACTS §1) | Implemented | Status |
|---------------------------|-------------|--------|
| run_, queue_, cp_, sch_, store_, snap_, art_, lease_, prof_, job_, evt_, whk_ | ✅ All present | ✅ |
| (end of list) | **whkd_, block_, req_** | ❌ NOT DOCUMENTED |

### Gap Details

**File:** `pkg/quarrycontracts/ids.go` lines 9-27

Code has three additional ID kinds not in CONTRACTS.md §1:

```go
KindWebhookDelivery IDKind = "whkd_"  // Not in spec
KindBlocklist       IDKind = "block_" // Not in spec
KindRequest         IDKind = "req_"   // Not in spec
```

**Issue:** CONTRACTS.md must document these or code must remove them. This violates the "Frozen" contract principle in CONTRACTS §0 ("Authoritative across Rust + Go").

**Fix:**
- Either add to CONTRACTS.md §1 table with definitions
- Or remove from code and use qualified references (e.g., `whk_` for both webhooks and deliveries via a `delivery_type` discriminator field)

**Rationale:** If Go has `whkd_`, then Rust `quarry-core` must also have it. Check `crates/quarry-core/src/ids.rs`.

---

## 2. OutputFormats Incomplete

### Documented vs. Implemented

**File:** `pkg/quarrycontracts/output.go` lines 11-30

CONTRACTS.md §9 specifies these format artifacts:

```json
"formats": {
  "html": {},
  "markdown": {},
  "raw": {},
  "links": [],
  "images": [],           // ❌ MISSING
  "screenshot": {},
  "pdf": {},
  "extract": {},
  "summary": {},          // ❌ MISSING
  "attributes": {},       // ❌ MISSING
  "branding": {},         // ❌ MISSING
  "audio": {},            // ❌ MISSING
  "change": {},           // ❌ MISSING
  "chunks": {},           // ❌ MISSING
  "meta": {}              // ❌ MISSING
}
```

**Implemented:**

```go
type OutputFormats struct {
    HTML       *FormatRef  `json:"html,omitempty"`
    Markdown   *FormatRef  `json:"markdown,omitempty"`
    Raw        *FormatRef  `json:"raw,omitempty"`
    Links      []Link      `json:"links,omitempty"`
    Screenshot *FormatRef  `json:"screenshot,omitempty"`
    PDF        *FormatRef  `json:"pdf,omitempty"`
    Extract    *ExtractRef `json:"extract,omitempty"`
    // Missing 7 fields ↓
}
```

### Fields to Add

| Field | Type | CONTRACTS Reference |
|-------|------|---------------------|
| `Images` | `[]ImageRef` | §9 (array of {src, alt, title, width, height}) |
| `Summary` | `*FormatRef` | §9, Phase 6 (artifact-backed) |
| `Attributes` | `*FormatRef` | §9, Phase 6 (selector extraction) |
| `Branding` | `*FormatRef` | §9, Phase 6, feature-gated (browser/CDP only) |
| `Audio` | `*FormatRef` | §9, Phase 6, feature-gated |
| `Change` | `*FormatRef` | §9, Phase 6 (artifact: paragraph diff + fingerprint) |
| `Chunks` | `*FormatRef` | §9, Phase 6 (deterministic chunk boundaries) |
| `Meta` | `*FormatRef` | LLM-Wiki.md, Phase 6 (metadata sidecar: run_id, urls, fingerprints, driver_plan, policy, profile/snapshot ids) |

### Associated Types to Add

```go
// Image artifact (array element for Images field)
type ImageRef struct {
    Src    string  `json:"src"`
    Alt    *string `json:"alt,omitempty"`
    Title  *string `json:"title,omitempty"`
    Width  *uint32 `json:"width,omitempty"`
    Height *uint32 `json:"height,omitempty"`
}

// Chunks artifact (array of chunk records)
type ChunkRef struct {
    ArtifactID ID     `json:"artifact_id"`
    Count      uint32 `json:"count"` // number of chunks in this artifact
}
```

**Fix:** Add all 7 fields + associated types to `output.go`.

**Priority:** High (Phase 2 runtime will emit these artifacts; control plane must accept them).

---

## 3. Event Types Incomplete

### Documented vs. Implemented

CONTRACTS.md §3 specifies **22+ event types**. Code in `event.go` implements **only 16**.

**Missing categories:**

| Category | Events | Status |
|----------|--------|--------|
| Driver selection | `driver.planned`, `driver.fallback` | ❌ Missing 2 |
| Transport | `transport.probed` | ❌ Missing 1 |
| Actions | `action.started`, `action.completed`, `action.failed` | ❌ Missing 3 |
| Preview | `preview.screenshot`, `preview.markdown`, `preview.artifact` | ❌ Missing 3 |
| Agent | `agent.started`, `agent.delta`, `agent.completed`, `agent.failed` | ❌ Missing 4 (Phase 8+) |

**Implemented but use underscore naming:** Code uses `EvtPageFetched` (underscore style) where CONTRACTS.md uses `page.fetched` (dot style).

### Specific Gaps

**File:** `pkg/quarrycontracts/event.go` lines 4-27

```go
// Missing:
// - EvtDriverPlanned   EventType = "driver_planned"
// - EvtDriverFallback  EventType = "driver_fallback"
// - EvtTransportProbed EventType = "transport_probed"
// - EvtActionStarted   EventType = "action_started"
// - EvtActionCompleted EventType = "action_completed"
// - EvtActionFailed    EventType = "action_failed"
// - EvtPreviewScreenshot EventType = "preview_screenshot"
// - EvtPreviewMarkdown EventType = "preview_markdown"
// - EvtPreviewArtifact EventType = "preview_artifact"
// Note: agent.* events deferred to Phase 8/9 (LLM eval integration)
```

**Event Type Naming Inconsistency:** 

The code uses underscore (`run_started`) but CONTRACTS.md uses dots (`run.started`). This is a **canonical form choice**:
- CONTRACTS.md shows dots for readability in JSON schema section
- Code normalizes to underscores for Go constant naming
- JSON wire format should match code (underscore)

**Action:** Decide: are CONTRACTS.md examples aspirational (dots in JSON) or literal (underscores)? Document the canonical form.

**Fix:** Add the 8 missing event type constants to `event.go`.

**Priority:** Medium (driver planning is Phase 2; transport probes in Phase 2; actions in Phase 2/4).

---

## 4. Missing Contract Types (Not in quarrycontracts Package)

CONTRACTS.md defines 9 additional schemas beyond what's currently in `pkg/quarrycontracts`. These are **referenced by Phase 1+ work** but not yet codified:

| Type | CONTRACTS Reference | Purpose | Status |
|------|---------------------|---------|--------|
| `BrowserLeaseSchema` | §8 | Lease model: profile_id, session_affinity_key, proxy_affinity, capabilities, ttl_s, artifact_bucket | ❌ Missing |
| `BrowserProfileSnapshot` | §8 | Profile state: cookies, localStorage, sessionStorage, indexed_db, user_agent, viewport, locale, timezone, affinity, validation, storage, encryption | ❌ Missing |
| `CachePolicy` | §6 | mode, max_age_s, vary_on, stale_while_revalidate_s | ❌ Missing |
| `RequestedFormats` | §9 "Requested formats" | Firecrawl-compatible `formats` array with inline type configs | ❌ Missing |
| `ActionSchema` | §9 "Action schema" | wait, click, write, press, scroll, screenshot, scrape, executeJavascript, pdf with limits | ❌ Missing |
| `DriverPlanSchema` | §9 "Driver plan schema" | requested, chosen, fallbacks, signals (cache_hit, requires_js, known_static, blocked_previous_attempt) | ❌ Missing |
| `TLSProfileSchema` | §9 "TLS profile schema" | mode, implementation, version_hint, boringssl, probe | ❌ Missing |
| `WebhookPayload` | §4 | delivery_id, signature, event | ❌ Missing |
| `MapRequest/MapResponse` | §9 "/v1/map" | Discovery-only responses (lighter than scrape) | ❌ Missing |

### Where These Are Needed

**Phase 1 (now):** 
- `WebhookPayload` — dispatcher in control plane (services/quarry-control/internal/dispatcher/)

**Phase 2 (runtime):**
- `RequestedFormats` — edge request parsing
- `ActionSchema` — browser action runtime
- `DriverPlanSchema` — metadata artifact

**Phase 4 (browser leases):**
- `BrowserLeaseSchema` — lease model
- `BrowserProfileSnapshot` — profile storage

**Phase 6 (change tracking):**
- `CachePolicy` — cache policy enforcement

**Phase 3+ (future):**
- `TLSProfileSchema` — profile selection
- `MapRequest/MapResponse` — /v1/map endpoint

### Fix

These types should be defined in `pkg/quarrycontracts` with full serde support, then mirrored in Rust `quarry-core`. For now, add them as comments + stubs so the contract is explicit:

```go
// BrowserLeaseSchema (Phase 4)
// type BrowserLease struct {
//     LeaseID          ID            `json:"lease_id"`
//     ProfileID        *ID           `json:"profile_id,omitempty"`
//     SessionAffinityKey string      `json:"session_affinity_key"`
//     ProxyAffinity    ProxyAffinity `json:"proxy_affinity"`
//     TTLSeconds       uint32        `json:"ttl_s"`
//     Capabilities     []string      `json:"capabilities"` // "js", "screenshots", "pdf", "actions"
//     ArtifactBucket   string        `json:"artifact_bucket"`
// }
```

**Priority:** Medium-High (needed for Phase 1 hook signatures and Phase 2+ runtime).

---

## 5. Lab Folder Assessment

### File: `lab/README.md`

✅ **Correct.** Lab scope document accurately describes:

- **Scope:** Extraction prompts, anti-bot experiments, ML driver selection, eval harness (Phases 8+)
- **Boundaries:** "Never in hot path. Outputs feed back via artifacts or static config, never via runtime call."
- **Structure:** Proposed 4 subdirs (extraction/, antibot/, driver_select/, evals/)
- **Implementation:** "No services here. Notebooks, scripts, reports only."

### Current State

❌ **Not yet implemented.** The lab directory exists but:
- No Python notebooks
- No evaluation harness code
- No extraction prompt sweeps
- No anti-bot strategy experiments

### What Needs to Be Done

Per PROGRESS.md Phase 8 and PLAN.md Phase 8:

```
lab/
├── extraction/        # prompt + schema sweeps vs. gold set
│   ├── sweep.py       # hyperparameter search over extraction prompts
│   └── results.json   # scored results
├── antibot/           # challenge fingerprint learning
│   ├── fingerprint.py # challenge detection heuristics
│   └── corpus/        # test URLs with challenge signals
├── driver_select/     # static vs. browser ML classifier (future)
│   └── model.pkl      # trained classifier
└── evals/             # benchmark harness + scoreboards
    ├── fixtures/      # 20+ golden test URLs
    ├── run_eval.py    # orchestrate eval suite
    ├── compare.py     # compare Quarry V1 vs V2 vs Firecrawl
    └── SCOREBOARD.md  # auto-generated results
```

**Status in PROGRESS.md:** Phase 8 is "In progress" but lab is "Not started" (empty folder).

---

## 6. Summary: Critical Path Fixes

| Issue | File(s) | Fix | Phase | Deadline |
|-------|---------|-----|-------|----------|
| 3 undocumented ID kinds | CONTRACTS.md + ids.go | Document in CONTRACTS §1 or remove from code | 0 | **ASAP** |
| OutputFormats missing 7 fields | output.go | Add Images, Summary, Attributes, Branding, Audio, Change, Chunks, Meta | Phase 2 | Before Phase 2 ships |
| 8 event types missing | event.go | Add driver/action/preview events; clarify dot vs underscore naming | Phase 2 | Before Phase 2 ships |
| 9 schema types missing | N/A in pkg/ | Add stubs + comments for Lease, Profile, CachePolicy, Formats, Action, DriverPlan, TLS, Webhook, Map | Phase 1-2 | Phase 1 final |
| Lab not implemented | lab/ | Create Python structure + eval harness scaffolding | Phase 8 | Phase 8 (not blocking) |

---

## 7. Validation Checklist

Before Phase 1 deployment:

- [ ] All ID kinds in ids.go match CONTRACTS.md §1
- [ ] OutputFormats has all 15 fields (html, markdown, raw, links, images, screenshot, pdf, extract, summary, attributes, branding, audio, change, chunks, meta)
- [ ] All 22 event types in event.go match CONTRACTS.md §3
- [ ] At least stubs for Lease/Profile/CachePolicy/Formats/Action/DriverPlan/TLS/Webhook schemas in contracts package or as comments
- [ ] Rust quarry-core mirrors all Go additions
- [ ] Control plane tests pass for all new ID kinds and event types

---

## 8. Rust Mirror Check

**Action Required:** Verify that `crates/quarry-core/src/ids.rs` and `crates/quarry-core/src/event.rs` have:

- [ ] whkd_, block_, req_ ID kinds
- [ ] All 22 event types
- [ ] Missing 9 schema types (can be in separate modules)

If Rust is ahead, copy those definitions back to Go.

---

## Appendix: Complete Event Type List

### Documented (CONTRACTS.md §3)

1. `run.started`
2. `run.paused`
3. `run.resumed`
4. `run.cancelled`
5. `run.completed`
6. `run.failed`
7. `page.queued`
8. `page.fetched`
9. `page.failed`
10. `page.blocked`
11. `page.retried`
12. `page.escalated`
13. **driver.planned** ❌
14. **driver.fallback** ❌
15. **transport.probed** ❌
16. **action.started** ❌
17. **action.completed** ❌
18. **action.failed** ❌
19. **preview.screenshot** ❌
20. **preview.markdown** ❌
21. **preview.artifact** ❌
22. `artifact.written`
23. `snapshot.created`
24. `store.record.written`
25. `lease.acquired`
26. `lease.released`
27. `profile.restored`
28. `profile.captured`
29. `change.detected`
30. `change.unchanged`
31. `schedule.fired`
32. **agent.started** (Phase 8+) ❌
33. **agent.delta** (Phase 8+) ❌
34. **agent.completed** (Phase 8+) ❌
35. **agent.failed** (Phase 8+) ❌

**In Code:** 1-12, 22-31 (16 types)  
**Missing:** 13-21, 32-35 (9 types; agent.* deferred)
