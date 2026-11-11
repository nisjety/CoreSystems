# ADR 10: browser execution substrate

**Status:** Proposed, 2026-11-11. Decision memo for the owner. This document accepts nothing.

**Decision owner:** The user. The infrastructure owner sets the memory budget and approves any host or proxy vendor outside the owner's control. Ingestion Plane owns the substrate under every option.

**Scope:** Quarry browser execution. `QV` below means `apps/Ingestion Plane/Quarry-v2`. This memo changes no code, and no measurement was run to write it.

**Basis:** Commit `48a86a3fc`. Line numbers are as of that commit. The working tree is shared and moves, so re-check a line before citing it elsewhere.

**Plan records:** `docs/plans/quarry-browser-build-plan-2026-10-14.md` (ADR-10 row at line 396; epics E4.4 and E4.5 at lines 3382-3401); `.plan-work/arch-A-decisions.md` (ADR-10 at lines 180-181; X-15 at lines 185-196); `.plan-work/quarry-w1w2/W4-BLOCKERS-RANKED.md` (ranks 2, 6 and 9); `.plan-work/quarry-w1w2/W4-ORCH2.md` (section 2, row E4.4-c; section 4, ADR 5 row).

## Summary

Keep the local browser at one process per driver instance, and make no isolation claim between sessions. Build no pool, sidecar or remote backend until the owner decides, and fill the measurement table in section 3 first. Without Browserbase, the edge builds two local driver instances, so any memory budget has to cover both.

## 1. Context

### 1.1 One process per driver instance, two instances per edge

- The driver runs one browser process. `QV/crates/quarry-browser/src/chromiumoxide.rs:130` says so ("The driver runs one browser process today (K = 1, ADR-10)"), `SLOT` is 0 (line 131), and `QV/crates/quarry-browser/src/slots.rs:245` builds the slot book with `slots: 1`. The slot module's own doc says the same (`slots.rs:10-11`).
- Without Browserbase, the edge builds two local drivers. One serves scrape and is registered only when no Browser driver exists (`QV/crates/quarry-edge/src/main.rs:541-556`). The other serves agent runs and is always built under the `browser-agent` feature, which is a default feature (`main.rs:1776-1781`; `QV/crates/quarry-edge/Cargo.toml:74`).
- Each instance owns its own slot book (`chromiumoxide.rs:354`) and its own browser (`chromiumoxide.rs:345`). K = 1 therefore holds per instance, and the edge can run two Chromium processes.
- The slot book admits a session to the least loaded slot (`slots.rs:457`). A pool only has to raise the slot count.
- Defaults are 40 sessions and 24 pages per slot (`slots.rs:146-147`). `HEAVY_LANE_BUDGET` is 1 (`slots.rs:257`).
- `session_cap_warnings` (`slots.rs:269`) is WARN-only by design (`slots.rs:259-264`) until the Chromium RSS at 8, 16 and 32 contexts is recorded. The edge logs its output at boot (`main.rs:287-293`).
- Each session's tabs live in their own browser context (`create_session_context`, `chromiumoxide.rs:1058`). Every context in an instance shares that instance's single browser process.
- A process loss takes every session on the slot. `SlotBook::mark_dead` releases each one and tombstones it as `browser_process_lost` (`slots.rs:694-716`).
- The scrape path's adapter pool is built with two permits (`main.rs:547`). That bounds leases, not browser processes.

### 1.2 Agent containment and the egress boundary

