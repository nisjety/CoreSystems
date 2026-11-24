package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// DM creator erasure (D22o, DEC-WS12-Q5, F-t1-016).
//
// When a person is erased under GDPR, every direct message and group DM they
// are in keeps existing for the others: the Space stays registered, its
// participant_hash and owner_principal_id stay as registered (Application's
// lifecycle events are matched against them), and the peers keep their own
// memberships, so their messages stay readable. Only the erased person leaves
// the fixed roster. Erasure is author-level in Session Core: the erased
// person's own messages go there, never the thread or the peer's messages.
//
// The erased person's roster rows are deleted (no raw id is kept), their
// history-window rows in those DMs go with them, the Space's authority and
// membership revisions advance so a cached decision or audience snapshot is
// stale, and one ledger row is appended per DM with the erased id replaced by
// the same pseudonym the ledger erasure uses. That row is written already
// published: Session Core and Application learn of an erasure from the GDPR
// fan-out, not from a membership event about a pseudonym.

// DMErasureActorID is the service actor of an erasure ledger row.
const DMErasureActorID = "user-core-gdpr-erasure"

// DMErasureResult counts what EraseDMParticipantTx changed.
type DMErasureResult struct {
	// Spaces is the number of DMs and group DMs the person left.
	Spaces int
}

type dmErasureRow struct {
	spaceRef, orgID, role string
	active                bool
}

// EraseDMParticipantTx removes userID from the roster of every DM and group DM
// inside tx, the erasure saga's local-stage transaction. pseudonym is the
// ledger pseudonym the saga already uses for this person. It is idempotent: a
// re-run after commit finds no rows and changes nothing. It never touches a
// non-DM Space.
func EraseDMParticipantTx(ctx context.Context, tx pgx.Tx, userID, pseudonym string) (DMErasureResult, error) {
	userID, pseudonym = strings.TrimSpace(userID), strings.TrimSpace(pseudonym)
	if userID == "" || pseudonym == "" || pseudonym == userID {
		return DMErasureResult{}, fmt.Errorf("erased user and a distinct pseudonym are required")
	}
	rows, err := tx.Query(ctx, `
		SELECT m.space_ref, s.org_id, m.role, m.active
		FROM space_memberships m
		JOIN registered_spaces s ON s.space_ref = m.space_ref
		WHERE m.subject_type = 'user' AND m.subject_id = $1
		  AND s.space_kind IN ('dm', 'group_dm')
		ORDER BY m.space_ref
		FOR UPDATE OF m`, userID)
	if err != nil {
		return DMErasureResult{}, fmt.Errorf("read direct message rosters to erase: %w", err)
	}
	var found []dmErasureRow
	for rows.Next() {
		var row dmErasureRow
		if err := rows.Scan(&row.spaceRef, &row.orgID, &row.role, &row.active); err != nil {
			rows.Close()
			return DMErasureResult{}, fmt.Errorf("scan direct message roster: %w", err)
		}
		found = append(found, row)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return DMErasureResult{}, fmt.Errorf("read direct message rosters to erase: %w", err)
	}

	result := DMErasureResult{}
	for _, row := range found {
		if _, err := tx.Exec(ctx, `
			DELETE FROM space_memberships
			WHERE space_ref = $1 AND subject_type = 'user' AND subject_id = $2`,
			row.spaceRef, userID); err != nil {
			return result, fmt.Errorf("erase direct message roster row: %w", err)
		}
		if _, err := tx.Exec(ctx, `
			DELETE FROM space_member_history_windows WHERE space_ref = $1 AND subject_id = $2`,
			row.spaceRef, userID); err != nil {
			return result, fmt.Errorf("erase direct message history window: %w", err)
		}
		result.Spaces++
		if !row.active {
			// Already off the roster: nothing an audience or decision holds changes.
			continue
		}
		var seq int64
		err := tx.QueryRow(ctx, `
			UPDATE space_authority_revisions
			SET authority_revision = authority_revision + 1,
			    membership_revision = membership_revision + 1,
			    updated_at = NOW()
			WHERE space_ref = $1
			RETURNING authority_revision`, row.spaceRef).Scan(&seq)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return result, fmt.Errorf("advance direct message revision: %w", err)
		}
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_membership_events
				(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id,
				 subject_type, subject_id, from_role, published_at)
			VALUES ($1, $2, $3, $4, $5, $6, $7, 'user', $8, NULLIF($9, ''), NOW())`,
			"sme_"+uuid.NewString(), row.spaceRef, row.orgID, seq, ActionMemberRemoved,
			ActorKindService, DMErasureActorID, pseudonym, row.role); err != nil {
			return result, fmt.Errorf("append direct message erasure ledger row: %w", err)
		}
	}
	return result, nil
}
