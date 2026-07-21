package org

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// ErrOrganizationNotPendingDeletion is returned by RestoreOrganization and
// MarkReminderSent when the target organization's deleted_at is already NULL
// (nothing to restore / no active grace window to remind about) — the "not
// found/not applicable" signal for the Flow C org-deletion ledger, matching
// the RowsAffected()==0 -> sentinel-error convention used elsewhere in this
// package (see e.g. SetInteractiveRetention's ErrNotFound).
var ErrOrganizationNotPendingDeletion = errors.New("organization is not pending deletion")

// deletionGracePeriodDays is Flow C's fixed 30-day soft-delete grace window:
// the HTTP soft-delete handler (DELETE /orgs/:id/gdpr/soft-delete, a separate
// build stage) publishes velion.org.deletion.pending with
// deadline = now + 30 days. This constant mirrors that fixed contract value
// for computing reminder thresholds and the returned Deadline. It is
// deliberately NOT wired to ORG_PURGE_DAYS (cmd/server/main.go's operator-
// tunable purge-cron knob, also defaulting to 30) — the two can drift under a
// nonstandard operator configuration, but reconciling them is a separate
// build stage; main.go is untouched here.
const deletionGracePeriodDays = 30

// CreateDeletionLedger records one org_deletion_members row per active
// member at soft-delete time (migrations/018_org_deletion_ledger.up.sql). It
// is idempotent — ON CONFLICT DO NOTHING per (org_id, user_id) — so
// re-triggering DELETE /orgs/:id/gdpr/soft-delete on an org that already has
// ledger rows (retry, at-least-once delivery) never errors and never resets
// an existing member's exported_at/acknowledged_at checkpoint.
//
// notified_at is stamped NOW() here rather than left NULL: ledger creation
// happens in the same soft-delete request that synchronously publishes the
// member's initial velion.org.deletion.pending notice, so "row created" and
// "member notified of the pending deletion" are the same event. There is no
// separate MarkNotified method in this contract.
func (r *Repository) CreateDeletionLedger(ctx context.Context, orgID string, memberUserIDs []string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("create deletion ledger: org id is required")
	}
	if len(memberUserIDs) == 0 {
		return nil
	}

	const q = `
INSERT INTO org_deletion_members (org_id, user_id, notified_at)
VALUES ($1, $2, NOW())
ON CONFLICT (org_id, user_id) DO NOTHING`

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		// Batch all inserts into one round-trip instead of N sequential Exec
		// calls (same pattern as SetDefaultEntitlements).
		batch := &pgx.Batch{}
		for _, userID := range memberUserIDs {
			batch.Queue(q, orgID, userID)
		}
		results := tx.SendBatch(ctx, batch)
		defer results.Close()
		for range memberUserIDs {
			if _, err := results.Exec(); err != nil {
				return fmt.Errorf("create deletion ledger row: %w", err)
			}
		}
		return nil
	})
}

// DeleteDeletionLedger removes every org_deletion_members row for orgID. It
// is the ledger half of restore: RestoreOrganization clears deleted_at, this
// clears the ledger. A no-op (zero rows affected) is not an error — restoring
// an org whose ledger was never created (or already cleared by a previous,
// retried restore) is exactly the idempotent behavior at-least-once delivery
// requires.
func (r *Repository) DeleteDeletionLedger(ctx context.Context, orgID string) error {
	const q = `DELETE FROM org_deletion_members WHERE org_id = $1`
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, q, orgID); err != nil {
			return fmt.Errorf("delete deletion ledger: %w", err)
		}
		return nil
	})
}

// markDeletionLedgerTimestamp stamps NOW() into one checkpoint column of a
// single (org_id, user_id) ledger row. column is always one of the two fixed,
// package-internal literals passed by MarkExported/MarkAcknowledged below
// (never caller/user input), so string-building the column name is safe.
func (r *Repository) markDeletionLedgerTimestamp(ctx context.Context, orgID, userID, column string) error {
	q := fmt.Sprintf(`
UPDATE org_deletion_members
SET %s = NOW()
WHERE org_id = $1 AND user_id = $2`, column)

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, q, orgID, userID)
		if err != nil {
			return fmt.Errorf("mark deletion ledger %s: %w", column, err)
		}
		if tag.RowsAffected() == 0 {
			return ErrNotFound
		}
		return nil
	})
}

