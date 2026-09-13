// Package process holds the durable (Postgres-backed) registry of
// background processes running inside a Space-scoped sandbox lease —
// S4.2 step 1, per
// apps/Frontend Plane/verevonv3/docs/S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md §2.
//
// The ownership line this package sits on (design §1): sandbox-manager owns
// the ROWS — metadata, the output log, cursors, retention, the state machine
// and cleanup state — while execution-core owns the OS process. Nothing here
// spawns, signals or reaps anything; it records what the host reports and
// makes the record impossible to lie with.
//
// Two conventions are inherited from internal/lease and internal/workspace:
// a narrow Exec/Query/QueryRow interface over *pgxpool.Pool so unit tests
// substitute a stub, and no explicit transactions or row locks. The second
// one needs justifying here, because this store has a genuine
// concurrent-writer problem the lease store does not: a stale host must not
// be able to append to a process a newer boot has already taken over. Rather
// than introduce Begin/FOR UPDATE into a package family that has never used
// them, every statement that writes on a host's behalf carries its own fence
// predicate (id + org + backend_id + host_epoch + a live state) INSIDE the
// statement — the same "fold the read into the write" shape
// workspace.Store.Promote uses to close its own lost-update window. A fenced
// write by a superseded host matches zero rows and lands nothing.
package process

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// State is a process's lifecycle state, stored as its numeric value in
// sandbox_processes.state.
//
// These numbers are the source of truth until S4.2 step 2 adds the
// ProcessState proto enum, which must mirror them exactly — the same
// relationship leases.state already has with SandboxLifecycleState.
type State int16

// Process lifecycle states. STARTING exists so the registry can reserve a
// slot (limits, TTL clamp, lease eligibility) before the host spawns.
const (
	StateUnspecified State = 0
	StateStarting    State = 1
	StateRunning     State = 2
	StateExited      State = 3
	StateKilled      State = 4
	StateLost        State = 5
	StateExpired     State = 6
)

// IsLive reports whether a process may still be written to by its host.
func (s State) IsLive() bool { return s == StateStarting || s == StateRunning }

// IsTerminal reports whether a process has reached a final state.
func (s State) IsTerminal() bool { return s >= StateExited && s <= StateExpired }

// End reasons. The set is closed and mirrored by a CHECK constraint in
// migration 0003: a free-form reason column would eventually carry provider
// errors or command output, which is exactly what this table must not hold.
const (
	EndExited                = "exited"
	EndSignaled              = "signaled"
	EndTTLExpired            = "ttl_expired"
	EndLeaseReleased         = "lease_released"
	EndHostLost              = "host_lost"
	EndSpawnFailed           = "spawn_failed"
	EndOutputBudgetExhausted = "output_budget_exhausted"
)

// Stream identifies which of a process's streams a chunk came from.
type Stream int16

// Output streams. StreamSystem carries registry-authored markers only
// ("process started", "SIGTERM requested", "output trimmed: N bytes
// dropped"), never process content.
const (
	StreamStdout Stream = 1
	StreamStderr Stream = 2
	StreamSystem Stream = 3
)

// Signal is the strongest termination signal a caller has asked for.
type Signal int16

// Requested signals. Ordered so an escalation to kill can never be
// downgraded back to term (see RequestSignal's GREATEST).
const (
	SignalNone Signal = 0
	SignalTerm Signal = 1
	SignalKill Signal = 2
)

// Cleanup states: whether the host has finished tearing down a finished
// process's pipes and scratch directory.
const (
	CleanupPending int16 = 1
	CleanupDone    int16 = 2
)

// Sentinel errors.
var (
	// ErrProcessNotFound means no such process in this organization.
	ErrProcessNotFound = errors.New("process not found")
	// ErrProcessFenced means the caller is not the host that owns this
	// process: a different backend, a superseded host_epoch, or a process
	// that is no longer live. A host receiving this must stop writing and
	// drop its local handle — the registry has moved on without it.
	ErrProcessFenced = errors.New("process is not owned by this host epoch")
	// ErrLeaseNotEligible means the lease cannot host background processes
	// because of its own state: missing, not ACTIVE, expired, or pinned to
	// another backend.
	ErrLeaseNotEligible = errors.New("lease is not eligible to host background processes")
	// ErrProcessesNotPermitted means the lease is otherwise fine but its
	// Space capability decision never granted space:processes. Separate from
	// ErrLeaseNotEligible because it is an authority answer, not a state
	// one — the caller is refused no matter how long it waits or retries.
	ErrProcessesNotPermitted = errors.New("lease is not permitted to host background processes")
	// ErrProcessLimit means this lease or Space already has as many live
	// processes as it is allowed.
	ErrProcessLimit = errors.New("live process limit reached")
)