- Agent runs use only the local driver. `agent_driver` is always a `ChromiumoxideDriver` with the pinned proxy and profile store attached (`main.rs:1777-1781`). No remote driver can reach an agent run today.
- `require_isolated_agent_egress` (`QV/crates/quarry-edge/src/agent_routes/admission.rs:139-149`) refuses an agent run unless the driver advertises both `isolated_egress` and `security_evidence`. The error code is `unsupported`.
- The local driver advertises both only when a pinned proxy is wired (`chromiumoxide.rs:4974-4975`). The trait default is all false (`QV/crates/quarry-browser/src/lib.rs:1129-1131`).
- `PinnedEgressProxyEndpoint` (`QV/crates/quarry-browser/src/lib.rs:959`) accepts only a credential-free HTTP endpoint on a loopback address with a port (`lib.rs:962-990`). A third-party proxy cannot be passed under that type.
- Each session's browser context binds to its own endpoint (`proxy_server`, `chromiumoxide.rs:1062`) and removes Chromium's implicit loopback bypass (`proxy_bypass_list("<-loopback>")`, line 1066). `--disable-quic` is passed when a pinned proxy is set (line 992), because QUIC can open sockets outside the HTTP proxy path.
- There is no upstream proxy chaining. The header of `QV/crates/quarry-runtime/src/browser_egress_proxy.rs` (lines 1-8) describes a local proxy that connects straight to the vetted address. A search of `QV/crates` finds no `--proxy-server` setting and no upstream proxy configuration.

### 1.3 Remote shims are compiled but never registered

- Kernel (`QV/crates/quarry-browser/src/kernel.rs`: driver at line 91, trait impl at 494, unit tests at 805) sits behind cargo feature `kernel` (`lib.rs:659-660`). That feature is in `quarry-browser`'s default set (`QV/crates/quarry-browser/Cargo.toml:38`), and `quarry-edge` depends on the crate with its defaults (`QV/crates/quarry-edge/Cargo.toml:19`). Kernel therefore compiles by default.
- Browserless (`QV/crates/quarry-browser/src/browserless.rs`: driver at line 104, trait impl at 192, unit tests at 312) is compiled unconditionally (`lib.rs:581`).
- Neither appears in `QV/crates/quarry-edge/src/main.rs`. Browserbase is the only remote driver registered (`main.rs:518-539`).
- Kernel, Browserless and Browserbase do not override `capabilities()`, so they report the all-false default.
- `SlotTransport::RemoteCdp` does not exist in the tree. The plan records it as deliberately unwritten.
- `docs/DRIVER_MATRIX.md` still presents Kernel and Browserless as a working fallback (lines 19-24, 45-50 and 55-66). Line 66 credits Kernel with per-session VM isolation. This memo does not change that file.

### 1.4 Sandbox and sidecar

- The compose default is `QUARRY_BROWSER_NO_SANDBOX: ${QUARRY_BROWSER_NO_SANDBOX:-0}` (`QV/deploy/compose/docker-compose.yml:134`). The production launch refuses the opt-out (`sandbox_policy` in `chromiumoxide.rs`, lines 2660-2666).
- `apps/Model Plane/go/services/sandbox-manager` is a durable lease and process registry (`internal/lease/lease.go:119`). It imports no `os/exec` and starts no browser or container. Its process store says the host performs the kill (`internal/process/store.go:933-934`). A per-org container pool would therefore be an Ingestion-owned service, not a Model Plane lease.
- No browser sidecar exists in `QV/crates`. The only sidecar code is an unrelated vision sidecar and an artifact metadata sidecar.

### 1.5 ADR 5 and ZDR

- ADR 5 part (a), honest identity, is recorded as decided (`W4-ORCH2.md` section 4). The code default is `Honest` (`QV/crates/quarry-runtime/src/escalation.rs:34`). Under honest, a status never changes identity, and only a transport failure rotates a proxy (`escalation.rs:3-8`). The egress broker does not count an origin status against proxy health in honest mode (`QV/crates/quarry-runtime/src/egress_broker.rs:176`). D-W5-2 (user agent and `Accept-Language`) is still open and gates E4.4-c.
- `W4-BLOCKERS-RANKED.md` rank 6 still says ADR 5 has "no record". The ledger should reconcile that with the W4-ORCH2 entry.
- The processor guard (`QV/crates/quarry-core/src/privacy.rs:65-99`) refuses every third-party browser or proxy under ZDR (`zdr_denies_third_party`, line 77). It also refuses credential or secret data, and it needs both `allow_third_party_processing` and a matching `processor_id` (lines 86-97). Managed browser providers run it before every fetch (`QV/crates/quarry-runtime/src/browser_driver.rs:614-618`).
- `docs/compliance/subprocessor-list.md:35` has one generic row for browser-automation, residential-proxy and unblocker vendors, marked "TBC — verify per vendor". No browser vendor is named.

