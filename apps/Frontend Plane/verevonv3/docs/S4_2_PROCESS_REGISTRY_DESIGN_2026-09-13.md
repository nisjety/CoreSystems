# S4.2 Durable Process Registry Design: Reattachable Background Processes Under the Space Computer Profile

## Why this doc exists

`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md` §S4.2 (lines
2193-2198) is the whole written spec for this item:

> - Add process start/read/write/signal/list under the Space computer profile.
> - Persist redacted command identity, cursor, status, TTL, run/operation
>   provenance, and cleanup state.
>
> Verification: reattach, cursor resume, worker restart, TTL, TERM→KILL,
> secret redaction, concurrent readers.

Its only dependency in the plan's own graph (line 2375, `P --> T["S4.2
Process registry"]`) is S3.3, which landed in full on 2026-09-12
(`S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md`, commits `4e1eb12f`..`1610d188`).
S4.2 is in turn the sole blocker for S4.3 (the general Watch primitive —
`SPACE_PAGE_AUDIT_2026-09-06.md:1061`), whose first adapter is process output
(plan line 2205), and through S4.3 for S4.4 and S5.3. This doc is the
follow-on design, written the way the S3.2 and S3.3 docs were: against what
the code actually does today, with every claim below traced to a file.

**What exists today, verified against the code on 2026-09-13, not assumed:**

- There is **no long-lived process machinery anywhere in execution-core.**
  Exactly one place spawns a tool child — `executor.rs:260`,
  `tokio::process::Command::output()` under `tokio::time::timeout` with
  `kill_on_drop(true)` — buffered to completion, no stdin pipe
  (`Stdio::piped()` appears nowhere in the crate), no signal API beyond
  `kill_on_drop`'s implicit SIGKILL, no `Child` handle that outlives the
  call, no `JoinHandle`/channel/`DashMap` of running processes. Its own
  timeout error string says it: *"long-running commands are not supported
  here."*
- **All six `ExecutionCore` RPCs are unary** (`execution.proto`); the only
  live-progress path in the service is `browser_events.rs` pushing unary
  `RecordOrchestrationEvent`s into session-core's per-run stream. A pull
  cursor is the natural fit; no new streaming RPC is needed.
- **sandbox-manager spawns nothing** (`capability-ownership-matrix.md:121`,
  still true) and runs zero goroutines beyond its two servers
  (`cmd/main.go:85,141`). Lease expiry is enforced lazily on read
  (`lease.go:157`); there is no sweeper.
- **The "durable process" concept exists nowhere** — not in `sandboxes.proto`,
  not in the V3 gateway (`apps/gateway/src/domains/spaces.rs`), not in the
  frontend (`src/features/spaces`). The Work tab today has `runs` and
  `schedules` sections plus an `unavailable` gap list with four codes; there
  is no `processes` section or code, and no `process` member on
  `ActivityRenderClass` (`activity-grammar.ts:45-59`). S4.2 is greenfield end
  to end.
- **`processes: "bounded_oneshot"` is signed cross-service.** execution-core
  measures it (`sandbox.rs:328`), Control binds it into the Space capability
  decision's payload digest (`user-core/internal/spaces/space_capability_decision.go:68`),
  sandbox-manager recomputes that digest (`capability_verifier.go:185`). But
  the value is a free-form string validated as non-blank only
  (`space_capability_decision.go:39-54`; `capability_verifier_test.go:16`
  even uses a third value, `"isolated"`), nothing compares it against a
  policy or a measured profile, and the `leases` table does not persist it —
  only `backend_id` survives `AcquireLease` (`migrations/0002`,
  `server.go:88-106`). The one existing template for gating a profile
  dimension is `egress` → `space:egress` permission
  (`space_capability_decision.go:137-140`, `capability_verifier.go:133-135`).

**A confirmed pre-existing bug found while researching this, fixed before
this doc was committed (same posture as the `ActivateLease` authz bug found
while designing S3.3 step 3.5.C):** `sandbox_lease.rs`'s
`SandboxManagerTokenProvider` requested `scopes: ["sandbox:write"]` only.
sandbox-manager's `authz.go` gates `GetWorkspaceManifest` on `sandbox:read`
with an exact `slices.Contains` check (no write-implies-read), and Auth Core
issues exactly the scopes requested (`plane-service-principal.ts:359`,
`scopes: requestedScopes`, narrowed from the registry allowlist — which does
list both). So S3.3 step 3.5.C.5's `ensure_hydrated_workspace` →
`get_workspace_manifest` was refused at the real interceptor, invisible to
every Rust test because none crosses the Go boundary, and
`authz_test.go:151` ("GetWorkspaceManifest missing read scope") had already
locked in exactly that refusal on the Go side. Fixed: the provider now
requests `["sandbox:read", "sandbox:write"]`. Recorded in
`apps/Model Plane/docs/decisions/ledger.md` (2026-09-13). Every read RPC this
doc adds rides on the same token, so the fix is a prerequisite, not a
side quest.

## 1. Ownership: two services, one line, deliberately not crossed

`capability-ownership-matrix.md`'s 2026-06-03 ruling (line 138) is the
constraint: **execution-core owns per-tool-call OS isolation (bwrap);
sandbox-manager owns per-session lease bookkeeping and spawns nothing.** A
process registry sits across that line, so the split has to be explicit:

| Concern | Owner | Why |
|---|---|---|
| The OS process: spawn, stdio pipes, signals, TTL timer, reaping | **execution-core** (`process_host.rs`, new) | It is the only service with bwrap, the interpreter stack, and the hydrated Space workspace on local disk. Moving spawning into sandbox-manager would give a Go container `exec.Command` over a filesystem it does not have. |
| The durable registry: metadata, output log, cursor allocation, retention, state machine, cleanup state, the human read path | **sandbox-manager** (`internal/process`, new; Postgres migration `0003`) | It already owns the lease the process belongs to, the Postgres store the lease lives in, the Control-decision verifier, and the `backend_id` pin. Every process is scoped to a lease; the registry is the lease's children. |

Consequence stated plainly, because the plan's word "durable" invites a
stronger reading: **the registry survives an execution-core restart; the
processes do not.** Every sandboxed child runs under bwrap's
`--die-with-parent` (`sandbox.rs:167`) — by design, so a crashed worker
never leaks a sandbox. Making processes outlive their host would need a
supervisor that itself outlives execution-core, i.e. the "second real
sandbox backend" the 2026-08-22 ledger entry names as the trigger for a
`SandboxBackend` trait. This doc does not build that (see Open Questions),
and it does not introduce the trait either: there is still exactly one
backend. What "worker restart" *does* mean here is the property session-core
enforces for every durable outbox it owns: after a restart, no row lies. A
process whose host died is marked `LOST`, its output up to the last
committed chunk stays readable from any cursor, and its cleanup state is
recorded — never a `RUNNING` row for a process that no longer exists.

The matrix gains a row: *"Background process registry | sandbox-manager (Go,
rows) + execution-core (Rust, host) | Postgres | … "*, and the ledger gets
the ownership decision (§8).

