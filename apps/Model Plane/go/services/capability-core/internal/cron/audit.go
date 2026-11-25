package cron

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"log/slog"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Schedule contract audit (build plan K3-4). A create, a renewal, a refusal and an expiry of a stored
// consent contract each write one content-free registry_audit_log row. A row is written in the same
// transaction as the change it records, where the change has one, so the two commit or roll back
// together. A refusal changes nothing, so its row is written on its own. A create or renewal refusal
// whose row cannot be written is answered 503 audit_unavailable instead of its refusal, so no refusal
// is answered without its row; the sweeper's and the dispatcher's rows are logged and queued for the next
// tick, and an expiry row lost with the process is written again by the sweep's durable recovery. The
// writer runs the registry_audit_log INSERT itself rather than reuse
// registry.CapabilitiesStore.AppendAuditLog, because that writes through its own pool and cannot join
// the transaction of the create or the guard pause. A row's diff holds the
// contract's digest, its tool count and expiry, the closed class and a closed reason token. It never
// holds a tool name, an argument, the goal, the template text or an error string. A renewal is the
// in-place PATCH in cron_contract_renew.go, and the statement that replaces the contract writes its
// row, so one renewal is one row.

// Actions of a schedule contract audit row. The entity kind is always "schedule", and the entity id
// is the schedule id. A renewal writes schedule.contract.renewed in the statement that replaces the
// contract, so one renewal is one row, and its decision's idempotency key keeps it to one. The O-10
// renewal rows written before this vocabulary (schedule_contract_renewed, entity kind cron_schedule)
// keep their names; no repo reader depends on them.
const (
	ContractAuditCreated = "schedule.contract.created"
	ContractAuditRenewed = "schedule.contract.renewed"
	ContractAuditRefused = "schedule.contract.refused"
	ContractAuditExpired = "schedule.contract.expired"

	// ContractAuditEntityKind is the entity kind of every contract audit row.
	ContractAuditEntityKind = "schedule"
)

// Closed reason tokens of a refused contract. Nothing else is ever written as a reason.
const (
	ContractTokenContractsDisabled = "contracts_disabled"
	ContractTokenInvalid           = "contract_invalid"
	ContractTokenExpired           = "contract_expired"
	ContractTokenWindowInvalid     = "window_invalid"
	ContractTokenRegistryMismatch  = "registry_mismatch"
)

// SweeperAuditActor is the actor of an expiry row the sweeper writes. The dispatcher's rows carry its
// own actor.
const SweeperAuditActor = "capability-core/cron-sweeper"

// contractAuditRetryLimit bounds the rows a retry queue holds. Past it, the oldest row is dropped and
// logged.
const contractAuditRetryLimit = 256

// auditTimeLayout is the canonical contract time, the one ParseScheduleContract accepts.
const auditTimeLayout = "2006-01-02T15:04:05Z"

// AuditExecer runs the audit insert. A *pgxpool.Pool, a pgx.Tx and a savepoint all satisfy it.
type AuditExecer interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
}

// ContractAuditRow is one audit row before it is written.
type ContractAuditRow struct {
	// ID is the row's primary key. Empty, a fresh id is made. A fixed id makes a retried row land once.
	ID       string
	EntityID string
	Action   string
	Actor    string
	OrgID    string
	// Diff is the content-free JSON diff, as ContractAuditDiffJSON makes it.
	Diff []byte
}

// ContractAuditor writes a contract audit row through db. db is the pool, or the transaction the
// audited change is made in. A nil db writes through the pool the auditor was built on.
type ContractAuditor interface {
	Append(ctx context.Context, db AuditExecer, row ContractAuditRow) error
}

// NewContractAuditor builds the registry_audit_log writer over the registry pool. A nil pool is an
// error, because contract audit cannot be off while contracts are on.
func NewContractAuditor(pool *pgxpool.Pool) (ContractAuditor, error) {
	if pool == nil {
		return nil, errors.New("schedule contract audit needs the registry database")
	}
	return registryContractAuditor{pool: pool}, nil
}

type registryContractAuditor struct {
	pool *pgxpool.Pool
}