// MarkExported sets org_deletion_members.exported_at = NOW() for the calling
// member's own row. Returns ErrNotFound if no ledger row exists for
// (orgID, userID) — e.g. the org isn't pending deletion, or userID wasn't an
// active member when the ledger was created.
func (r *Repository) MarkExported(ctx context.Context, orgID, userID string) error {
	return r.markDeletionLedgerTimestamp(ctx, orgID, userID, "exported_at")
}

// MarkAcknowledged sets org_deletion_members.acknowledged_at = NOW() for the
// calling member's own row. Returns ErrNotFound under the same conditions as
// MarkExported.
func (r *Repository) MarkAcknowledged(ctx context.Context, orgID, userID string) error {
	return r.markDeletionLedgerTimestamp(ctx, orgID, userID, "acknowledged_at")
}

// GetDeletionStatus returns the organization row for GET
// /orgs/:id/gdpr/deletion/status, WITHOUT filtering on deleted_at (unlike
// GetOrganization) — the whole point of the status endpoint is to answer
// during the pending-deletion window, when deleted_at IS NOT NULL. Reuses
// Organization instead of inventing a parallel "deletion status" shape: the
// caller reads .Name and .DeletedAt (nil DeletedAt means active / not
// pending). Returns ErrNotFound if orgID does not exist at all.
func (r *Repository) GetDeletionStatus(ctx context.Context, orgID string) (*Organization, error) {
	const q = `SELECT id, name, deleted_at FROM organizations WHERE id = $1`

	var out *Organization
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var org Organization
		if err := tx.QueryRow(ctx, q, orgID).Scan(&org.ID, &org.Name, &org.DeletedAt); err != nil {
			return err
		}
		out = &org
		return nil
	})
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("query org deletion status: %w", err)
	}
	return out, nil
}

// ListDeletionLedger returns every member's checkpoint row for orgID, used by
// GET /orgs/:id/gdpr/deletion/status's owner/admin "all members" array.
func (r *Repository) ListDeletionLedger(ctx context.Context, orgID string) ([]DeletionLedgerEntry, error) {
	const q = `
SELECT user_id, notified_at, exported_at, acknowledged_at
FROM org_deletion_members
WHERE org_id = $1
ORDER BY user_id ASC`

	var entries []DeletionLedgerEntry
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, q, orgID)
		if err != nil {
			return fmt.Errorf("list deletion ledger: %w", err)
		}
		defer rows.Close()

		for rows.Next() {
			var entry DeletionLedgerEntry
			if err := rows.Scan(
				&entry.UserID, &entry.NotifiedAt, &entry.ExportedAt, &entry.AcknowledgedAt,
			); err != nil {
				return fmt.Errorf("scan deletion ledger entry: %w", err)
			}
			entries = append(entries, entry)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return entries, nil
}

// RestoreOrganization clears deleted_at/status back to active for an org
// currently pending deletion. It touches only the organizations row — the
// caller is responsible for also calling DeleteDeletionLedger and publishing
// velion.org.deletion.cancelled (a separate build stage).
//
// Returns ErrOrganizationNotPendingDeletion (no-op, zero rows affected) if
// the org's deleted_at is already NULL — matching the RowsAffected()==0 ->
// sentinel-error convention used by SetInteractiveRetention/UpdatePlanWithOutbox
// elsewhere in repository.go. The HTTP layer maps this to the contract's 409.
func (r *Repository) RestoreOrganization(ctx context.Context, orgID string) error {
	const q = `
UPDATE organizations
SET deleted_at = NULL, status = 'active', updated_at = NOW()
WHERE id = $1 AND deleted_at IS NOT NULL`

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, q, orgID)
		if err != nil {
			return fmt.Errorf("restore organization: %w", err)
		}
		if tag.RowsAffected() == 0 {
			return ErrOrganizationNotPendingDeletion
		}
		return nil
	})
}