### 1.6 Measurement hooks that already exist

- `measure_memory_per_open_session` (`chromiumoxide.rs:12028`, ignored by default) prints `MEASURE sessions=N processes=... pss_mib=... rss_mib=...` for N = 0, 1, 8, 16 and 32, with one tab each, on a synthetic page (`synthetic_page`, line 12010). It sums `Pss:` and `Rss:` from `/proc/<pid>/smaps_rollup` over processes whose name starts with `chrom`. Off Linux it returns zeros (`chromium_memory_kib`, line 11971).
- Command, from the test's doc comment: `CHROMIUMOXIDE_TEST=1 CHROMIUMOXIDE_MEASURE=1 cargo test -p quarry-browser --features chromiumoxide measure_memory_per_open_session -- --ignored --nocapture`.
- Cold start: each acquire records `startup_mode` (warm or cold) and `startup_latency_ms` (`chromiumoxide.rs:5093-5111`). The fields are defined in `QV/crates/quarry-core/src/contracts.rs:137-139`.
- Renderer telemetry (`chromiumoxide.rs:6466`) is per page and is documented as not host-wide accounting (`contracts.rs:131-133`). It cannot fill the RSS table.

## 2. Options

| | A. K = 1, no isolation claim (recommended now) | B. Pool of local slots after a measured budget | C. Per-org containers, each with a pinned-proxy sidecar | D. Remote CDP providers, Browserbase first |
|---|---|---|---|---|
| Unlocks E4.4-c tiers | No | No | Gives the browser path the sidecar that E4.4 requires. The tier policy still needs ADR 5 and the E4.4-c design. | No |
| Unlocks F-t2-059 | No | No | No | Yes, all parts, once the section 6 gates clear |
| Unlocks F-t2-060 step 2 | No | No | Yes | No |
| Unlocks WS3.G beyond K = 1 | No | Yes | No | No |
| Main costs | One failure domain per instance. Throughput is one process. | Memory per slot (unmeasured). Host limit is a placeholder. Session ordering is WARN-only. | Container lifecycle. A sidecar per container. Memory per org. Security-evidence proof. Org-qualified session keys. Per-provider SSRF tests. | Processor under a DPA. Refused for ZDR. Live-view URLs carry tokens. Provider identity is outside Quarry's control. |
| ADR 5 | No change. UA and `Accept-Language` stay under D-W5-2. | One identity across slots. No per-slot variation. | One identity across containers. Proxy rotation on transport failure only. | Quarry cannot verify provider identity. No identity change on a block. No CAPTCHA solving. |
| DPA and ZDR | No new processor. ZDR unchanged. | No new processor. ZDR unchanged. | No new processor if the owner runs every host. Any outside host or proxy is a processor. ZDR runs keep no profile volume and no content log. | A processor. DPA review and a subprocessor row (both TBC) before any enablement. Refused for ZDR. |

### A. K = 1, no isolation claim (recommended now)

- Keep one browser process per driver instance. Keep the egress claim as it is. It holds only with the pinned proxy (`chromiumoxide.rs:4974-4975`).
- Make no claim that sessions are isolated from one another. Sessions in one instance share a browser process.
- Per-page locale and timezone emulation (E4.4-a) does not need this decision. The plan's E4.4 recommendation builds E4.4-a and E4.4-b (emulation and profile pinning; `docs/plans/quarry-browser-build-plan-2026-10-14.md` lines 3384 and 3390) and waits for ADR 10 only for the tiers, E4.4-c. W4-BLOCKERS-RANKED rank 6 says ADR 5 gates the UA, `Accept-Language` and device parts of E4.4-a and does not block its locale and timezone part.
- The caps (32 global, 8 per org, 6 interactive per org) stay placeholders until section 3 is filled (`arch-A-decisions.md` X-15, D2 Q1).

