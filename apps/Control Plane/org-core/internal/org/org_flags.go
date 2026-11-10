package org

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Organisation flag values (W5-079-b, F-t3-079). An organisation may set its own value
// for a short, closed list of presentation flags. The list lives here because org-core
// is its own Go module, so every name here must also be registered in
// apps/Control Plane/flag-registry/flag-registry.json with orgOverridable: true. Keep the
// two in sync by hand until the registry check exists.
//
// A value narrows a flag or enables it, but only within the build or environment
// ceiling: the frontend reads the stored value beside its own build flag. No security
// gate reads these values, and every write is recorded in the audit outbox the way a
// web policy write is.

// OrgFlagKind is the shape of an org-overridable flag's value.
type OrgFlagKind string

const (
	// OrgFlagKindSwitch is "true" or "false".
	OrgFlagKindSwitch OrgFlagKind = "switch"
	// OrgFlagKindChoice is one of the flag's closed Values.
	OrgFlagKindChoice OrgFlagKind = "choice"
)

// OrgFlagDef is one org-overridable flag: its registry name, its shape, the closed
// values of a choice flag, and the default an organisation sees until it writes one.
type OrgFlagDef struct {
	Name    string
	Kind    OrgFlagKind
	Values  []string
	Default string
}

// OrgOverridableFlags is the closed list of flags an organisation may set. Every name
// is the registry name of a flag with orgOverridable: true. Keep it sorted by name.
var OrgOverridableFlags = []OrgFlagDef{
	{Name: "VITE_AGENT_RESULT_FEEDBACK", Kind: OrgFlagKindSwitch, Default: "false"},
	{Name: "VITE_CHAT_CHANGED_FILES", Kind: OrgFlagKindSwitch, Default: "false"},
	{Name: "VITE_CHAT_READER_POLISH", Kind: OrgFlagKindSwitch, Default: "false"},
}

var (
	// ErrUnknownOrgFlag reports a name outside OrgOverridableFlags.
	ErrUnknownOrgFlag = errors.New("unknown org flag")
	// ErrInvalidOrgFlagValue reports a value the flag's kind does not allow.
	ErrInvalidOrgFlagValue = errors.New("invalid org flag value")
)

// OrgFlagError is a refusal of a flag write, with the flag it concerns.
type OrgFlagError struct {
	Kind    error
	Name    string
	Message string
}

func (e *OrgFlagError) Error() string {
	if e.Message != "" {
		return e.Kind.Error() + ": " + e.Message
	}
	return e.Kind.Error()
}

func (e *OrgFlagError) Unwrap() error { return e.Kind }

// OrgFlagView is one organisation's state for one org-overridable flag: the value it
// reads (the stored value, or the default), and the revision and writer of the stored
// value. Revision is 0 and UpdatedBy and UpdatedAt are empty until the flag is written.
type OrgFlagView struct {
	Name      string
	Kind      OrgFlagKind
	Values    []string
	Default   string
	Value     string
	Stored    bool
	Revision  int64
	UpdatedBy string
	UpdatedAt string
}

// OrgFlagPutResult is the value and revision one write stored.
type OrgFlagPutResult struct {
	Name     string
	Value    string
	Revision int64
}

// orgFlagDef returns the definition of an org-overridable flag by its registry name.
func orgFlagDef(name string) (OrgFlagDef, bool) {
	for _, def := range OrgOverridableFlags {
		if def.Name == name {
			return def, true
		}
	}
	return OrgFlagDef{}, false
}

// normalise checks a value against the flag's kind and returns the canonical text that
// is stored. Surrounding space is ignored. A switch takes only "true" or "false", in
// lower case, so the stored text is never ambiguous. A choice takes only its Values.
func (def OrgFlagDef) normalise(raw string) (string, bool) {
	value := strings.TrimSpace(raw)
	switch def.Kind {
	case OrgFlagKindSwitch:
		if value == "true" || value == "false" {
			return value, true
		}
	case OrgFlagKindChoice:
		if slices.Contains(def.Values, value) {
			return value, true
		}
	}
	return "", false
}

// ListOrgFlags returns every org-overridable flag in OrgOverridableFlags order, each with
// the organisation's stored value or the default, and the stored revision. A missing or
// soft-deleted organisation is ErrNotFound.
func (s *Service) ListOrgFlags(ctx context.Context, orgID string) ([]OrgFlagView, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	stored, err := s.repo.ListOrgFlagValues(ctx, orgID)
	if err != nil {
		return nil, err
	}
	views := make([]OrgFlagView, 0, len(OrgOverridableFlags))
	for _, def := range OrgOverridableFlags {
		view := OrgFlagView{
			Name:    def.Name,
			Kind:    def.Kind,
			Values:  slices.Clone(def.Values),
			Default: def.Default,
			Value:   def.Default,
		}
		if row, ok := stored[def.Name]; ok {
			view.Value = row.Value
			view.Stored = true
			view.Revision = row.Revision
			view.UpdatedBy = row.UpdatedBy
			view.UpdatedAt = row.UpdatedAt.UTC().Format(time.RFC3339)
		}
		views = append(views, view)
	}
	return views, nil
}