## 2. The durable registry (sandbox-manager, Postgres)

### 2.1 Schema — migration `0003_process_registry.{up,down}.sql`

Conventions copied from `0001`/`0002` and the entrypoint that re-applies
every `*.up.sql` on every boot (`docker-entrypoint.sh`): `TEXT` ids,
`TIMESTAMPTZ NOT NULL DEFAULT now()`, `IF NOT EXISTS` on everything, partial
indexes keyed to the state column, matching `.down.sql`. The database is
session-core's `session_core` (shared, `docker-compose.production.yml:107`),
so the tables are prefixed `sandbox_` to be unambiguous next to `events`,
`runs`, `approvals`.

```sql
-- Lease-side gate. Fail-closed default: every lease that exists today was
-- acquired before Control could grant space:processes, and must read as not
-- permitted rather than be grandfathered in.
ALTER TABLE leases
    ADD COLUMN IF NOT EXISTS processes_permitted BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS sandbox_processes (
    id                 TEXT PRIMARY KEY,           -- ULID, minted by the host (mp_ids::new_ulid)
    org_id             TEXT NOT NULL,
    space_id           TEXT NOT NULL,
    lease_id           TEXT NOT NULL,
    backend_id         TEXT NOT NULL,              -- host, == leases.backend_id
    host_epoch         TEXT NOT NULL,              -- execution-core boot id; the write fence
    -- provenance
    run_id             TEXT NOT NULL,
    step_id            TEXT NOT NULL DEFAULT '',
    subject_id         TEXT NOT NULL,              -- the Space member whose decision the lease carries
    -- redacted command identity
    command_redacted   JSONB NOT NULL,             -- {"program": "...", "args": ["..."]} after redact.Command
    command_digest     TEXT NOT NULL,              -- sha256 over the UNredacted argv; lets a caller recognize
                                                   -- "the same command" without the registry storing it
    -- lifecycle
    state              SMALLINT NOT NULL,          -- ProcessState wire values, like leases.state
    exit_code          INTEGER,
    end_reason         TEXT,                       -- bounded vocabulary, CHECKed below
    signal_requested   SMALLINT NOT NULL DEFAULT 0,-- 0 none, 1 term, 2 kill
    term_requested_at  TIMESTAMPTZ,
    cleanup_state      SMALLINT NOT NULL DEFAULT 1,-- 1 pending, 2 done
    ttl_seconds        INTEGER NOT NULL,
    expires_at         TIMESTAMPTZ NOT NULL,
    started_at         TIMESTAMPTZ,
    ended_at           TIMESTAMPTZ,
    last_heartbeat_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- output bookkeeping (the log itself is the table below)
    next_seq           BIGINT NOT NULL DEFAULT 1,  -- informational mirror of the host's counter
    retained_from_seq  BIGINT NOT NULL DEFAULT 1,  -- first seq still present after retention trimming
    retained_bytes     BIGINT NOT NULL DEFAULT 0,
    dropped_bytes      BIGINT NOT NULL DEFAULT 0,
    stdin_bytes        BIGINT NOT NULL DEFAULT 0,  -- a count only; stdin content is never stored
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (state BETWEEN 1 AND 6),
    CHECK (signal_requested BETWEEN 0 AND 2),
    CHECK (cleanup_state BETWEEN 1 AND 2),
    CHECK (end_reason IS NULL OR end_reason IN (
        'exited', 'signaled', 'ttl_expired', 'lease_released',
        'host_lost', 'spawn_failed', 'output_budget_exhausted')),
    -- state-shape invariant, the migration-0015 pattern: a terminal row always
    -- says how it ended; a live row never does.
    CHECK (
        (state IN (1, 2) AND ended_at IS NULL AND end_reason IS NULL)
     OR (state IN (3, 4, 5, 6) AND ended_at IS NOT NULL AND end_reason IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS sandbox_processes_space_idx
    ON sandbox_processes (org_id, space_id, created_at DESC);
CREATE INDEX IF NOT EXISTS sandbox_processes_lease_idx
    ON sandbox_processes (lease_id);
-- The two hot paths of the sweeper and the boot-time reconcile, narrowed to
-- live rows only.
CREATE INDEX IF NOT EXISTS sandbox_processes_live_idx
    ON sandbox_processes (backend_id, host_epoch) WHERE state IN (1, 2);
CREATE INDEX IF NOT EXISTS sandbox_processes_heartbeat_idx
    ON sandbox_processes (last_heartbeat_at) WHERE state IN (1, 2);

CREATE TABLE IF NOT EXISTS sandbox_process_output (
    process_id         TEXT NOT NULL REFERENCES sandbox_processes(id) ON DELETE CASCADE,
    seq                BIGINT NOT NULL,            -- host-assigned, monotonic per process
    stream             SMALLINT NOT NULL,          -- 1 stdout, 2 stderr, 3 system marker
    content            BYTEA NOT NULL,             -- already redacted by the host
    ends_with_newline  BOOLEAN NOT NULL,           -- S4.3's "partial line" adapter needs this
    captured_at        TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (process_id, seq),
    CHECK (stream BETWEEN 1 AND 3)
);
```