// Append writes one row. ON CONFLICT DO NOTHING makes a fixed id land once, so a retried write never
// doubles a row.
func (a registryContractAuditor) Append(ctx context.Context, db AuditExecer, row ContractAuditRow) error {
	if db == nil {
		db = a.pool
	}
	id := row.ID
	if id == "" {
		id = NewAuditID()
	}
	diff := row.Diff
	if len(diff) == 0 {
		diff = []byte("{}")
	}
	_, err := db.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
		ON CONFLICT (id) DO NOTHING
	`, id, ContractAuditEntityKind, row.EntityID, row.Action, row.Actor, row.OrgID, string(diff))
	return err
}

// NewAuditID is a fresh registry_audit_log id, for a row that no retry rewrites.
func NewAuditID() string {
	return "ral_" + uuid.NewString()
}

// ExpiredAuditID is the fixed id of a schedule's expiry row for one paused slot. The sweeper pauses a
// schedule once, so one slot has one expiry row, and a retried write lands on that row. The durable
// recovery recomputes the same id from the schedule's next_fire_at, which a pause leaves unchanged.
func ExpiredAuditID(scheduleID, fireKey string) string {
	return "ral_expired_" + digestHex(scheduleID, fireKey)
}

// RefusedFireAuditID is the fixed id of the refusal row of one claimed task.
func RefusedFireAuditID(taskID string) string {
	return "ral_refused_" + digestHex(taskID)
}

func digestHex(parts ...string) string {
	h := sha256.New()
	for _, part := range parts {
		h.Write([]byte(part))
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil)[:16])
}

// ContractAuditDiff is the closed diff of a contract audit row. Each field is a digest, a count, a
// canonical time or a closed token, and the field names are fixed.
type ContractAuditDiff struct {
	ContractDigest string `json:"contract_digest,omitempty"`
	ToolCount      int    `json:"tool_count,omitempty"`
	ExpiresAt      string `json:"expires_at,omitempty"`
	Class          string `json:"class,omitempty"`
	Reason         string `json:"reason,omitempty"`
}

// ContractAuditDiffJSON is the content-free diff for one contract text. The digest is the one
// ContractDigest computes. The tool count, expiry and class are read only when the text parses, so a
// text that cannot be read says nothing about its tools. reason is the closed token of a refusal, and
// it is empty otherwise.
func ContractAuditDiffJSON(text, reason string) []byte {
	diff := ContractAuditDiff{Reason: reason}
	if text != "" {
		diff.ContractDigest = ContractDigest(text)
		if contract, err := ParseScheduleContract(text); err == nil {
			diff.ToolCount = len(contract.Tools)
			diff.ExpiresAt = contract.ExpiresAt
			diff.Class = ContractClassCeiling
		}
	}
	raw, err := json.Marshal(diff)
	if err != nil {
		return []byte("{}")
	}
	return raw
}

// ContractRefusal is a refused contract with the closed token its audit row carries. Its message is
// the refusal's own, so a caller that prints it sees no change.
type ContractRefusal struct {
	Token string
	Err   error
}

func (r *ContractRefusal) Error() string { return r.Err.Error() }

func (r *ContractRefusal) Unwrap() error { return r.Err }

// ContractRefusalToken is the closed token of a refusal. An error that names none is contract_invalid.
func ContractRefusalToken(err error) string {
	var refusal *ContractRefusal
	if errors.As(err, &refusal) && refusal.Token != "" {
		return refusal.Token
	}
	return ContractTokenInvalid
}

// ContractWindowToken names a window refusal: contract_expired once the expiry has passed, and
// window_invalid when the window runs past the maximum.
func ContractWindowToken(contract ScheduleContract, now time.Time) string {
	if expires, err := time.Parse(auditTimeLayout, contract.ExpiresAt); err == nil && !expires.After(now) {
		return ContractTokenExpired
	}
	return ContractTokenWindowInvalid
}

// ApprovalContractText is the approval_contract text a template carries. It is "" when there is none,
// or when it is not text.
func ApprovalContractText(template []byte) string {
	var fields struct {
		ApprovalContract string `json:"approval_contract"`
	}
	if err := json.Unmarshal(template, &fields); err != nil {
		return ""
	}
	return fields.ApprovalContract
}

// ContractAuditRetry holds the contract audit rows whose first append failed. The next sweep tick writes
// the sweeper's rows again, and the next executor tick the dispatcher's. Each row has a fixed id, so a row
// that lands twice still lands once. The queue is bounded, and past its limit the oldest row is dropped. A
// dropped expiry row is then written by the durable recovery. The zero value is ready to use, and a nil
// queue does nothing.
type ContractAuditRetry struct {
	mu      sync.Mutex
	pending []ContractAuditRow
}

// Add queues one row for the next flush.
func (q *ContractAuditRetry) Add(row ContractAuditRow) {
	if q == nil {
		return
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.pending) >= contractAuditRetryLimit {
		slog.Warn("contract audit retry queue is full; the oldest row was dropped", "action", q.pending[0].Action)
		q.pending = q.pending[1:]
	}
	q.pending = append(q.pending, row)
}

// Pending is the number of rows waiting for the next flush.
func (q *ContractAuditRetry) Pending() int {
	if q == nil {
		return 0
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	return len(q.pending)
}

// Flush writes every queued row through db again. A row that fails again stays queued, in order.
func (q *ContractAuditRetry) Flush(ctx context.Context, auditor ContractAuditor, db AuditExecer) {
	if q == nil || auditor == nil {
		return
	}
	q.mu.Lock()
	pending := q.pending
	q.pending = nil
	q.mu.Unlock()
	var failed []ContractAuditRow
	for _, row := range pending {
		if err := auditor.Append(ctx, db, row); err != nil {
			slog.Warn("contract audit row retry failed", "action", row.Action, "error", err)
			failed = append(failed, row)
		}
	}
	if len(failed) == 0 {
		return
	}
	q.mu.Lock()
	defer q.mu.Unlock()
	q.pending = append(failed, q.pending...)
	if over := len(q.pending) - contractAuditRetryLimit; over > 0 {
		q.pending = q.pending[over:]
	}
}