// SetOrgFlag stores an organisation's value for one org-overridable flag. The name and
// the value are checked against OrgOverridableFlags first, and a refusal is an
// *OrgFlagError. The write then runs in one transaction that locks the organisation,
// moves the revision on and queues the audit event. Once committed it flushes the audit
// outbox once as best effort. A failed publish stays pending and never fails the write.
func (s *Service) SetOrgFlag(ctx context.Context, orgID, name, value, changedBy, actorRole string) (*OrgFlagPutResult, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("organization id is required")
	}
	def, ok := orgFlagDef(name)
	if !ok {
		return nil, &OrgFlagError{Kind: ErrUnknownOrgFlag, Name: name, Message: "not an org-overridable flag"}
	}
	canonical, ok := def.normalise(value)
	if !ok {
		return nil, &OrgFlagError{Kind: ErrInvalidOrgFlagValue, Name: name, Message: "value is not allowed for this flag"}
	}
	result, err := s.repo.PutOrgFlagValue(ctx, orgID, def.Name, canonical, changedBy, actorRole)
	if err != nil {
		return nil, err
	}
	if s.auditPublisher != nil {
		if _, err := s.FlushOrgFlagAuditOutbox(ctx, 100); err != nil {
			log.Printf("org-core: org flag %s for org %s: inline audit flush failed, left pending: %v", def.Name, orgID, err)
		}
	}
	return result, nil
}

// --- storage ---

// OrgFlagAuditEvent and OrgFlagAuditSubject are the durable audit row written for every
// flag write. audit-core ingests the subject as a Control event.
const (
	OrgFlagAuditEvent   = "org_flag_changed"
	OrgFlagAuditSubject = "verevon.audit.v2.control.org-core." + OrgFlagAuditEvent
)

// orgFlagAuditOutboxTable is a constant, never caller input: the claim and mark
// statements below are built with it.
const orgFlagAuditOutboxTable = "org_flag_audit_outbox"

// orgFlagStoredValue is one stored flag row.
type orgFlagStoredValue struct {
	Value     string
	Revision  int64
	UpdatedBy string
	UpdatedAt time.Time
}

// ListOrgFlagValues reads every stored flag row of one organisation in one RLS-scoped
// transaction. A missing or soft-deleted organisation is ErrNotFound.
func (r *Repository) ListOrgFlagValues(ctx context.Context, orgID string) (map[string]orgFlagStoredValue, error) {
	stored := make(map[string]orgFlagStoredValue)
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var one int
		if err := tx.QueryRow(ctx, `SELECT 1 FROM organizations WHERE id = $1 AND deleted_at IS NULL`, orgID).Scan(&one); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("check organization for org flags: %w", err)
		}
		rows, err := tx.Query(ctx, `
SELECT name, value, revision, updated_by, updated_at
FROM org_flag_values
WHERE org_id = $1`, orgID)
		if err != nil {
			return fmt.Errorf("query org flags: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var (
				name string
				row  orgFlagStoredValue
			)
			if err := rows.Scan(&name, &row.Value, &row.Revision, &row.UpdatedBy, &row.UpdatedAt); err != nil {
				return fmt.Errorf("scan org flags: %w", err)
			}
			stored[name] = row
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return stored, nil
}

// PutOrgFlagValue stores one flag value in one RLS-scoped transaction:
//
//  1. lock the organisation row, which serialises every writer of the organisation;
//  2. upsert the row, moving its revision on by one (a new row starts at 1);
//  3. enqueue the audit intent, all or nothing.
//
// name and value must already be checked against OrgOverridableFlags (the service does
// that). A missing or soft-deleted organisation is ErrNotFound.
func (r *Repository) PutOrgFlagValue(ctx context.Context, orgID, name, value, changedBy, actorRole string) (*OrgFlagPutResult, error) {
	if strings.TrimSpace(changedBy) == "" {
		return nil, fmt.Errorf("org flag actor is required")
	}
	var result *OrgFlagPutResult
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var lockedID string
		if err := tx.QueryRow(ctx, `
SELECT id FROM organizations WHERE id = $1 AND deleted_at IS NULL FOR UPDATE`, orgID).Scan(&lockedID); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("lock organization for org flag: %w", err)
		}
		var revision int64
		if err := tx.QueryRow(ctx, `
INSERT INTO org_flag_values (org_id, name, value, revision, updated_by, updated_at)
VALUES ($1, $2, $3, 1, $4, NOW())
ON CONFLICT (org_id, name) DO UPDATE
SET value = EXCLUDED.value,
    revision = org_flag_values.revision + 1,
    updated_by = EXCLUDED.updated_by,
    updated_at = EXCLUDED.updated_at
RETURNING revision`, orgID, name, value, changedBy).Scan(&revision); err != nil {
			return fmt.Errorf("write org flag: %w", err)
		}
		if err := enqueueOrgFlagAudit(ctx, tx, orgFlagAuditChange{
			OrgID: orgID, ActorID: changedBy, ActorRole: actorRole,
			Name: name, Value: value, Revision: revision,
		}); err != nil {
			return err
		}
		result = &OrgFlagPutResult{Name: name, Value: value, Revision: revision}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return result, nil
}