### B. Pool of local slots after a measured memory budget

- The book already supports it (`slots.rs:457`). The work is in the numbers: the host memory limit (a 4 GiB placeholder in `arch-A-decisions.md:193`), the `session_cap_warnings` ordering (WARN-only today), and the heavy-lane budget.
- A crash would then lose one slot's sessions rather than all of them. That is crash containment, not an isolation claim (plan ADR-10 row).
- It adds no loopback chain, no remote backend and no per-org boundary.
- The budget has to cover both driver instances (section 1.1).

### C. Per-org containers, each with a pinned-proxy sidecar, owned by Ingestion

- Chromium keeps a loopback endpoint, and the sidecar makes the outbound connection, inside one network namespace per container. The plan's phrase is "loopback-only type satisfied in one network namespace" (E4.5). The type's loopback rule is kept, so no relaxation is needed. The E4.4 text asks for a pinned chain or a sidecar before any browser tier ships, and this is the sidecar option.
- It gives F-t2-060 step 2 its shape. Ingestion owns the lifecycle, since sandbox-manager starts nothing (section 1.4).
- Costs: per-container memory and cold start (both unmeasured); an operations surface; one sidecar per container; org-qualified session keys (E4.5); per-provider redirect, iframe and subresource SSRF tests; and a current security-evidence proof, because `require_isolated_agent_egress` refuses without `security_evidence`.
- ADR 5: one identity across all containers. The broker's block-status rotation is legacy-only (`egress_broker.rs:176`), and the container design must not bring it back.
- ZDR: a ZDR run keeps no persistent profile volume and no content log. The design must show how.
- DPA: none new if the owner runs every host. A host or proxy vendor outside the owner's control is a processor and needs a DPA and a subprocessor row.

### D. Remote CDP providers behind the processor guard, Browserbase first

- Unlocks F-t2-059. Its parts are: register Kernel and Browserless behind config and the processor guard, after F-t2-074 (the plan confirms raw provider bodies reach errors, line 3343); add a CDP-attached driver over `SlotTransport::RemoteCdp`, which is not written; replace the single agent driver with a backend registry in `AppState` (E4.5 part 2); and expose `GET /v1/agent/backends` (part 3). It also needs E5.2-074 `ProviderError` (`W4-BLOCKERS-RANKED.md:77`).
- Containment: a remote provider gives Quarry no containment proof. Agent use still needs `isolated_egress` and `security_evidence` from the driver.
- ZDR: refused (`privacy.rs:77`). ZDR orgs get no remote backend.
- DPA: a processor. The subprocessor list names no browser vendor (`subprocessor-list.md:35`). A DPA review and a subprocessor row come before any enablement.
- ADR 5: the provider's browser identity is outside Quarry's control. Quarry must not ask a provider to solve CAPTCHAs, mask fingerprints or change identity on a block.
- Live-view URLs carry the session token when one is configured (`browserless.rs:214-215`). A live-view feature needs a token policy first.

### Recommendation

1. Option A now.
2. Option B only after section 3 is filled on a Linux stack and the owner accepts a memory budget that covers both driver instances.
3. Option C before any option D. It keeps containment inside Quarry's boundary.
4. Option D only on a scale need, behind the processor guard, with a DPA review and a subprocessor row in place, and never for ZDR.

## 3. What to measure

No measurement was run for this memo. Every result cell is "not measured".