// Command is a process's argv, already redacted by internal/redact.Command
// before it reaches this package. The registry never sees the plaintext.
type Command struct {
	Program string   `json:"program"`
	Args    []string `json:"args"`
}

// Process is one registered background process.
type Process struct {
	ID        string
	OrgID     string
	SpaceID   string
	LeaseID   string
	BackendID string
	HostEpoch string

	RunID     string
	StepID    string
	SubjectID string

	Command       Command
	CommandDigest string

	State           State
	ExitCode        *int32
	EndReason       string
	SignalRequested Signal
	TermRequestedAt *time.Time
	CleanupState    int16

	TTLSeconds      int32
	ExpiresAt       time.Time
	StartedAt       *time.Time
	EndedAt         *time.Time
	LastHeartbeatAt time.Time

	NextSeq         int64
	RetainedFromSeq int64
	RetainedBytes   int64
	DroppedBytes    int64
	StdinBytes      int64

	CreatedAt time.Time
	UpdatedAt time.Time
}

// Chunk is one append-only slice of a process's output. Seq is assigned by
// the host, which is the single writer for its own process; the primary key
// on (process_id, seq) is what turns a retried batch into a no-op instead of
// a duplicate.
type Chunk struct {
	Seq             int64
	Stream          Stream
	Content         []byte
	EndsWithNewline bool
	CapturedAt      time.Time
}

// OutputPage is one cursor-paged read of a process's output.
type OutputPage struct {
	Chunks     []Chunk
	NextCursor int64
	// GapBefore reports that output between the caller's cursor and the
	// first chunk on this page was trimmed and will never be returned. A
	// resuming reader must surface it rather than presenting the page as
	// contiguous.
	//
	// It is computed from the first returned chunk's seq, not only from
	// RetainedFromSeq, because retention protects the head: the hole a
	// trim leaves is in the MIDDLE of the stream, so a reader resuming
	// from inside the surviving head would otherwise be handed the
	// surviving tail with no indication that anything was dropped between
	// them.
	GapBefore       bool
	RetainedFromSeq int64
	State           State
	ExitCode        *int32
	EndReason       string
	HasMore         bool
}

// AppendResult reports a process's output bookkeeping after an append.
type AppendResult struct {
	NextSeq         int64
	RetainedFromSeq int64
	RetainedBytes   int64
	DroppedBytes    int64
}

// Fence identifies both the process and the exact host allowed to write it.
type Fence struct {
	ProcessID string
	OrgID     string
	BackendID string
	HostEpoch string
}

func (f Fence) validate() error {
	if f.ProcessID == "" || f.OrgID == "" || f.BackendID == "" || f.HostEpoch == "" {
		return fmt.Errorf("process_id, org_id, backend_id, and host_epoch are required")
	}
	return nil
}

// RegisterRequest reserves a process slot before the host spawns anything.
//
// There is deliberately no SpaceID: the Space is whatever the lease says it
// is, read from the lease row inside the same statement that inserts. A
// caller cannot name one, so it cannot name the wrong one.
type RegisterRequest struct {
	ID        string
	OrgID     string
	LeaseID   string
	BackendID string
	HostEpoch string
	RunID     string
	StepID    string
	SubjectID string
	// Command must already be redacted.
	Command       Command
	CommandDigest string
	TTLSeconds    int32
}

// Limits bound what one lease, one Space, and one process may consume.
type Limits struct {
	MaxLivePerLease int32
	MaxLivePerSpace int32
	// OutputHeadBytes is never trimmed: the start of a process's output
	// carries its banner and first errors, which is what a human reads to
	// understand a failure long after the tail has rolled over.
	OutputHeadBytes int64
	// OutputRetainBytes is the newest-output window kept beyond the head.
	OutputRetainBytes int64
	// MaxReadBytes caps one ReadOutput page.
	MaxReadBytes int64
	// MaxListLimit caps one ListProcesses page.
	MaxListLimit int32
}

// DefaultLimits are the design's own defaults (§2.3, §4).
func DefaultLimits() Limits {
	return Limits{
		MaxLivePerLease:   4,
		MaxLivePerSpace:   8,
		OutputHeadBytes:   64 * 1024,
		OutputRetainBytes: 2 * 1024 * 1024,
		MaxReadBytes:      256 * 1024,
		MaxListLimit:      100,
	}
}

type processDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// Store is the durable process registry backed by sandbox_processes and
// sandbox_process_output (migration 0003).
type Store struct {
	pool   processDatabase
	limits Limits
}

