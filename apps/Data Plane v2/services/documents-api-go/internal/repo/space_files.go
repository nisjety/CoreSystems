package repo

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

// WS14-S3: room-file admission. A room file is a documents row with
// type = 'space_upload' and a space_ref taken from a verified Control Space
// import decision. Admission runs inside the transaction that writes the row, a
// NEW row or a same-key refresh of one, under a per-(org, Space) advisory lock,
// so the count, the byte sum and the hash lookup see every concurrent upload to
// the same Space.

// SpaceFileType is the documents.type of a room file.
const SpaceFileType = "space_upload"

// spaceFileSHAIndex is the unique index that backstops the hash lookup.
const spaceFileSHAIndex = "uq_documents_space_file_sha256"

// SpaceFileDuplicateError reports a live room file with the same content hash
// in the same Space. DocumentID names it when the lookup found it.
type SpaceFileDuplicateError struct {
	DocumentID string
}

func (e *SpaceFileDuplicateError) Error() string {
	return "room file with the same content already exists in this Space"
}

// SpaceFileQuotaError reports a per-Space limit. Limit is "max_files" or
// "max_text_bytes".
type SpaceFileQuotaError struct {
	Limit        string
	MaxFiles     int
	MaxTextBytes int64
}

func (e *SpaceFileQuotaError) Error() string {
	return "room file quota exceeded: " + e.Limit
}

// sameRoomFileKey reports whether an idempotency key that already names a live row
// may be reused for this write. A room file refreshes only a room file of the same
// Space, and an ordinary write never rewrites a room file: a key collision across
// either line is an ownership conflict, never a silent move of content between the
// two. An ordinary write keeps the same Space line too: it rewrites only a row with
// the same space_ref (both empty, or equal), so a key reused across Spaces, or
// between a Space-scoped row and an unscoped one, is a conflict as well.
func sameRoomFileKey(existing *model.Document, input model.CreateDocumentInput) bool {
	existingRoomFile := existing.Type == SpaceFileType
	if input.SpaceFile == nil {
		existingSpace := ""
		if existing.SpaceRef != nil {
			existingSpace = *existing.SpaceRef
		}
		return !existingRoomFile && existingSpace == input.SpaceRef
	}
	return existingRoomFile && existing.SpaceRef != nil && *existing.SpaceRef == input.SpaceRef
}

// checkSpaceFileAdmissionTx admits one room file inside the caller's transaction.
// refreshing names the row a same-key refresh rewrites in place, or "" for a new
// row. That row is left out of the count, the byte sum and the hash lookup: it
// already holds the Space's budget and its own hash, so the refresh is judged on
// the room the file leaves behind, not on a room that counts the file twice.
func checkSpaceFileAdmissionTx(ctx context.Context, tx pgx.Tx, input model.CreateDocumentInput, refreshing string) error {
	constraints := input.SpaceFile
	if constraints == nil {
		return nil
	}
	if input.OrgID == "" || input.SpaceRef == "" || input.Type != SpaceFileType {
		return errors.New("room file admission requires org, Space and type space_upload")
	}
	// One lock per (org, Space), released at commit or rollback.
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`,
		"space_files:"+input.OrgID+":"+input.SpaceRef); err != nil {
		return fmt.Errorf("lock room files: %w", err)
	}

	if constraints.SHA256 != "" {
		var existing string
		err := tx.QueryRow(ctx, `
			SELECT document_id FROM documents
			WHERE org_id = $1 AND space_ref = $2 AND type = 'space_upload'
			  AND deleted_at IS NULL AND metadata->>'sha256' = $3
			  AND document_id <> $4
			LIMIT 1`, input.OrgID, input.SpaceRef, constraints.SHA256, refreshing).Scan(&existing)
		if err == nil {
			return &SpaceFileDuplicateError{DocumentID: existing}
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("room file hash lookup: %w", err)
		}
	}

	if constraints.MaxFiles > 0 || constraints.MaxTextBytes > 0 {
		var files int
		var textBytes int64
		if err := tx.QueryRow(ctx, `
			SELECT count(*), COALESCE(sum(octet_length(content)), 0)
			FROM documents
			WHERE org_id = $1 AND space_ref = $2 AND type = 'space_upload'
			  AND deleted_at IS NULL AND document_id <> $3`, input.OrgID, input.SpaceRef, refreshing).Scan(&files, &textBytes); err != nil {
			return fmt.Errorf("room file quota count: %w", err)
		}
		if constraints.MaxFiles > 0 && files+1 > constraints.MaxFiles {
			return &SpaceFileQuotaError{Limit: "max_files", MaxFiles: constraints.MaxFiles, MaxTextBytes: constraints.MaxTextBytes}
		}
		if constraints.MaxTextBytes > 0 && textBytes+int64(len(input.Content)) > constraints.MaxTextBytes {
			return &SpaceFileQuotaError{Limit: "max_text_bytes", MaxFiles: constraints.MaxFiles, MaxTextBytes: constraints.MaxTextBytes}
		}
	}
	return nil
}