| Measurement | Method | Result |
|---|---|---|
| Chromium RSS at 0 contexts (baseline) | `measure_memory_per_open_session`, `MEASURE sessions=0` | not measured |
| Chromium RSS at 1 context | same, `sessions=1` | not measured |
| Chromium RSS at 8 contexts | same, `sessions=8` | not measured |
| Chromium RSS at 16 contexts | same, `sessions=16` | not measured |
| Chromium RSS at 32 contexts | same, `sessions=32` | not measured |
| PSS at 1, 8, 16 and 32 contexts (container sizing) | same, `pss_mib` | not measured |
| Browser process count at 1, 8, 16 and 32 contexts | same, `processes` | not measured |
| Cold acquire latency | `startup_latency_ms` where `startup_mode` is cold | not measured |
| Warm acquire latency | `startup_latency_ms` where `startup_mode` is warm | not measured |
| Sidecar feasibility: Chromium reaches a loopback sidecar inside one network namespace | design spike; no harness exists | not measured |
| Sidecar cost: added latency per request through the sidecar | design spike; no harness exists | not measured |

Notes:

- The harness uses a synthetic page of about 3,000 DOM rows and a 50,000-item array. Its own comment says real pages use more memory, so its numbers are a floor.
- The harness needs Linux (`/proc`). Run it on the Linux stack that the plan lists as B6 (`W4-BLOCKERS-RANKED.md` rank 9), not on a Windows host.
- The harness drives one driver instance. Run it with both instances active to size the edge (section 1.1).

## 4. Corrections to other documents (not edited here)

- `docs/DRIVER_MATRIX.md` lines 19-24, 45-50 and 55-66 present Kernel and Browserless as a working fallback. Line 66 credits Kernel with per-session VM isolation.
- Plan E4.5 (line 3396) cites compose `:-1` at lines 97-99. HEAD has `:-0` at `QV/deploy/compose/docker-compose.yml:134`.
- Plan E4.4 (line 3386) and `W4-BLOCKERS-RANKED.md` rank 2 cite `PinnedEgressProxyEndpoint` at `lib.rs:331-360`. It is at `QV/crates/quarry-browser/src/lib.rs:959`.
- Plan E4.5 (line 3397) cites `require_isolated_agent_egress` at `agent_routes.rs:1131`, Browserbase registration at `main.rs:312-339`, the Kernel feature at `lib.rs 155-159`, and the scrape session key at `browser_driver.rs:139-148`. They now sit at `agent_routes/admission.rs:139`, `main.rs:518-539`, `lib.rs:659-660` (default-on at `Cargo.toml:38`), and lines that hold DOM sampling code, respectively.
- `W4-BLOCKERS-RANKED.md` rank 6 ("no record") and `W4-ORCH2.md` section 4 ("(a) honest identity decided") disagree.
- The ADR-10 row (plan line 396) lists F-t2-058 as a gate. Read with the E4.4 recommendation, that means its tier part only. The row should say so.
- `arch-A-decisions.md` ADR-10 describes K = 1 without noting that the edge builds two local driver instances (section 1.1).

## 5. Decision requested from the owner

Ratify option A as ADR 10: one browser process per local driver instance, no isolation claim between sessions, and no pool until a measured memory budget exists.

This does not choose B, C or D. Each returns to this memo with its own evidence before anything is built. The caps stay placeholders until section 3 is filled.

## 6. Blocked until the decision is made

These stay blocked until section 5 is decided and the chosen option is built:

1. E4.4-c egress tiers (browser proxy chain and tier policy). Also needs ADR 5 and D-W5-2 (`W4-ORCH2.md` section 2, row E4.4-c).
2. E4.5 F-t2-059 remote backends, parts 1 to 3. Also needs E5.2-074 `ProviderError` (`W4-BLOCKERS-RANKED.md:77`).
3. E4.5 F-t2-060 step 2: CDP-connect mode and a per-org container pool. Step 3 stays parked.
4. Kernel and Browserless registration (part 1 of F-t2-059). It also waits for F-t2-074 and the processor guard.
5. WS3.G beyond K = 1, the local slot pool (named in the plan's ADR-10 row).

Not blocked by this decision: E4.4-a per-page locale and timezone emulation (section 2, option A).