`state` stores the proto enum's numeric wire values, exactly as `leases.state`
does (`0002`'s own comment): `1 STARTING, 2 RUNNING, 3 EXITED, 4 KILLED,
5 LOST, 6 EXPIRED`. `STARTING` exists so the registry can reserve the slot
(count limits, TTL bound) *before* the host spawns; a spawn failure moves it
straight to `EXITED` with `end_reason = 'spawn_failed'`.

### 2.2 Cursor: host-assigned `seq`, idempotent appends, integer on the wire

The precedents were weighed rather than picked by habit (all in
`apps/Model Plane`): session-core's `events.step_ordinal` is a `MAX+1`
trigger whose own migration says the single-writer assumption is what makes
it safe (`0002_events_and_ordinals.sql:11-15`) and whose consumers retry on
unique violations (`compaction.rs:7-9`); `messages.sequence` is a global
`GENERATED ALWAYS AS IDENTITY`, race-free but non-idempotent across a retried
batch; model-gateway's `stream_buffer.rs` is a plain `u64` owned by the single
producer with `replay_after(key, Option<u64>)` — the cleanest reader API in
the repo; `mp-eventlog::Cursor` is an opaque base64 codec with zero
consumers, not adopted.

The choice: **the host assigns `seq`** (it is the single writer per process,
by construction — the row is pinned to `backend_id` + `host_epoch` and every
write is fenced on both), and the registry inserts with
`ON CONFLICT (process_id, seq) DO NOTHING`. A batch retried after an
ambiguous transport failure lands the same rows under the same seqs and is a
no-op — idempotency for free, no batch-id table. `sandbox_processes.next_seq`
is advanced with `GREATEST(next_seq, $max_seq + 1)` as an informational
mirror. On the wire the cursor is a plain integer, the same shape as SSE
`Last-Event-Id` and `stream_buffer.rs`'s `after_seq`. A reader passes
`after_seq` and gets back `chunks`, `next_cursor`, and the process `state`
— *"a valid, fully-acknowledged stream is not a 404 just because it has
nothing left to replay"* (`stream_buffer.rs:296-306`) holds here too: a
terminal process with nothing after the cursor returns an empty page and its
terminal state, never `NotFound`.

The write fence, the one place a stale host is stopped:

```sql
-- AppendProcessOutput: every chunk insert runs inside one transaction after
-- this row lock succeeds; zero rows means "not yours any more" and the host
-- must stop writing and drop its local handle.
SELECT id FROM sandbox_processes
 WHERE id = $1 AND backend_id = $2 AND host_epoch = $3 AND state IN (1, 2)
   FOR UPDATE;
```

### 2.3 Retention: head + tail, gaps visible, never silent

A background process can emit unboundedly; the plan's own "head + tail
tool-result cap" row (line 1239) and QM's `pi-tools.ts:82-94` pattern say
what to keep: the beginning (the command's own banner and first errors) and
the most recent tail. Per process: `PROCESS_OUTPUT_HEAD_BYTES` (default
64 KiB) is never trimmed; beyond that a rolling window of
`PROCESS_OUTPUT_RETAIN_BYTES` (default 2 MiB) is kept and the oldest middle
chunks are deleted as new ones land, in the same transaction as the append.
`retained_from_seq` advances and `dropped_bytes` accumulates; a
`ReadProcessOutput` whose `after_seq` falls before `retained_from_seq`
returns `gap_before = true` with the first retained seq — the exact
"eviction the resuming reader must detect" case `stream_buffer.rs`'s
`delta_cap_evicts_oldest` test pins. A process that exceeds a hard total
(`PROCESS_OUTPUT_MAX_TOTAL_BYTES`, default 64 MiB counting dropped bytes) is
terminated by the host with `end_reason = 'output_budget_exhausted'` — a
runaway logger is a resource bug, and the head+tail window already preserves
what a human needs to see why.

The system-marker stream (`stream = 3`) carries exactly these registry-authored
lines and no content: `process started`, `SIGTERM requested`, `SIGKILL after
grace`, `output trimmed: N bytes dropped`, `TTL expired`, `host lost:
execution-core restarted`. Readers render them; the model sees them as
ordinary lines.

### 2.4 Redaction: promote the existing Go port, add the one missing rule

`internal/snapshot/exclude.go:24-60` is already a faithful, tested Go port of
execution-core's `scrub.rs` pattern set (Bearer/Basic, JWTs, `sk-*`, AWS,
GitHub/Slack/Google keys, PEM blocks, URI userinfo, inline `KEY=value`).
It is unexported and in the wrong package for a second consumer, so it
moves to a sibling `internal/redact` with `func String(string) string` and
gains `func Command(program string, args []string) (string, []string)`;
`snapshot.ExcludeCredentials` keeps calling it, unchanged in behavior. The
genuinely new rule — no pattern in either language handles a *separate*
`--flag value` pair (`--token abc123`, `-p secret`) because the inline
pattern keys on `[:=]` — is a small positional allowlist over `argv[i]`/
`argv[i+1]` (`--token`, `--password`, `--secret`, `--api-key`,
`--access-key`, `--auth`, `-p`, and their `=`-less forms) applied *on top
of* `String`, never instead of it. The unredacted argv exists in execution-core
memory only long enough to spawn; `command_digest` is computed there and
the registry never sees the plaintext.

Output chunks are redacted **by the host, at line boundaries, before
persistence** (§3.3) — the registry stores them as received and re-scrubs
nothing, so there is one scrubber per byte, not two disagreeing ones.

### 2.5 State machine

```
STARTING ──spawned──▶ RUNNING ──exit──▶ EXITED   (end_reason exited | signaled | spawn_failed)
    │                    │──TTL──▶ EXPIRED
    │                    │──kill──▶ KILLED   (after TERM grace, or direct kill)
    │                    │──host heartbeat stale / boot reconcile──▶ LOST
    └──spawn failed──▶ EXITED(spawn_failed)
```

Every transition is `UPDATE ... WHERE id = $1 AND state = $old` with the
fence columns, the `lease.go:180-195` shape; a zero-row result is a lost
race, not an error. Terminal rows are kept, not deleted (`ReleaseScoped`'s
own reasoning: a second terminalization is an idempotent no-op, and a reader
must still find the row). The store exposes the same two-lookup split
`lease.go:237-244` established — `lookupLive` (operational grants: append,
heartbeat, state changes; excludes terminal rows) and `lookupAny` (reads;
descriptive; a `KILLED` process's tail is still readable). `cleanup_state`
records whether the host has finished tearing down the process's scratch
directory and closed its pipes; `LOST` rows are marked `done` at reconcile
time, because `--die-with-parent` has already guaranteed there is nothing
left to clean.

Lease release: `ReleaseLease` on a lease with live processes marks each
`KILLED` with `end_reason = 'lease_released'` — but only the registry side.
The host learns of it through its own release path (`release_sandbox_lease_if_any`
already runs at the two points a run's end is observable) and kills the
children first; a lease released by TTL from under a still-live host is
caught by the sweeper via the heartbeat staleness that follows, never by a
registry row pretending the process stopped.

### 2.6 The first background goroutine in sandbox-manager

Shape copied from capability-core's `cron/sweeper.go:55-78` (ticker, immediate
first sweep on start so a restart re-scans, errors logged not fatal,
`PROCESS_SWEEPER_ENABLED=false` opt-out), body deliberately a single
idempotent statement so multiple replicas need no claim:

```sql
UPDATE sandbox_processes
   SET state = 5, end_reason = 'host_lost', ended_at = now(),
       cleanup_state = 2, updated_at = now()
 WHERE state IN (1, 2)
   AND last_heartbeat_at < now() - ($1::bigint * interval '1 second');
```

`$1` is `PROCESS_HEARTBEAT_STALE_SECS` (default 60; the host heartbeats every
15 s). This is the only thing the sweeper does. TTL is enforced by the host
(§3.4), not here — the registry only needs to notice a host that stopped
talking.

## 3. The process host (execution-core)

New module `process_host.rs`, wired into `ExecutionService` the way
`sandbox_manager_client`/`cas_client` were: constructed in `grpc.rs::serve`,
threaded through the `SandboxLeaseContext` bundle so **one** new field, not
five parameters, crosses the dispatch seams B.2 established.

### 3.1 Enablement and truthful measurement

`EXECUTION_CORE_PROCESS_HOST=enabled` (default absent → disabled) sets a
`OnceLock<bool>` at startup, exactly the `sandbox::is_supported()` shape.
`sandbox::capability_profile()` reports `processes: "background_registry"`
iff that flag is set **and** `is_supported()` (bwrap present) — otherwise
`"bounded_oneshot"`, byte-for-byte the current profile and digest. Every
existing deployment is unchanged until an operator flips the flag; this is
the same zero-behavior-change-by-default posture every S3.x slice held.
`sandbox.rs`'s `capability_profile_never_claims_durable_or_credentialed_workspace`
test splits: `persistence`/`backup`/`credential_mode`/`egress` stay fixed
assertions; `processes` is asserted to be `background_registry` when and only
when the host reports enabled, and `bounded_oneshot` otherwise — the
"advertise truthfully" invariant made executable instead of relaxed.

### 3.2 Spawn

Space-scoped only (`space_id` non-empty, a lease exists, `processes_permitted`
on the cached `SandboxLease` — §4). The command is staged like
`code_interpreter` does — a program file, not `-c`, for real tracebacks and
no argv-size cliff (`code_interpreter.rs:532-537`) — into a per-process
scratch directory **outside** the workspace root
(`<tmp>/verevon-space-processes/<space_slug>/<lease_slug>/<process_id>/`,
reusing `path_slug`), readable inside the sandbox through the `--ro-bind / /`
baseline. cwd is the lease's hydrated workspace (`sandbox_lease::workspace_dir`),
policy `WorkspaceWrite { writable_roots: [workspace], network: Disabled }`,
env the exact `child_env` allowlist `code_interpreter.rs:420-442` already
uses (`PATH`, `HOME`=workspace, `TMPDIR`/`MPLCONFIGDIR` under the workspace's
reserved dirs, `MPLBACKEND=Agg`, `PYTHONDONTWRITEBYTECODE=1`, optional `LANG`).
`executor::require_requested_isolation` runs before the spawn, unchanged:
a host that cannot bwrap does not start a background process unsandboxed —
and cannot advertise `background_registry` in the first place (§3.1).

The spawn itself is `tokio::process::Command` with `stdin/stdout/stderr =
Stdio::piped()`, `kill_on_drop(true)`, through `sandbox::wrap_command_with`
so the argv is the existing one — `--die-with-parent`, `--unshare-pid`,
`--new-session` and all. Nothing about the argv changes. The returned
`Child` is what outlives the tool call: it lives in
`ProcessHost { processes: DashMap<process_id, ManagedProcess> }` alongside a
stdin writer handle, the two reader `JoinHandle`s, the TTL/grace timer
handle, and the `host_epoch`.

Order of operations, so the registry never learns of a process that
exists only in memory: `RegisterProcess` (row `STARTING`, limits and TTL
bound checked there) → spawn → `UpdateProcessState(started)` → readers
start. A spawn failure reports `exited(spawn_failed)`; a `RegisterProcess`
refusal (`processes_permitted` false, limits hit, lease not `ACTIVE`) is
returned to the model as the tool error, and nothing was spawned.

### 3.3 Output: line-buffered, scrubbed, batched, appended

Per stream, a reader task reads into a buffer and cuts chunks at **line
boundaries** — the boundary rule is not aesthetic: `executor.rs:91-92` states
why scrubbing must happen before any split (*"a secret in half … no longer
matches the scrub patterns"*), and a fixed-size chunker would reintroduce
exactly that. A chunk is flushed when it holds ≥ 16 KiB of complete lines or
250 ms have passed since the first unflushed byte; a single line longer than
64 KiB is flushed anyway with `ends_with_newline = false` (the residual risk
is stated, not hidden). Each flushed chunk is `scrub::scrub_string`'d, given
the next `seq` from the process's local counter, and batched into one
`AppendProcessOutput` call with whatever the other stream has ready. An
append that returns the fence's "not yours" error stops both readers and
drops the handle — the registry has moved on (reconciled or released), and
so must the host.

An `AppendProcessOutput` with zero chunks is a heartbeat; the host sends
one every 15 s per live process that has emitted nothing, so silence is
distinguishable from death.

### 3.4 TTL, TERM→KILL, and the two mechanisms

**TTL** is host-enforced: `ttl_secs` (default 30 min, max `min(caller, 1 h,
lease remaining)` — a process never outlives its lease's own `expires_at`)
arms a tokio timer at spawn. Expiry runs the same escalation as a `term`
request and records `end_reason = 'ttl_expired'` → `EXPIRED`.

**TERM→KILL** is new work — the only escalation with a grace period in the
whole repo is auth-core's own shutdown watchdog (`auth-core/src/main.ts:249-291`,
*"a hard exit we choose is strictly better than a SIGKILL we do not"*), and
Model Plane child processes have never been signalled at all. The contract:

1. `signal = term`: deliver SIGTERM to the sandboxed *command* (best effort),
   record `signal_requested = 1`, `term_requested_at`, arm a grace timer
   (`PROCESS_TERM_GRACE_SECS`, default 10).
2. Grace elapsed and still running, or `signal = kill`: `Child::kill()` on
   the tokio handle — SIGKILL to the outer bwrap monitor, which
   `--die-with-parent`'s documented PDEATHSIG chain turns into the death of
   the sandbox init and every process inside it. **This half is guaranteed**
   by a bwrap feature the argv already uses.
3. Exit is observed by the readers' EOF + `Child::wait()`; the row becomes
   `KILLED` (`end_reason = 'signaled'`) or `EXITED` if the command beat the
   grace timer.

The best-effort half needs the command's host PID, and there is a real
uncertainty to state rather than paper over: with `--unshare-pid` bwrap forks
a sandbox init (PID 1 inside) that forks the command, and a signal sent from
outside to a namespace's init is delivered only if init handles it
(`pid_namespaces(7)`); whether bwrap's init forwards SIGTERM is not something
this Windows checkout can verify. So the mechanism is resolved
**at implementation time on the Linux runtime image**, in this order of
preference: (a) walk `/proc/<monitor>/task/<monitor>/children` twice
(monitor → init → command; `CONFIG_PROC_CHILDREN` is on in Debian kernels,
and host procfs is readable — no fd passing, no `pre_exec`) and
`nix::sys::signal::kill(command_pid, SIGTERM)` — one small `cfg(unix)`
dependency, preferred over spawning `/bin/kill` to signal a process; (b) if
that resolution proves unreliable, `--json-status-fd` for the init PID plus
one `children` hop. Either way the Linux proof is a new probe in
`scripts/verify-sandbox-isolation.sh` (the script that already verifies
bwrap isolation separately from the cross-platform unit tests): start a
`sh` loop under the real argv, request `term`, assert the trap ran and the
row is `KILLED` within grace; request `kill`, assert no process remains in
the namespace. The escalation state machine itself is tested cross-platform
through the passthrough path, exactly as `executor.rs`'s spawn→capture→scrub
tests are.

### 3.5 Reconcile at boot

`process_host::boot()` mints `host_epoch = mp_ids::new_ulid()` and calls
`ReconcileProcesses(backend_id, host_epoch)` once before the gRPC listener
binds (so `/readyz` stays false until the registry is truthful, matching
the bind-aware readiness rule). The registry marks every `STARTING|RUNNING`
row with this `backend_id` and a *different* epoch as `LOST` /
`host_lost` / `cleanup_state = done`, appends the `host lost` system marker,
and returns the count for the boot log. A registry that is unreachable at
boot is logged and retried by the sweeper's staleness rule (§2.6) — the
rows still cannot lie for longer than `PROCESS_HEARTBEAT_STALE_SECS`.

## 4. Authority: what "under the Space computer profile" means in code

Four layers, each fail-closed, each reusing something that exists.

**Control (user-core) — the entitlement and the permission.** Migration
`028_space_process_registry_effect_policy.up.sql` adds
`process_registry_entitled BOOLEAN NOT NULL DEFAULT FALSE` to
`space_effect_policies`, the exact deny-by-default shape of `027`'s
`sandbox_capability_entitled`; `PersonalThreadDecisionEvidence` and the
repository read gain the field. `IssueSpaceCapabilityDecision`
(`space_capability_decision.go:137-140`) extends the `egress` template:

```go
if intent.Processes == "background_registry" && evidence.ProcessRegistryEntitled {
    permissions = append(permissions, "space:processes")
}
```

Deliberately *not* a refusal when the backend claims `background_registry`
and the Space is not entitled — unlike `egress`, where an egress-capable
backend must not get a lease for a non-entitled Space at all, a
process-capable backend is still a perfectly good one-shot `code_interpreter`
substrate. The decision is signed either way; the permission is simply
absent, and absent means refused at the next two layers. `Validate()` also
gains the first enum check this claim has ever had: `processes ∈
{bounded_oneshot, background_registry}` — closing, for this one dimension,
the S3.2 doc's never-implemented "policy allowlist" (§1 lines 112-117 of
that doc). Tests: not entitled → signed, no permission; entitled +
`bounded_oneshot` → no permission; entitled + `background_registry` →
permission; `Processes: "isolated"` → rejected.

**sandbox-manager — persist the answer on the lease.** `Verify` returns the
decision's `Permissions` alongside the claims (a `VerifiedCapability{Claims,
Permissions}` struct; one refactor, the func-field injection pattern on
`Server` unchanged). `AcquireLease` stores
`processes_permitted = claims.Processes == "background_registry" &&
hasPermission(perms, "space:processes")` on the new column. `RegisterProcess`
requires `processes_permitted`, `state == ACTIVE` (a `SCRATCH` lease has no
hydrated workspace to run in), the `backend_id` pin, and the live-count
limits (`PROCESS_MAX_LIVE_PER_LEASE` default 4, `PROCESS_MAX_LIVE_PER_SPACE`
default 8). This is the authoritative gate; today the `leases` row cannot
answer "was this lease authorized for background processes?" at all, which
is why the column is the design and not a re-presented decision.

**execution-core — the fast local refusal and the Space check for reads.**
`state::SandboxLease` gains `processes_permitted: bool` from the claims
`ensure_sandbox_lease` currently discards; `process_start` refuses locally
before any RPC when it is false. For `process_read`/`process_list`/
`process_stdin`/`process_signal`, execution-core enforces that the target
process's `space_id` (from `GetProcess`) equals the run's verified Space —
the boundary for the *model* is execution-core, the same way it is for
`code_interpreter`'s Space-scoped bearer. sandbox-manager scopes service
principals to the token's `org_id` exactly as the lease RPC family already
does (`OwnerFilter` is `""` for services; execution-core can already
`Activate`/`Snapshot`/`Release`/`Promote` any lease in the org) — this doc
adds no new trust to that principal, and names the exposure honestly in
Open Questions rather than widening or silently narrowing it.

**Credentials per RPC**, following the ledger's 2026-09-11 ruling
verbatim: a background process outlives the 300 s user bearer, so **every
host↔registry RPC rides execution-core's own service token**
(`SandboxManagerTokenProvider`, now correctly minted with both scopes).
The user-bound bearer appears exactly where it does today — `AcquireLease`
— and that is where the user-subject-bound decision that carries
`space:processes` enters. Human reads (§7) use the V3-minted user bearer
plus a Control read decision, not a service token.

**ZDR.** A background process's output is content persisted durably in
Postgres by construction. `process_start` is refused for a ZDR caller with
`failed_precondition` ("background processes are unavailable under ZDR"),
the same gate `workspace_promote.rs` applies to durable promotion; no ZDR
posture ever reaches `RegisterProcess`. The registry's ZDR justification
against migration `0015`'s "no content-derived column" rule is that: rows
exist only for persistent-posture callers, argv is redacted before it
leaves execution-core, stdin content is never stored, and output is
scrubbed at the line boundary before persistence.

## 5. RPC surface (sandbox-manager, `sandboxes.proto`)

Seven RPCs, each added to `authz.go`'s method switch **and** to
`authz_test.go`'s interceptor table in the same commit — the `ActivateLease`
bug is the reason that rule is written down (the switch's own comment).

| RPC | Scope | Caller | Purpose |
|---|---|---|---|
| `RegisterProcess(lease_id, backend_id, host_epoch, run_id, step_id, subject_id, command_redacted, command_digest, ttl_seconds)` → `(process_id accepted, expires_at)` | write | host (service) | reserve the row in `STARTING`; all gates of §4 |
| `UpdateProcessState(process_id, backend_id, host_epoch, oneof { started, signal_requested{term\|kill}, exited{exit_code, end_reason} })` | write | host | the three host-driven transitions |
| `AppendProcessOutput(process_id, backend_id, host_epoch, repeated chunk{seq, stream, content, ends_with_newline, captured_at}, stdin_bytes_delta)` → `(next_seq, retained_from_seq)` | write | host | output + heartbeat (zero chunks) + stdin count |
| `ReconcileProcesses(backend_id, host_epoch)` → `(lost_count)` | write | host, at boot | mark this backend's stale-epoch live rows `LOST` |
| `GetProcess(process_id)` → `Process` | read | host (service) / human (user + read decision) | metadata only, no output |
| `ListProcesses(space_id, include_terminal, limit, after_process_id)` → `(repeated Process, has_more)` | read | same | ULID-descending page, `after_process_id` cursor — the `ListRuns` shape |
| `ReadProcessOutput(process_id, after_seq, max_bytes)` → `(repeated chunk, next_cursor, gap_before, retained_from_seq, state, exit_code, has_more)` | read | same | the cursor read; `max_bytes` capped at 256 KiB server-side |

`enum ProcessState { PROCESS_STATE_UNSPECIFIED=0; STARTING=1; RUNNING=2;
EXITED=3; KILLED=4; LOST=5; EXPIRED=6; }` and a `Process` message mirroring
§2.1's columns minus the bookkeeping the host does not need. All new
messages start at field 1; no existing message changes. `buf generate`
touches ~90 unrelated files — isolate to the `sandboxes`-named outputs by
`git status --short` before/after, as every prior proto change did.
`internal/server/types.go` gains the alias lines; `memory_store.go` gains a
`MemoryProcessStore` for `server_test.go`'s fast path, mirroring the SQL
semantics by hand as `MemoryWorkspaceStore.Promote` does.

User-principal reads (`GetProcess`/`ListProcesses`/`ReadProcessOutput` from a
`principal_type == "user"` caller) additionally require
`space_read_decision_ref` + `space_read_decision_token` in the request —
§7 explains where they come from.

## 6. Tools: the `process_*` family and `cap.process.background`

Five dispatch names, Space-scoped only, all bound to **one** new capability
row `cap.process.background` in `capability_policy::trusted_capability_id`
(`capability_policy.rs:738-746`; a model-supplied name selects a tool, never
forges its capability):

| Tool | Input | Output | Note |
|---|---|---|---|
| `process_start` | `{language: python\|sh, code, args?: [string], ttl_secs?, stdin?: bool}` | `{process_id, state, expires_at}` | same code-body shape as `code_interpreter` — a code body, never a named host command, which is what keeps `cap.command.sandbox` low-risk (`0010`'s own rationale) |
| `process_read` | `{process_id, after_seq?, max_bytes?}` | `{state, exit_code?, chunks: [{seq, stream, text, ends_with_newline}], next_cursor, gap_before}` | the model's own cursor; a terminal process still answers |
| `process_stdin` | `{process_id, data, close?: bool}` | `{bytes_written}` | named `stdin`, not `write`: `permission::is_risky_tool`'s substring list (`permission/mod.rs:229-260`) matches `write` and would route it to `ask` on name alone |
| `process_signal` | `{process_id, signal: term\|kill}` | `{state}` | §3.4 |
| `process_list` | `{include_finished?: bool}` | `{processes: [...]}` | the run's Space only |

`process_stdin` and `process_signal` require the process to be **local**
(`backend_id == mine && host_epoch == mine && present in ProcessHost`);
otherwise `failed_precondition process_not_local`. A run whose lease landed
on another backend can read and list, not control — cross-host control is
an explicit non-goal (Open Questions), the same honesty as the lease
backend-pin's `FailedPrecondition`.

**The capability row**, capability-core migration
`0015_background_process_capability.{up,down}.sql`, mirrors `0010` line for
line with `dispatch_name = 'process_start'` (the family's entry point) and an
`isolation` blob that states the *differences* from `cap.command.sandbox`:
`workspace = 'lease_hydrated_space_workspace'`, `lifetime = 'ttl_bounded_background'`,
`output = 'redacted_durable_head_tail'`, plus the unchanged `network:
disabled`, `rootfs: read_only`, `host_command_execution: false`. Risk level
**`low`**, recommended with its reasoning exposed rather than asserted: the
residual blast radius over `cap.command.sandbox` is duration (≤ 1 h, ≤ lease
TTL) and concurrency (count-bounded), both operator-tunable, inside the same
no-egress read-only-root sandbox writing only its own Space's workspace;
sending every long computation through the `ask` branch would recreate the
approval-fatigue argument `0010` made against doing that to a calculator.
If product wants `ask` for the first slice, it is one field in the migration
(Open Questions). Registered in `availability.go`'s
`genericGlobalHealthCapabilityIDs` allowlist (a list that "stays a list
precisely because widening it is a decision"), attested by `health_attest.rs`
only when the host is enabled *and* the sandbox+interpreter probes pass —
starts `unavailable` / `health_not_attested` like every row before it.

**Offering.** `runtime_loop/agent.rs::offered_tool_defs()` offers the family
only when the run is Space-scoped and the host is enabled; the authoritative
gate stays `process_start` (§4). The chat loop (model-gateway's
`tool_loop.rs` catalog, `ExecuteStep`'s literal `tool_name == "code_interpreter"`
bearer gate at `grpc.rs:787-789`, SSE rendering) is **not** in this slice:
`RunAgent` — the governed loop, which already demands the sandbox bearer
whenever `space_id` is set — is where background work belongs first. A
contract test asserts that every name in `trusted_capability_id`'s process
arm is offered and vice versa, because `agent.rs:2446-2461` records how
`save_memory`/`recall_memory` shipped dead when tests injected an `Allow`
double that never consulted the mapping.

## 7. The human read path — the S4.5 hook, specified here, built last

The Work tab reaches Model Plane only through model-gateway HTTP
(`GET /v1/runs`, `GET /v1/cron`), always under a Control-signed
`model.thread.read` decision that the *owning* service verifies
(session-core joins runs to threads under the decision's audience ceiling,
`run_service_grpc.rs:697-712`) — "reuse an existing decision rather than mint
a permission, and let the owner verify it." Processes copy that exactly:

- **Control**: `POST /api/v1/internal/spaces/thread-read-decision` gains an
  optional `service_audience ∈ {model-plane-session-core (default),
  model-plane-sandbox-manager}`; nothing else about the decision changes.
  The V3 gateway already obtains this decision for `/work` (`spaces.rs:2383`);
  it requests the second audience alongside.
- **sandbox-manager**: `authz.SpaceReadVerifier` — a Go port of session-core's
  `verify_thread_read_space_decision` (`grpc.rs:1727-1831`: version/key-id/
  signature envelope it already parses, `action_id == model.thread.read`,
  `thread:read` present, `thread:append`/`thread:create` absent, `space_ref`
  == the process's `space_id`, `subject_id` == the principal's `ActorID`,
  expiry). A process row has no audience snapshot of its own, so the lease's
  decision revisions are stored on the process at `RegisterProcess`
  (`recipient_audience_revision` from the lease's verified claims — see Open
  Questions) and the read decision's revision must be ≥ it, the same ceiling
  runs get.
- **model-gateway**: `GET /v1/processes?space_ref=&space_read_decision_ref=&
  space_read_decision_token=` and `GET /v1/processes/{id}/output?after_seq=`
  → sandbox-manager `ListProcesses`/`ReadProcessOutput`, forwarding the
  `x-sandbox-authorization` user bearer (already verified as
  `VerifiedSandboxBearer`) as the upstream `authorization`. The V3 gateway
  mints that bearer **per `/work` request** with
  `get_model_service_token(.., ModelServiceAudience::SandboxManager)` — the
  same way it mints `aud=capability-core` for `/v1/cron` — because the
  chat-path `sandbox_token` helper only fires inside a Control-injected turn
  (`is_space_scoped_turn`, `chat/shared.rs:205-215`), and the Work tab is not
  a turn. This is the
  **first real caller of the `SandboxManagerClient` model-gateway has
  constructed and never used since `state.rs:122`** — the S3.3 doc's §3.5
  amendment flagged it; S4.2 is what finally gives it a job. URL-safe
  `space_ref` check as `/v1/cron` does (`http_routes.rs:5092-5101`).
- **V3 gateway `/work`**: a third upstream, following the `schedules`
  precedent (missing token degrades to a gap row, `spaces.rs:2475-2479`, not
  the `runs` precedent's 503): `"processes": [...]` in the envelope
  (`spaces.rs:2482-2496`), gap codes `processes_upstream_unavailable`,
  `processes_session_unavailable`, `processes_read_not_authorized`; each row
  carries `space_ref` so it can link (run rows today cannot — agent-verified,
  `run_detail_value` has no Space field). Go `nil`→`null` guard as for
  schedules.
- **Frontend**: `SpaceWork.processes`, a `workRows(response.processes)` line
  in `getSpaceWork`, `sectionLabel`/`gapReason` cases in `SpaceWorkPanel.tsx`
  (else a new section renders the literal word `processes:` inside Norwegian
  copy — verified behavior), `'process'` on `ActivityRenderClass`, an
  `activityFromProcess` adapter (`RUNNING` → live/pending, `LOST`/`KILLED` →
  attention, `EXITED` code 0 → success, else failure; `expires_at` → `at`).
  Output tailing needs a second component — `SpaceActivityItem.detail` is
  the only free-text slot and is not a byte cursor — and `SpaceWorkPanel`
  does not poll (one-shot `createResource`), so a live list needs
  `space-live-work.ts`'s publish/subscribe store or a refetch timer. Controls
  (signal/stdin from the UI) need new `ActionDescriptor`s in
  `src/shared/actions` per `CLAUDE.md`; the audit's own framing applies —
  *"writes against runs the room may not own, which is a separate authority
  question from reading them"* (`SPACE_PAGE_AUDIT_2026-09-06.md:797-799`) —
  so the first slice ships **read-only** in the room.

## 8. File-by-file change list

**Model Plane — sandbox-manager (Go):**
- NEW `migrations/0003_process_registry.{up,down}.sql` (§2.1); add to the
  hard-coded migration lists in `internal/lease/lease_integration_test.go`
  and `internal/workspace/store_integration_test.go`
- NEW `internal/redact/redact.go` (+ `_test.go`) — `String`, `Command`; MODIFY
  `internal/snapshot/exclude.go` to call it
- NEW `internal/process/store.go` (+ unit stub tests, + `//go:build integration`
  tests), `nowFn`/`randFn` injected like `lease.Store`
