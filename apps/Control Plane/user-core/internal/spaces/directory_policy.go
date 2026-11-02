package spaces

import (
	"context"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

// Room visibility and the per-organization directory policy (WS04-S8, decision
// D-WS04-2).
//
// A room is 'private' (the default: never listed, not joinable) or
// 'org_public' (listed in the room directory and joinable by an active
// organization member). 'org_public' only works while the organization's row in
// space_directory_policies says org_public_rooms_enabled. There is NO default
// row: an organization without a row denies, and so does a row that says false.
// The policy is written by the Control workload only (operator path, ledgered in
// space_directory_policy_events); a room's own owners can never turn it on.
//
// Ids, booleans and counts only: no content, no model call, so a Zero Data
// Retention organization behaves identically.

// Visibility values of registered_spaces.visibility.
const (
	VisibilityPrivate   = "private"
	VisibilityOrgPublic = "org_public"
)

// CodeDirectoryDisabled is the typed refusal of an org_public change while the
// organization's directory policy is off or absent.
const CodeDirectoryDisabled = "directory_disabled"

// ValidVisibility reports whether v is one of the two closed visibility values.
func ValidVisibility(v string) bool {
	return v == VisibilityPrivate || v == VisibilityOrgPublic
}

// DirectoryPolicy is one organization's directory switch.
type DirectoryPolicy struct {
	OrgID                 string `json:"org_id"`
	OrgPublicRoomsEnabled bool   `json:"org_public_rooms_enabled"`
}

// Validate reports whether the policy names an organization.
func (p DirectoryPolicy) Validate() error {
	if strings.TrimSpace(p.OrgID) == "" {
		return fmt.Errorf("space directory policy requires an organization")
	}
	return nil
}

// UpsertDirectoryPolicy is deliberately Control-workload-only at the HTTP
// boundary. Absence of a row denies. Every attempted change appends operator
// evidence only when the stored value really changes, so a replay changes
// nothing (convergent). It reports whether the value changed.
func (r *Repository) UpsertDirectoryPolicy(ctx context.Context, policy DirectoryPolicy, actorPrincipalID string) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("space authority repository unavailable")
	}
	policy.OrgID = strings.TrimSpace(policy.OrgID)
	if err := policy.Validate(); err != nil {
		return false, err
	}
	if strings.TrimSpace(actorPrincipalID) == "" {
		return false, fmt.Errorf("space directory policy actor is required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin Space directory policy update: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	tag, err := tx.Exec(ctx, `
		INSERT INTO space_directory_policies (org_id, org_public_rooms_enabled)
		VALUES ($1,$2)
		ON CONFLICT (org_id) DO UPDATE SET
			org_public_rooms_enabled=EXCLUDED.org_public_rooms_enabled,
			updated_at=NOW()
		WHERE space_directory_policies.org_public_rooms_enabled IS DISTINCT FROM EXCLUDED.org_public_rooms_enabled`,
		policy.OrgID, policy.OrgPublicRoomsEnabled)
	if err != nil {
		return false, fmt.Errorf("upsert Space directory policy: %w", err)
	}
	changed := tag.RowsAffected() > 0
	if changed {
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_directory_policy_events (org_id, actor_principal_id, org_public_rooms_enabled)
			VALUES ($1,$2,$3)`, policy.OrgID, actorPrincipalID, policy.OrgPublicRoomsEnabled); err != nil {
			return false, fmt.Errorf("record Space directory policy audit: %w", err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit Space directory policy update: %w", err)
	}
	return changed, nil
}

// directoryEnabledTx reads the organization's switch inside a transaction.
// Absence of a row is false.
func directoryEnabledTx(ctx context.Context, tx pgx.Tx, orgID string) (bool, error) {
	var enabled bool
	err := tx.QueryRow(ctx, `
		SELECT COALESCE((SELECT org_public_rooms_enabled FROM space_directory_policies WHERE org_id=$1), FALSE)`,
		orgID).Scan(&enabled)
	if err != nil {
		return false, fmt.Errorf("read Space directory policy: %w", err)
	}
	return enabled, nil
}