// NewStore constructs a Store over pool with DefaultLimits. Returns an error
// for a nil pool — there is no in-memory fallback here, matching
// lease.Store/workspace.Store's own convention for a store meant to survive
// a restart.
func NewStore(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Store{pool: pool, limits: DefaultLimits()}, nil
}

// WithLimits returns a copy of s using the given limits. Zero-valued fields
// keep their default, so a caller can override one bound without restating
// the rest.
func (s *Store) WithLimits(limits Limits) *Store {
	merged := s.limits
	if limits.MaxLivePerLease > 0 {
		merged.MaxLivePerLease = limits.MaxLivePerLease
	}
	if limits.MaxLivePerSpace > 0 {
		merged.MaxLivePerSpace = limits.MaxLivePerSpace
	}
	if limits.OutputHeadBytes > 0 {
		merged.OutputHeadBytes = limits.OutputHeadBytes
	}
	if limits.OutputRetainBytes > 0 {
		merged.OutputRetainBytes = limits.OutputRetainBytes
	}
	if limits.MaxReadBytes > 0 {
		merged.MaxReadBytes = limits.MaxReadBytes
	}
	if limits.MaxListLimit > 0 {
		merged.MaxListLimit = limits.MaxListLimit
	}
	return &Store{pool: s.pool, limits: merged}
}

// Limits reports the bounds this store enforces.
func (s *Store) Limits() Limits { return s.limits }

const processColumns = `id, org_id, space_id, lease_id, backend_id, host_epoch, ` +
	`run_id, step_id, subject_id, command_redacted, command_digest, ` +
	`state, exit_code, end_reason, signal_requested, term_requested_at, cleanup_state, ` +
	`ttl_seconds, expires_at, started_at, ended_at, last_heartbeat_at, ` +
	`next_seq, retained_from_seq, retained_bytes, dropped_bytes, stdin_bytes, ` +
	`created_at, updated_at`

// Register reserves a STARTING row for a process the host is about to spawn.
//
// The eligibility gate and both live-count limits are evaluated INSIDE the
// insert, joined against the lease itself, so two concurrent registrations
// cannot both pass a limit check. The separate pre-read exists only to
// classify a refusal into a precise error — it is never the authority, and
// the insert re-checks everything it looked at.
//
// expires_at is clamped to the lease's own expiry: a process must never
// outlive the lease whose workspace and authority it runs under. The
// requested TTL is stored as-is for the record.
func (s *Store) Register(ctx context.Context, req RegisterRequest) (*Process, error) {
	if req.ID == "" || req.OrgID == "" || req.LeaseID == "" ||
		req.BackendID == "" || req.HostEpoch == "" || req.RunID == "" || req.SubjectID == "" {
		return nil, fmt.Errorf("id, org_id, lease_id, backend_id, host_epoch, run_id, and subject_id are required")
	}
	if req.Command.Program == "" {
		return nil, fmt.Errorf("command program is required")
	}
	if req.CommandDigest == "" {
		return nil, fmt.Errorf("command_digest is required")
	}
	if req.TTLSeconds <= 0 {
		return nil, fmt.Errorf("ttl_seconds must be greater than zero")
	}
	command, err := json.Marshal(req.Command)
	if err != nil {
		return nil, fmt.Errorf("encode redacted command: %w", err)
	}

	row := s.pool.QueryRow(ctx, `
		INSERT INTO sandbox_processes (
			id, org_id, space_id, lease_id, backend_id, host_epoch,
			run_id, step_id, subject_id, command_redacted, command_digest,
			state, ttl_seconds, expires_at, last_heartbeat_at, created_at, updated_at)
		SELECT $1, $2, l.space_id, $3, $4, $5,
		       $6, $7, $8, $9::jsonb, $10,
		       $11, $12, LEAST(now() + ($15::bigint * interval '1 second'), l.expires_at),
		       now(), now(), now()
		FROM leases l
		WHERE l.id = $3
		  AND l.org_id = $2
		  AND l.backend_id = $4
		  AND l.processes_permitted
		  AND l.state = $13
		  AND l.expires_at > now()
		  AND l.space_id <> ''
		  AND (SELECT count(*) FROM sandbox_processes p
		        WHERE p.lease_id = $3 AND p.state IN (1, 2)) < $14
		  AND (SELECT count(*) FROM sandbox_processes p
		        WHERE p.org_id = $2 AND p.space_id = l.space_id AND p.state IN (1, 2)) < $16
		RETURNING `+processColumns,
		req.ID, req.OrgID, req.LeaseID, req.BackendID, req.HostEpoch,
		req.RunID, req.StepID, req.SubjectID, string(command), req.CommandDigest,
		int16(StateStarting), req.TTLSeconds, int32(mpv1.SandboxLifecycleState_ACTIVE),
		s.limits.MaxLivePerLease,
		// The same TTL again, as its own parameter: Postgres deduces a
		// parameter's type from every use, and one placeholder cannot be
		// both the INTEGER ttl_seconds column and the bigint of the
		// interval arithmetic ("inconsistent types deduced for parameter",
		// SQLSTATE 42P08).
		int64(req.TTLSeconds),
		s.limits.MaxLivePerSpace)
	p, err := scanProcess(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, s.diagnoseRefusal(ctx, req)
	}
	if err != nil {
		return nil, fmt.Errorf("register process: %w", err)
	}
	return p, nil
}