- NEW `internal/process/sweeper.go` (§2.6); MODIFY `cmd/main.go` to start it
  and construct the store on the same `pgxpool.Pool`
- MODIFY `internal/authz/authz.go` — seven method cases; `authz_test.go` —
  seven interceptor rows; `capability_verifier.go` — return `Permissions`;
  NEW `internal/authz/space_read_verifier.go` (§7)
- MODIFY `internal/lease/lease.go` — `processes_permitted` on `Create`/scan;
  `ReleaseScoped` marks live children (§2.5)
- MODIFY `internal/server/{server.go,store.go,types.go,memory_store.go}` —
  handlers, `ProcessStore` interface, aliases, `MemoryProcessStore`
- MODIFY `internal/telemetry/metrics.go` — `ProcessDecisionsTotal`

**Model Plane — proto:** MODIFY `proto/model_plane/v1/sandboxes.proto` (§5).

**Model Plane — capability-core (Go):** NEW
`migrations/0015_background_process_capability.{up,down}.sql`; MODIFY
`internal/api/availability.go` allowlist; MODIFY `internal/registry/registry.go`
static seed.

**Model Plane — execution-core (Rust):**
- NEW `src/process_host.rs` (§3) — `ProcessHost`, `ManagedProcess`,
  spawn/readers/stdin/signal/TTL/reconcile; `cfg(unix)` signal delivery