// --- audit outbox ---

type orgFlagAuditChange struct {
	OrgID     string
	ActorID   string
	ActorRole string
	Name      string
	Value     string
	Revision  int64
}

// orgFlagAuditPayload is content-free by construction: the flag name, its value (a
// switch or one of a choice's closed values, never free text), the revision and the
// actor. It carries no organisation content.
func orgFlagAuditPayload(change orgFlagAuditChange, occurredAt time.Time) (eventID string, payload map[string]any) {
	identity := sha256.Sum256([]byte(change.OrgID + "\x00" + change.Name + "\x00" + strconv.FormatInt(change.Revision, 10)))
	eventID = "orgflag:org-core:" + hex.EncodeToString(identity[:])
	return eventID, map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": change.OrgID, "user_id": change.ActorID, "actor_role": change.ActorRole,
		"plane": "control", "producer": "org-core", "event": OrgFlagAuditEvent,
		"subject": "org_flag:" + change.OrgID, "resource_id": change.OrgID, "outcome": "ok",
		"details": map[string]any{
			"name": change.Name, "value": change.Value, "revision": change.Revision,
		},
	}
}

func enqueueOrgFlagAudit(ctx context.Context, tx pgx.Tx, change orgFlagAuditChange) error {
	if strings.TrimSpace(change.ActorID) == "" || strings.TrimSpace(change.ActorRole) == "" {
		return fmt.Errorf("org flag audit actor and role are required")
	}
	eventID, payload := orgFlagAuditPayload(change, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode org flag audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO `+orgFlagAuditOutboxTable+` (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, change.OrgID, OrgFlagAuditSubject, body); err != nil {
		return fmt.Errorf("record org flag audit outbox: %w", err)
	}
	return nil
}

func (r *Repository) claimOrgFlagAuditOutbox(ctx context.Context, limit int) ([]GDPRAuditOutboxRow, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin org flag audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM `+orgFlagAuditOutboxTable+`
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE `+orgFlagAuditOutboxTable+` o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, GDPRAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim org flag audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]GDPRAuditOutboxRow, 0)
	for rows.Next() {
		var row GDPRAuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan org flag audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode org flag audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate org flag audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit org flag audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) markOrgFlagAuditPublished(ctx context.Context, eventID string) error {
	result, err := r.pool.Exec(ctx, `
UPDATE `+orgFlagAuditOutboxTable+`
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark org flag audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("org flag audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) markOrgFlagAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.pool.QueryRow(ctx, `
UPDATE `+orgFlagAuditOutboxTable+`
SET attempts = attempts + 1,
    processing_at = NULL,
    next_attempt_at = NOW() + LEAST(
      INTERVAL '5 minutes',
      INTERVAL '1 second' * power(2, LEAST(attempts, 8))
    ),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3,
    updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL
RETURNING dead_lettered_at IS NOT NULL`, eventID, GDPRAuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, fmt.Errorf("org flag audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record org flag audit publish failure: %w", err)
	}
	return deadLettered, nil
}

// FlushOrgFlagAuditOutbox publishes pending flag-change audit intents, the same loop
// FlushWebPolicyAuditOutbox runs. SetOrgFlag calls it once after a commit as best
// effort; a failed publish stays pending for a later flush.
func (s *Service) FlushOrgFlagAuditOutbox(ctx context.Context, limit int) (GDPRAuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return GDPRAuditFlushResult{}, fmt.Errorf("org flag audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("org flag audit publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("org flag audit outbox repository is unavailable")
	}
	rows, err := s.repo.claimOrgFlagAuditOutbox(ctx, limit)
	if err != nil {
		return GDPRAuditFlushResult{}, err
	}
	result := GDPRAuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.repo.markOrgFlagAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.repo.markOrgFlagAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}