// diagnoseRefusal turns a zero-row Register into the specific reason. It
// runs only on the refusal path, and its answer is advisory: the insert
// above has already decided.
func (s *Store) diagnoseRefusal(ctx context.Context, req RegisterRequest) error {
	var (
		leaseState int32
		permitted  bool
		backendID  string
		spaceID    string
		expired    bool
		leaseLive  int64
		spaceLive  int64
	)
	err := s.pool.QueryRow(ctx, `
		SELECT l.state, l.processes_permitted, l.backend_id, l.space_id, l.expires_at <= now(),
		       (SELECT count(*) FROM sandbox_processes p WHERE p.lease_id = l.id AND p.state IN (1, 2)),
		       (SELECT count(*) FROM sandbox_processes p
		         WHERE p.org_id = l.org_id AND p.space_id = l.space_id AND p.state IN (1, 2))
		FROM leases l
		WHERE l.id = $1 AND l.org_id = $2
	`, req.LeaseID, req.OrgID).Scan(&leaseState, &permitted, &backendID, &spaceID, &expired, &leaseLive, &spaceLive)
	if errors.Is(err, pgx.ErrNoRows) {
		return fmt.Errorf("%w: lease %q not found in this organization", ErrLeaseNotEligible, req.LeaseID)
	}
	if err != nil {
		return fmt.Errorf("diagnose process registration refusal: %w", err)
	}
	switch {
	case spaceID == "":
		// A thread/agent-scoped lease has no Space workspace to run in, so
		// there is nowhere to put a background process even in principle.
		return fmt.Errorf("%w: lease is not Space-scoped", ErrLeaseNotEligible)
	case !permitted:
		return fmt.Errorf("%w: the lease's Space capability decision does not grant space:processes", ErrProcessesNotPermitted)
	case backendID != req.BackendID:
		return fmt.Errorf("%w: lease is pinned to another backend", ErrLeaseNotEligible)
	case expired:
		return fmt.Errorf("%w: lease has expired", ErrLeaseNotEligible)
	case mpv1.SandboxLifecycleState(leaseState) != mpv1.SandboxLifecycleState_ACTIVE:
		return fmt.Errorf("%w: lease is not ACTIVE", ErrLeaseNotEligible)
	case leaseLive >= int64(s.limits.MaxLivePerLease):
		return fmt.Errorf("%w: %d live processes on this lease (limit %d)", ErrProcessLimit, leaseLive, s.limits.MaxLivePerLease)
	case spaceLive >= int64(s.limits.MaxLivePerSpace):
		return fmt.Errorf("%w: %d live processes in this Space (limit %d)", ErrProcessLimit, spaceLive, s.limits.MaxLivePerSpace)
	}
	// Every predicate the insert checks now passes, so the refusal was a
	// lost race that has since resolved. Report it as a limit rather than
	// inventing a cause.
	return fmt.Errorf("%w: registration lost a concurrent race", ErrProcessLimit)
}