- MODIFY `src/sandbox_manager_client.rs` — seven new methods
- MODIFY `src/sandbox.rs` — measured `processes`; test split (§3.1)
- MODIFY `src/state.rs` — `SandboxLease.processes_permitted`
- MODIFY `src/sandbox_lease.rs` — carry the claims' answer; kill children
  before release; `ProcessHost` on `SandboxLeaseContext`
- MODIFY `src/capability_policy.rs` — `trusted_capability_id` process arm
- MODIFY `src/health_attest.rs` — `PROCESS_CAPABILITY` attestation; the
  cross-language allowlist test gains the id
- MODIFY `src/runtime_loop/{mod.rs,agent.rs}` — dispatch arms,
  `offered_tool_defs`, contract test
- MODIFY `src/grpc.rs` — construct the host, `boot()` before bind
- MODIFY `Cargo.toml` — `nix` (`signal`, `process`), `cfg(unix)`
- MODIFY `deploy/docker-compose.yml` — `EXECUTION_CORE_PROCESS_HOST` documented,
  left disabled; `scripts/verify-sandbox-isolation.sh` — TERM/KILL probe

**Control Plane — user-core (Go):** NEW
`migrations/028_space_process_registry_effect_policy.up.sql`; MODIFY
`internal/spaces/{authority.go,personal_thread_decision.go,repository.go,
space_capability_decision.go}` (§4); MODIFY the `thread-read-decision`
handler for `service_audience` (§7).

