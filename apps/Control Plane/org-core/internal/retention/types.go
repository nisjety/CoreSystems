// Package retention is org-core's retention policy and legal holds of record
// (EP-9 C1): how long an organisation keeps personal threads, and which org or
// thread holds are active.
//
// org-core stores, validates and answers. It deletes nothing: session-core asks
// the internal check route which threads its sweeper may consider and drops every
// held one (EP-9 C2, C3). The records hold ids, numbers, a hold reference and
// actor ids, never thread content, so they carry no retention class of their own
// and go with the organisation.
package retention

import "time"

const (
	// MinThreadDays and MaxThreadDays bound the thread window (D-C2). The clock is
	// the thread's last activity, so a thread in use is never past its window.
	MinThreadDays = 30
	MaxThreadDays = 3650

	// ScopeOrg holds everything the organisation retains; ScopeThread holds one
	// thread. An org hold's target is the organisation id itself.
	ScopeOrg    = "org"
	ScopeThread = "thread"

	// MaxCheckThreadIDs is the most thread ids one check call may name, so the
	// sweeper pages its candidates instead of sending an org's whole history.
	MaxCheckThreadIDs = 500

	// MaxThreadIDLength bounds a thread id; MaxHoldRefLength bounds the matter or
	// case reference a placer gives a hold.
	MaxThreadIDLength = 128
	MaxHoldRefLength  = 200

	// MaxListedHolds caps the admin list: every active hold plus the most recent
	// released ones up to this many rows.
	MaxListedHolds = 500

	// MaxBodyBytes bounds one request body. A check body of 500 ids of 128 bytes
	// fits well inside it.
	MaxBodyBytes = 96 << 10

	// AuditEvent and AuditSubject are the durable audit row every write leaves in
	// org_retention_audit_outbox (migration 031 pins the subject).
	AuditEvent   = "org_retention_changed"
	AuditSubject = "verevon.audit.v2.control.org-core." + AuditEvent

	// AuditMaxAttempts bounds publication retries before a row is dead-lettered,
	// as the other org-core outboxes do.
	AuditMaxAttempts = 8

	// CodeNothingRetained is the typed refusal of a thread hold on a thread whose
	// content was never retained (Zero Data Retention or a waived run): there is
	// nothing to hold, and the placer is told so instead of being given a hold that
	// protects nothing. ZDRFeature is its feature key in zdr-refusals.json.
	CodeNothingRetained = "legal_hold_nothing_retained"
	ZDRFeature          = "legal_hold"
)

// EnvEnabled is the switch for the whole feature. Off, the routes are not mounted.
const EnvEnabled = "ORG_RETENTION_POLICY_ENABLED"

// Enabled reads EnvEnabled strictly: only the literal "true" turns it on, so a
// typo or "1" leaves the routes unmounted.
func Enabled(getenv func(string) string) bool {
	return getenv(EnvEnabled) == "true"
}

// Policy is the organisation's retention record. A zero Revision means nothing
// was ever written. A nil ThreadDays means keep: no thread is ever past a window.
type Policy struct {
	OrganizationID string     `json:"organization_id"`
	ThreadDays     *int       `json:"thread_days"`
	Revision       int64      `json:"revision"`
	UpdatedBy      string     `json:"updated_by"`
	UpdatedAt      *time.Time `json:"updated_at"`
}

// PutRequest replaces the policy. ExpectedRevision is a pointer so a body that
// omits it is refused instead of reading as "first write". ThreadDays null or
// absent sets keep.
type PutRequest struct {
	ExpectedRevision *int64 `json:"expected_revision"`
	ThreadDays       *int   `json:"thread_days"`
}

// Hold is one legal hold. ReleasedAt is nil while the hold is active.
type Hold struct {
	ID             string     `json:"id"`
	OrganizationID string     `json:"organization_id"`
	Scope          string     `json:"scope"`
	TargetRef      string     `json:"target_ref"`
	HoldRef        string     `json:"hold_ref"`
	CreatedBy      string     `json:"created_by"`
	CreatedAt      time.Time  `json:"created_at"`
	ReleasedBy     *string    `json:"released_by"`
	ReleasedAt     *time.Time `json:"released_at"`
}

// Active reports whether the hold still holds.
func (h Hold) Active() bool { return h.ReleasedAt == nil }

// HoldList is the admin read: active holds first, then released ones, newest
// first within each.
type HoldList struct {
	OrganizationID string `json:"organization_id"`
	Holds          []Hold `json:"holds"`
}

// PlaceHoldRequest places a hold. ThreadID is required for a thread hold and
// refused for an org hold, whose target is the organisation.
type PlaceHoldRequest struct {
	Scope    string `json:"scope"`
	ThreadID string `json:"thread_id,omitempty"`
	HoldRef  string `json:"hold_ref"`
}

// PlaceResult is a placed hold and whether it was already active: placing a hold
// on a held target answers the existing hold and writes nothing.
type PlaceResult struct {
	Hold    Hold `json:"hold"`
	Existed bool `json:"existed"`
}

// CheckRequest is the sweeper's question: which of these threads are held.
type CheckRequest struct {
	ThreadIDs []string `json:"thread_ids"`
}

// CheckResult answers it at one policy revision. WindowDays nil means keep.
// OrgHeld true means every thread is held whatever HeldThreadIDs says.
// HeldThreadIDs is the subset of the asked ids under an active thread hold, in
// bytewise order.
type CheckResult struct {
	OrganizationID string   `json:"organization_id"`
	WindowDays     *int     `json:"window_days"`
	Revision       int64    `json:"revision"`
	OrgHeld        bool     `json:"org_held"`
	HeldThreadIDs  []string `json:"held_thread_ids"`
}