// MarkReminderSent stamps the corresponding organizations.deletion_reminder_*
// column so the reminder sweep never re-fires velion.org.deletion.reminder
// for the same org. which must be "7d" or "1d".
func (r *Repository) MarkReminderSent(ctx context.Context, orgID, which string) error {
	var column string
	switch which {
	case "7d":
		column = "deletion_reminder_7d_sent_at"
	case "1d":
		column = "deletion_reminder_1d_sent_at"
	default:
		return fmt.Errorf("mark reminder sent: unsupported reminder kind %q (want 7d or 1d)", which)
	}

	// column is one of the two fixed literals above (never caller input).
	q := fmt.Sprintf(`
UPDATE organizations
SET %s = NOW()
WHERE id = $1 AND deleted_at IS NOT NULL`, column)

	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, q, orgID)
		if err != nil {
			return fmt.Errorf("mark reminder sent: %w", err)
		}
		if tag.RowsAffected() == 0 {
			return ErrOrganizationNotPendingDeletion
		}
		return nil
	})
}

// ListOrgsNeeding7DayReminder finds organizations pending deletion whose
// 30-day deadline is 7 (or fewer, if the sweep missed a day) days out and
// that have not yet had their 7-day reminder sent.
func (r *Repository) ListOrgsNeeding7DayReminder(ctx context.Context) ([]OrgPendingDeletionReminder, error) {
	return r.listOrgsNeedingReminder(ctx, "deletion_reminder_7d_sent_at", 7)
}

// ListOrgsNeeding1DayReminder is ListOrgsNeeding7DayReminder's 1-day-out
// counterpart.
func (r *Repository) ListOrgsNeeding1DayReminder(ctx context.Context) ([]OrgPendingDeletionReminder, error) {
	return r.listOrgsNeedingReminder(ctx, "deletion_reminder_1d_sent_at", 1)
}

// listOrgsNeedingReminder is a genuinely cross-org lookup — like
// ListOrganizations and PurgeOldDeletedOrganizations, it intentionally runs
// unscoped on r.pool (never WithOrgScope) so the daily reminder sweep sees
// every pending-deletion org across every tenant, not just one. sentAtColumn
// is always one of the two fixed literals from the wrapper methods above
// (never caller input).
//
// An org qualifies once its grace-period elapsed time reaches
// (deletionGracePeriodDays - daysRemaining) days and its sent_at column is
// still NULL. There is deliberately no upper bound: if a sweep is ever
// skipped (downtime), the org still qualifies on the next run — the NULL
// sent_at guard is what makes each reminder fire exactly once, not a time
// window.
func (r *Repository) listOrgsNeedingReminder(
	ctx context.Context, sentAtColumn string, daysRemaining int,
) ([]OrgPendingDeletionReminder, error) {
	elapsedThresholdDays := deletionGracePeriodDays - daysRemaining

	q := fmt.Sprintf(`
SELECT id, name, deleted_at
FROM organizations
WHERE deleted_at IS NOT NULL
  AND %s IS NULL
  AND deleted_at <= NOW() - ($1::float8 * INTERVAL '1 day')
ORDER BY deleted_at ASC`, sentAtColumn)

	rows, err := r.pool.Query(ctx, q, elapsedThresholdDays)
	if err != nil {
		return nil, fmt.Errorf("list orgs needing reminder: %w", err)
	}
	defer rows.Close()

	var out []OrgPendingDeletionReminder
	for rows.Next() {
		var id, name string
		var deletedAt time.Time
		if err := rows.Scan(&id, &name, &deletedAt); err != nil {
			return nil, fmt.Errorf("scan org needing reminder: %w", err)
		}
		out = append(out, OrgPendingDeletionReminder{
			OrgID:    id,
			OrgName:  name,
			Deadline: deletedAt.AddDate(0, 0, deletionGracePeriodDays),
		})
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read orgs needing reminder: %w", err)
	}
	return out, nil
}