**Model Plane — model-gateway (Rust):** MODIFY `src/http_routes.rs` — the two
`/v1/processes` routes (§7), first use of `state.sandbox_client`.

**Frontend Plane — V3 gateway + app:** MODIFY `apps/gateway/src/domains/spaces.rs`,
`src/shared/api/spaces-client.ts`, `src/features/spaces/{SpaceWorkPanel.tsx,
lib/activity-grammar.ts}` (§7).

**Docs:** this doc; `apps/Model Plane/docs/decisions/ledger.md` (ownership
decision, the scope bug, the TERM→KILL mechanism once verified);
`docs/capability-ownership-matrix.md` new row; `SPACE_PAGE_AUDIT_2026-09-06.md`
§12's "Monitors and delivery rows remain absent" once §7 lands.

## 9. Test plan — the plan's own verification list, one row each

| Plan word | Test | Where |
|---|---|---|
| **reattach** | a process started by run A is listed and read from cursor 0 by run B in the same Space; a third run in another Space gets `permission_denied` from execution-core's Space check | `process_host.rs` + `server_test.go` (`TestReadProcessOutputAcrossRuns…`) |
| **cursor resume** | append seqs 1..3, read after 0 → 3 rows; after 2 → 1 row; after 3 on a terminal process → empty page + state, not `NotFound`; retried batch is a no-op; `gap_before` after trimming | `store_integration_test.go` (real Postgres via testcontainers) |
| **worker restart** | rows written through one `Store`/pool are read through a fresh `Store`/pool (`TestLeaseStore_SurvivesAFreshPoolAgainstTheSameDatabase`'s template); `ReconcileProcesses` with a new epoch marks the old epoch's live rows `LOST` with `cleanup_state = done` and leaves other backends' rows alone; a stale-epoch `AppendProcessOutput` is refused by the fence | `store_integration_test.go`, `server_test.go` |
| **TTL** | fake clock (`nowFn`) — `RegisterProcess` clamps `ttl_seconds` to the lease's remaining time; host timer fires → `EXPIRED`/`ttl_expired`; sweeper marks a silent row `LOST` after `PROCESS_HEARTBEAT_STALE_SECS` and never touches a heartbeating one | `store_test.go`, `sweeper_test.go`, `process_host.rs` (`tokio::time::pause`) |
| **TERM→KILL** | passthrough `sh` that traps TERM and exits 0 → `EXITED`; one that ignores TERM → `KILLED` after grace; direct `kill` → `KILLED` immediately; state row carries `signal_requested`/`term_requested_at` | `process_host.rs` cross-platform; Linux proof in `verify-sandbox-isolation.sh` |
| **secret redaction** | `Command` on `["--token", "abc123"]`, `["KEY=secret"]`, a `postgres://u:p@h` arg; a line `Bearer x…` in stdout is `[REDACTED]` in the stored chunk; `preserves_ordinary_assignments` ported; a secret straddling a flush boundary does not survive because flushes are line-bounded | `redact_test.go`, `process_host.rs` |
| **concurrent readers** | 20 goroutines each read the same process from cursor 0 while the writer appends; every reader sees strictly increasing seqs and the same final set (`TestLeaseStore_ConcurrentCreateIsRaceFree`'s indexed-slice shape + `streaming_test.go`'s monotonicity assertion) | `store_integration_test.go` |
| profile truthfulness | `processes == "background_registry"` iff host enabled ∧ bwrap; digest differs between the two | `sandbox.rs` |
| authority | not entitled → decision signed without `space:processes`; `RegisterProcess` on such a lease → `PermissionDenied`; `Processes: "isolated"` → Control rejects; every new method refused without its scope and accepted with it (interceptor table) | `space_capability_decision_test.go`, `server_test.go`, `authz_test.go` |
| ZDR | ZDR caller's `process_start` → `failed_precondition` before any RPC | `runtime_loop` tests |
| offering ↔ mapping | every process tool name is both offered and mapped | `agent.rs` contract test |

## 10. Sequencing

Every step is independently testable and ships with no caller wired ahead of
the step that needs it — the S3.2/S3.3 precedent.

0. **Scope bug fix** — `SandboxManagerTokenProvider` requests both scopes.
   **DONE 2026-09-13**, committed separately ahead of this doc.
1. **Registry primitive** (§2): migration `0003`, `internal/redact`,
   `internal/process/store.go` + tests, sweeper. No proto, no server wiring;
   nothing calls it yet. **DONE 2026-09-13.** What differed from this list,
   and why:
   - **No `MemoryProcessStore`.** The in-memory lease/snapshot/workspace
     stores exist for `server_test.go`'s fast path and the
     ephemeral-development fallback; neither has a consumer until step 2
     adds the RPCs, and a registry whose entire purpose is surviving a
     restart has nothing to offer in a mode that discards it. It lands with
     the handlers that need it, or not at all.
   - **No `ProcessDecisionsTotal` counter.** It is emitted by handlers,
     which do not exist yet; an unread counter is not evidence.
   - **The other two integration tests' migration lists are unchanged.**
     `internal/lease` and `internal/workspace` do not read
     `processes_permitted` until step 2 teaches `lease.go` about it, so
     adding `0003` to their setup now would be unused scaffolding. It goes
     in when the code under test needs it.
   - **A real bug the integration tests caught**, recorded because it is the
     kind that unit tests structurally cannot: `ttl_seconds` was bound once
     and used both as its `INTEGER` column and inside `$n::bigint * interval
     '1 second'`. Postgres deduces a parameter's type from every use and
     refuses the statement (`42P08`). The stub-based tests asserted the SQL
     text and the bound args — both correct — and passed; only a real
     planner rejects it. Fixed by binding the TTL a second time as its own
     parameter.
   - **A design error found while writing the retention test**, fixed in
     §2.3's contract: `GapBefore` originally compared the cursor only
     against `retained_from_seq`, which can never detect anything, because
     protecting the head means a trim leaves its hole in the MIDDLE of the
     stream. A reader resuming from inside the surviving head would have
     been handed the surviving tail with no indication that anything was
     dropped between them. It now also reports a gap whenever the first
     returned chunk is not `cursor + 1`.
2. **RPCs + authz + lease column** (§4 sandbox-manager half, §5): proto,
   handlers, seven authz cases + interceptor rows, `Verify` returns
   permissions, `AcquireLease` persists `processes_permitted`. Rust client
   methods regenerate against it. Still no host.
3. **Control** (§4 first layer): migration `028`, evidence field,
   `space:processes` permission, `processes` enum validation. Independent of
   2 (a decision without the permission is what every current caller gets).
4. **Process host** (§3): `process_host.rs`, measured profile, `SandboxLease.
   processes_permitted`, boot reconcile, kill-before-release; behind
   `EXECUTION_CORE_PROCESS_HOST`, default off. Linux TERM/KILL probe added to
   `verify-sandbox-isolation.sh` and run in the runtime image before this
   step is called done.
5. **Tools + capability** (§6): `cap.process.background` (migration `0015`,
   allowlist, seed, attestation), dispatch arms, offering, contract test,
   ZDR gate. First point at which a model can start a process.
6. **Human read path** (§7): Control audience parameter, sandbox-manager
   read verifier, model-gateway `/v1/processes`, V3 `/work` `processes`
   section, frontend adapter — read-only. This is S4.5's consumption of S4.2
   and may be tracked there; it is specified here so the registry's read
   API is shaped for it from step 2, not retrofitted.

## Non-goals

- **Process survival across an execution-core restart.** §1. Recorded as
  the trigger condition for a supervisor / second backend, not built.
- **Cross-host control.** Signal and stdin are local to the hosting backend;
  a registry-mediated "signal intent" the host polls is a possible later
  slice, not this one.
- **Network ingress or egress for processes.** The sandbox stays
  `--unshare-net`; a dev server nobody can reach is out of scope until
  `space:egress` (which the verifier already knows) has a process-shaped
  consumer.
- **Offering in the plain chat loop** and **UI controls** (signal/stdin
  from the room). Both need decisions this doc does not make (§6, §7).
- **Resource limits beyond count/TTL/output.** No rlimit/cgroup mechanism
  exists in the crate today (verified); adding one is its own item.

## Open questions (flagged, not resolved)

- **Service-principal read exposure.** execution-core's service token is
  org-wide for the whole lease RPC family; this doc keeps that posture and
  puts the model-facing Space check in execution-core. Whether sandbox-manager
  should require a Space binding on *every* service read (e.g. the lease's
  `run_id` ↔ caller run correlation) is the same broader question S3.3 step 4
  left open for write RPCs. Not unique to processes; larger blast radius
  because output is content.
- **Storing the lease's decision revisions on the lease.** §7's audience
  ceiling for human reads needs `recipient_audience_revision` from the
  capability decision, which `AcquireLease` currently discards along with
  the rest of the claims. Persisting the whole `Permissions` list and the
  five revisions on `leases` (not just the derived boolean) would serve both
  this and the previous item; recommended, but it widens step 2.
- **Risk level of `cap.process.background`** — `low` recommended (§6); `ask`
  is one field if product prefers to start there.
- **TERM delivery mechanism** — resolved on Linux in step 4 (§3.4); the
  contract does not change either way.
- **Retention purge.** Terminal rows and their output are kept indefinitely
  by this design (soft-terminal, like leases). A purge policy
  (`PROCESS_RETENTION_HOURS`) touches Space deletion semantics
  (`DeleteSpaceThreads` erases nothing here today) and belongs with the
  Space-deletion owner, not here.
- **Release artifact.** `scripts/release-artifact.sh:96-101`'s
  `MIGRATION_SOURCES` does not list sandbox-manager at all — `0001`/`0002`
  are already outside the signed artifact. `0003` makes that gap larger;
  flagging for release engineering, not fixing silently.
- **`SandboxBackend` trait.** Not proposed. One backend still; the host is a
  new lifecycle over it, not a second implementation. The 2026-08-22 entry's
  trigger is process survival (Non-goals), not this.