// MarkStarted moves a reserved process to RUNNING once the host has spawned
// it. Accepts a row already RUNNING so a retried report is idempotent.
func (s *Store) MarkStarted(ctx context.Context, f Fence) error {
	if err := f.validate(); err != nil {
		return err
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET state = $5, started_at = COALESCE(started_at, now()),
		    last_heartbeat_at = now(), updated_at = now()
		WHERE id = $1 AND org_id = $2 AND backend_id = $3 AND host_epoch = $4
		  AND state IN (1, 2)
	`, f.ProcessID, f.OrgID, f.BackendID, f.HostEpoch, int16(StateRunning))
	if err != nil {
		return fmt.Errorf("mark process started: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return s.classifyFenceMiss(ctx, f)
	}
	return nil
}

// RequestSignal records that a caller has asked for this process to stop.
// GREATEST means an escalation to kill can never be downgraded back to term
// by a later or reordered request.
func (s *Store) RequestSignal(ctx context.Context, f Fence, signal Signal) error {
	if err := f.validate(); err != nil {
		return err
	}
	if signal != SignalTerm && signal != SignalKill {
		return fmt.Errorf("signal must be term or kill")
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET signal_requested = GREATEST(signal_requested, $5),
		    term_requested_at = CASE
		        WHEN term_requested_at IS NULL THEN now()
		        ELSE term_requested_at
		    END,
		    updated_at = now()
		WHERE id = $1 AND org_id = $2 AND backend_id = $3 AND host_epoch = $4
		  AND state IN (1, 2)
	`, f.ProcessID, f.OrgID, f.BackendID, f.HostEpoch, int16(signal))
	if err != nil {
		return fmt.Errorf("request process signal: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return s.classifyFenceMiss(ctx, f)
	}
	return nil
}

// MarkEnded records a terminal outcome. The first terminal outcome wins: a
// later report for an already-terminal process is an idempotent no-op, not
// an error, so a host reporting an exit it observed just after the sweeper
// declared the process LOST does not fail — the registry already had a
// truthful answer and does not overwrite it.
func (s *Store) MarkEnded(ctx context.Context, f Fence, state State, exitCode *int32, endReason string, cleanupDone bool) error {
	if err := f.validate(); err != nil {
		return err
	}
	if !state.IsTerminal() {
		return fmt.Errorf("state %d is not terminal", state)
	}
	if endReason == "" {
		return fmt.Errorf("end_reason is required for a terminal state")
	}
	cleanup := CleanupPending
	if cleanupDone {
		cleanup = CleanupDone
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET state = $5, exit_code = $6, end_reason = $7, ended_at = now(),
		    cleanup_state = $8, updated_at = now()
		WHERE id = $1 AND org_id = $2 AND backend_id = $3 AND host_epoch = $4
		  AND state IN (1, 2)
	`, f.ProcessID, f.OrgID, f.BackendID, f.HostEpoch, int16(state), exitCode, endReason, cleanup)
	if err != nil {
		return fmt.Errorf("mark process ended: %w", err)
	}
	if tag.RowsAffected() == 0 {
		existing, lookupErr := s.Get(ctx, f.OrgID, f.ProcessID)
		if lookupErr != nil {
			return lookupErr
		}
		if existing.State.IsTerminal() && existing.BackendID == f.BackendID && existing.HostEpoch == f.HostEpoch {
			return nil
		}
		return ErrProcessFenced
	}
	return nil
}

// classifyFenceMiss explains a zero-row fenced write: an unknown process, or
// one this host no longer owns (superseded epoch, different backend, or
// already terminal).
func (s *Store) classifyFenceMiss(ctx context.Context, f Fence) error {
	if _, err := s.Get(ctx, f.OrgID, f.ProcessID); err != nil {
		return err
	}
	return ErrProcessFenced
}

// AppendOutput records a batch of output chunks and refreshes the process's
// heartbeat. A batch with no chunks is a pure heartbeat, which is how
// silence stays distinguishable from a dead host.
//
// Every insert carries the fence, so a superseded host lands nothing; the
// bookkeeping update carries it too and is what reports ErrProcessFenced.
// Chunk inserts are ON CONFLICT DO NOTHING, so re-sending a batch after an
// ambiguous transport failure is a no-op rather than a duplicate.
func (s *Store) AppendOutput(ctx context.Context, f Fence, chunks []Chunk, stdinBytesDelta int64) (*AppendResult, error) {
	if err := f.validate(); err != nil {
		return nil, err
	}
	if stdinBytesDelta < 0 {
		return nil, fmt.Errorf("stdin_bytes_delta must not be negative")
	}
	var appendedBytes int64
	maxSeq := int64(0)
	for _, c := range chunks {
		if c.Seq <= 0 {
			return nil, fmt.Errorf("chunk seq must be greater than zero")
		}
		if c.Stream != StreamStdout && c.Stream != StreamStderr && c.Stream != StreamSystem {
			return nil, fmt.Errorf("chunk stream %d is not a known stream", c.Stream)
		}
		appendedBytes += int64(len(c.Content))
		if c.Seq > maxSeq {
			maxSeq = c.Seq
		}
	}

	for _, c := range chunks {
		capturedAt := c.CapturedAt
		if capturedAt.IsZero() {
			capturedAt = time.Now().UTC()
		}
		if _, err := s.pool.Exec(ctx, `
			INSERT INTO sandbox_process_output (process_id, seq, stream, content, ends_with_newline, captured_at)
			SELECT $1, $2, $3, $4, $5, $6
			FROM sandbox_processes p
			WHERE p.id = $1 AND p.org_id = $7 AND p.backend_id = $8 AND p.host_epoch = $9
			  AND p.state IN (1, 2)
			ON CONFLICT (process_id, seq) DO NOTHING
		`, f.ProcessID, c.Seq, int16(c.Stream), c.Content, c.EndsWithNewline, capturedAt,
			f.OrgID, f.BackendID, f.HostEpoch); err != nil {
			return nil, fmt.Errorf("append process output chunk %d: %w", c.Seq, err)
		}
	}

	// retained_bytes is advanced optimistically here (a chunk that conflicted
	// away is still counted) and recomputed exactly by trim below whenever
	// the budget is reached, so the drift is bounded and self-correcting.
	// next_seq is GREATEST so an out-of-order or replayed batch never rewinds it.
	var result AppendResult
	row := s.pool.QueryRow(ctx, `
		UPDATE sandbox_processes
		SET next_seq = GREATEST(next_seq, $5),
		    retained_bytes = retained_bytes + $6,
		    stdin_bytes = stdin_bytes + $7,
		    last_heartbeat_at = now(), updated_at = now()
		WHERE id = $1 AND org_id = $2 AND backend_id = $3 AND host_epoch = $4
		  AND state IN (1, 2)
		RETURNING next_seq, retained_from_seq, retained_bytes, dropped_bytes
	`, f.ProcessID, f.OrgID, f.BackendID, f.HostEpoch, maxSeq+1, appendedBytes, stdinBytesDelta)
	if err := row.Scan(&result.NextSeq, &result.RetainedFromSeq, &result.RetainedBytes, &result.DroppedBytes); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, s.classifyFenceMiss(ctx, f)
		}
		return nil, fmt.Errorf("record process output bookkeeping: %w", err)
	}

	if result.RetainedBytes > s.limits.OutputHeadBytes+s.limits.OutputRetainBytes {
		trimmed, err := s.trim(ctx, f.ProcessID)
		if err != nil {
			return nil, err
		}
		result = *trimmed
	}
	return &result, nil
}

// trim enforces the head+tail retention window: the first OutputHeadBytes
// are never deleted, and beyond them only the newest OutputRetainBytes are
// kept. Deleting from the middle is deliberate — the start of a process's
// output explains what it was doing and the end explains what happened,
// while the middle of a runaway logger explains neither. A reader whose
// cursor falls into the hole learns so from GapBefore rather than silently
// receiving a discontinuous page.
func (s *Store) trim(ctx context.Context, processID string) (*AppendResult, error) {
	var droppedBytes int64
	if err := s.pool.QueryRow(ctx, `
		WITH totals AS (
			SELECT COALESCE(SUM(octet_length(content)), 0) AS total
			FROM sandbox_process_output WHERE process_id = $1
		),
		ordered AS (
			SELECT seq, SUM(octet_length(content)) OVER (ORDER BY seq) AS cumulative
			FROM sandbox_process_output WHERE process_id = $1
		),
		victims AS (
			SELECT ordered.seq FROM ordered, totals
			WHERE ordered.cumulative > $2
			  AND (totals.total - ordered.cumulative) >= $3
		),
		deleted AS (
			DELETE FROM sandbox_process_output o
			USING victims v
			WHERE o.process_id = $1 AND o.seq = v.seq
			RETURNING octet_length(o.content) AS bytes
		)
		SELECT COALESCE(SUM(bytes), 0)::bigint FROM deleted
	`, processID, s.limits.OutputHeadBytes, s.limits.OutputRetainBytes).Scan(&droppedBytes); err != nil {
		return nil, fmt.Errorf("trim process output: %w", err)
	}

	var result AppendResult
	if err := s.pool.QueryRow(ctx, `
		UPDATE sandbox_processes
		SET dropped_bytes = dropped_bytes + $2,
		    retained_bytes = COALESCE(
		        (SELECT SUM(octet_length(content)) FROM sandbox_process_output WHERE process_id = $1), 0),
		    retained_from_seq = COALESCE(
		        (SELECT MIN(seq) FROM sandbox_process_output WHERE process_id = $1), retained_from_seq),
		    updated_at = now()
		WHERE id = $1
		RETURNING next_seq, retained_from_seq, retained_bytes, dropped_bytes
	`, processID, droppedBytes).Scan(
		&result.NextSeq, &result.RetainedFromSeq, &result.RetainedBytes, &result.DroppedBytes); err != nil {
		return nil, fmt.Errorf("record process output retention: %w", err)
	}
	return &result, nil
}

// Get resolves one process within an organization. Deliberately descriptive:
// unlike the fenced writes above it does not exclude terminal rows, because
// a KILLED or LOST process's record and output tail must stay readable —
// the same lookup split internal/lease draws between its operational grants
// and GetAny.
func (s *Store) Get(ctx context.Context, orgID, processID string) (*Process, error) {
	if orgID == "" || processID == "" {
		return nil, fmt.Errorf("org_id and process_id are required")
	}
	row := s.pool.QueryRow(ctx, `
		SELECT `+processColumns+`
		FROM sandbox_processes
		WHERE id = $1 AND org_id = $2
	`, processID, orgID)
	p, err := scanProcess(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrProcessNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get process: %w", err)
	}
	return p, nil
}

// List pages a Space's processes newest-first. The cursor is the process id
// itself (a ULID, so id order is creation order), the same keyset shape
// ListRuns uses. hasMore is computed by over-fetching one row.
func (s *Store) List(ctx context.Context, orgID, spaceID string, includeTerminal bool, limit int32, afterID string) ([]Process, bool, error) {
	if orgID == "" || spaceID == "" {
		return nil, false, fmt.Errorf("org_id and space_id are required")
	}
	if limit <= 0 || limit > s.limits.MaxListLimit {
		limit = s.limits.MaxListLimit
	}
	rows, err := s.pool.Query(ctx, `
		SELECT `+processColumns+`
		FROM sandbox_processes
		WHERE org_id = $1 AND space_id = $2
		  AND ($3 OR state IN (1, 2))
		  AND ($4 = '' OR id < $4)
		ORDER BY id DESC
		LIMIT $5
	`, orgID, spaceID, includeTerminal, afterID, int64(limit)+1)
	if err != nil {
		return nil, false, fmt.Errorf("list processes: %w", err)
	}
	defer rows.Close()

	var out []Process
	for rows.Next() {
		p, scanErr := scanProcess(rows)
		if scanErr != nil {
			return nil, false, fmt.Errorf("scan process: %w", scanErr)
		}
		out = append(out, *p)
	}
	if err := rows.Err(); err != nil {
		return nil, false, fmt.Errorf("list processes: %w", err)
	}
	hasMore := int32(len(out)) > limit
	if hasMore {
		out = out[:limit]
	}
	return out, hasMore, nil
}

// ReadOutput returns the chunks after a cursor, bounded by byte budget.
//
// A terminal process with nothing left after the cursor returns an empty
// page carrying its terminal state — never a not-found. A fully
// acknowledged stream is still a known stream, the same rule
// model-gateway's own resume buffer holds.
func (s *Store) ReadOutput(ctx context.Context, orgID, processID string, afterSeq, maxBytes int64) (*OutputPage, error) {
	p, err := s.Get(ctx, orgID, processID)
	if err != nil {
		return nil, err
	}
	if afterSeq < 0 {
		afterSeq = 0
	}
	if maxBytes <= 0 || maxBytes > s.limits.MaxReadBytes {
		maxBytes = s.limits.MaxReadBytes
	}

	rows, err := s.pool.Query(ctx, `
		SELECT seq, stream, content, ends_with_newline, captured_at
		FROM sandbox_process_output
		WHERE process_id = $1 AND seq > $2
		ORDER BY seq
	`, processID, afterSeq)
	if err != nil {
		return nil, fmt.Errorf("read process output: %w", err)
	}
	defer rows.Close()

	page := &OutputPage{
		NextCursor:      afterSeq,
		RetainedFromSeq: p.RetainedFromSeq,
		State:           p.State,
		ExitCode:        p.ExitCode,
		EndReason:       p.EndReason,
		// The caller asked to resume from before the oldest chunk that still
		// exists at all. The first-chunk check in the loop below catches the
		// other, more common case: a hole trimmed out of the middle.
		GapBefore: afterSeq+1 < p.RetainedFromSeq,
	}
	var budget int64
	for rows.Next() {
		var c Chunk
		var stream int16
		if err := rows.Scan(&c.Seq, &stream, &c.Content, &c.EndsWithNewline, &c.CapturedAt); err != nil {
			return nil, fmt.Errorf("scan process output chunk: %w", err)
		}
		c.Stream = Stream(stream)
		// The host assigns one contiguous seq counter per process across
		// both streams, so the first chunk after a cursor is seq+1 unless
		// something in between was trimmed.
		if len(page.Chunks) == 0 && c.Seq != afterSeq+1 {
			page.GapBefore = true
		}
		// Always return at least one chunk, even an oversized one, so a
		// reader can never be wedged by a single chunk larger than its
		// budget.
		if len(page.Chunks) > 0 && budget+int64(len(c.Content)) > maxBytes {
			page.HasMore = true
			break
		}
		budget += int64(len(c.Content))
		page.Chunks = append(page.Chunks, c)
		page.NextCursor = c.Seq
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read process output: %w", err)
	}
	return page, nil
}

// Reconcile marks every live process this backend owns under a DIFFERENT
// host epoch as LOST. execution-core calls it once at boot, before binding
// its listener: a restarted host cannot resume its predecessor's children
// (they died with it, by bwrap's --die-with-parent), so the registry must
// say so rather than leave rows claiming to be RUNNING.
//
// cleanup_state is set done because there is nothing left to clean: the
// children died with the process that owned their pipes and scratch
// directories.
func (s *Store) Reconcile(ctx context.Context, backendID, hostEpoch string) (int64, error) {
	if backendID == "" || hostEpoch == "" {
		return 0, fmt.Errorf("backend_id and host_epoch are required")
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET state = $3, end_reason = $4, ended_at = now(),
		    cleanup_state = $5, updated_at = now()
		WHERE backend_id = $1 AND host_epoch <> $2 AND state IN (1, 2)
	`, backendID, hostEpoch, int16(StateLost), EndHostLost, CleanupDone)
	if err != nil {
		return 0, fmt.Errorf("reconcile processes: %w", err)
	}
	return tag.RowsAffected(), nil
}

// SweepStale marks every live process whose host has stopped heartbeating as
// LOST. This is the backstop for a host that vanished without reconciling —
// killed, partitioned, or crashed hard enough never to boot again. One
// idempotent statement, so replicas need no claim or lease of their own.
func (s *Store) SweepStale(ctx context.Context, staleAfter time.Duration) (int64, error) {
	if staleAfter <= 0 {
		return 0, fmt.Errorf("stale_after must be greater than zero")
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET state = $2, end_reason = $3, ended_at = now(),
		    cleanup_state = $4, updated_at = now()
		WHERE state IN (1, 2)
		  AND last_heartbeat_at < now() - ($1::bigint * interval '1 second')
	`, int64(staleAfter.Seconds()), int16(StateLost), EndHostLost, CleanupDone)
	if err != nil {
		return 0, fmt.Errorf("sweep stale processes: %w", err)
	}
	return tag.RowsAffected(), nil
}

// KillForLease marks a released lease's live processes KILLED. This is the
// registry's half only: the host does the actual killing on its own release
// path, and cleanup_state stays pending precisely because this call proves
// nothing about the OS process. A lease released out from under a still-live
// host is caught afterwards by SweepStale, never by a row here pretending
// the process stopped.
func (s *Store) KillForLease(ctx context.Context, orgID, leaseID string) (int64, error) {
	if orgID == "" || leaseID == "" {
		return 0, fmt.Errorf("org_id and lease_id are required")
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE sandbox_processes
		SET state = $3, end_reason = $4, ended_at = now(), updated_at = now()
		WHERE org_id = $1 AND lease_id = $2 AND state IN (1, 2)
	`, orgID, leaseID, int16(StateKilled), EndLeaseReleased)
	if err != nil {
		return 0, fmt.Errorf("kill processes for lease: %w", err)
	}
	return tag.RowsAffected(), nil
}

// rowScanner is satisfied by both pgx.Row and pgx.Rows.
type rowScanner interface {
	Scan(dest ...any) error
}

func scanProcess(row rowScanner) (*Process, error) {
	var (
		p          Process
		state      int16
		signal     int16
		command    []byte
		endReason  *string
		termAt     *time.Time
		startedAt  *time.Time
		endedAt    *time.Time
		exitCode   *int32
		cleanup    int16
		ttlSeconds int32
	)
	if err := row.Scan(
		&p.ID, &p.OrgID, &p.SpaceID, &p.LeaseID, &p.BackendID, &p.HostEpoch,
		&p.RunID, &p.StepID, &p.SubjectID, &command, &p.CommandDigest,
		&state, &exitCode, &endReason, &signal, &termAt, &cleanup,
		&ttlSeconds, &p.ExpiresAt, &startedAt, &endedAt, &p.LastHeartbeatAt,
		&p.NextSeq, &p.RetainedFromSeq, &p.RetainedBytes, &p.DroppedBytes, &p.StdinBytes,
		&p.CreatedAt, &p.UpdatedAt,
	); err != nil {
		return nil, err
	}
	if len(command) > 0 {
		if err := json.Unmarshal(command, &p.Command); err != nil {
			return nil, fmt.Errorf("decode redacted command: %w", err)
		}
	}
	p.State = State(state)
	p.SignalRequested = Signal(signal)
	p.CleanupState = cleanup
	p.TTLSeconds = ttlSeconds
	p.ExitCode = exitCode
	p.TermRequestedAt = termAt
	p.StartedAt = startedAt
	p.EndedAt = endedAt
	if endReason != nil {
		p.EndReason = *endReason
	}
	return &p, nil
}
