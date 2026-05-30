package users

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// DocumentAcl represents a single ACL entry granting a user access to a document.
type DocumentAcl struct {
	AclID           string
	OrgID           string
	DocumentID      string
	UserID          string
	PermissionLevel string
	CreatedAt       time.Time
}

// AclRepository handles persistence of document ACL entries.
type AclRepository struct {
	db *database.DB
}

// NewAclRepository constructs an AclRepository using the shared database handle.
func NewAclRepository(db *database.DB) *AclRepository {
	return &AclRepository{db: db}
}

// Create inserts a new ACL entry.
func (r *AclRepository) Create(ctx context.Context, acl *DocumentAcl) error {
	_, err := r.db.Pool.Exec(ctx,
		`INSERT INTO document_acl (acl_id, org_id, document_id, user_id, permission_level)
		 VALUES ($1, $2, $3, $4, $5)`,
		acl.AclID, acl.OrgID, acl.DocumentID, acl.UserID, acl.PermissionLevel,
	)
	if err != nil {
		return fmt.Errorf("failed to create document acl: %w", err)
	}
	return nil
}

// Delete removes an ACL entry by its primary key.
func (r *AclRepository) Delete(ctx context.Context, aclID string) error {
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM document_acl WHERE acl_id = $1`,
		aclID,
	)
	if err != nil {
		return fmt.Errorf("failed to delete document acl: %w", err)
	}
	if result.RowsAffected() == 0 {
		return fmt.Errorf("document acl not found: %w", pgx.ErrNoRows)
	}
	return nil
}

// GetByID retrieves a single ACL entry by its primary key.
func (r *AclRepository) GetByID(ctx context.Context, aclID string) (*DocumentAcl, error) {
	row := r.db.Pool.QueryRow(ctx,
		`SELECT acl_id, org_id, document_id, user_id, permission_level, created_at
		 FROM document_acl WHERE acl_id = $1`,
		aclID,
	)
	acl := &DocumentAcl{}
	err := row.Scan(&acl.AclID, &acl.OrgID, &acl.DocumentID, &acl.UserID, &acl.PermissionLevel, &acl.CreatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("document acl not found: %w", pgx.ErrNoRows)
		}
		return nil, fmt.Errorf("failed to get document acl: %w", err)
	}
	return acl, nil
}

// ListByDocument returns all ACL entries for a given document within an org.
func (r *AclRepository) ListByDocument(ctx context.Context, orgID, documentID string) ([]*DocumentAcl, error) {
	rows, err := r.db.Pool.Query(ctx,
		`SELECT acl_id, org_id, document_id, user_id, permission_level, created_at
		 FROM document_acl WHERE org_id = $1 AND document_id = $2
		 ORDER BY created_at ASC`,
		orgID, documentID,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to list document acl: %w", err)
	}
	defer rows.Close()

	var entries []*DocumentAcl
	for rows.Next() {
		acl := &DocumentAcl{}
		if err := rows.Scan(&acl.AclID, &acl.OrgID, &acl.DocumentID, &acl.UserID, &acl.PermissionLevel, &acl.CreatedAt); err != nil {
			return nil, fmt.Errorf("failed to scan document acl row: %w", err)
		}
		entries = append(entries, acl)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("failed to iterate document acl rows: %w", err)
	}
	return entries, nil
}

// HasAccess reports whether a user holds any ACL entry for a document within an org.
func (r *AclRepository) HasAccess(ctx context.Context, orgID, documentID, userID string) (bool, error) {
	var exists bool
	err := r.db.Pool.QueryRow(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM document_acl
			WHERE org_id = $1 AND document_id = $2 AND user_id = $3
		)`,
		orgID, documentID, userID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("failed to check document acl: %w", err)
	}
	return exists, nil
}
// DeleteByUser removes the ACL entry matching (org, document, user).
func (r *AclRepository) DeleteByUser(ctx context.Context, orgID, documentID, userID string) error {
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM document_acl WHERE org_id=$1 AND document_id=$2 AND user_id=$3`,
		orgID, documentID, userID,
	)
	if err != nil {
		return fmt.Errorf("failed to delete document acl: %w", err)
	}
	if result.RowsAffected() == 0 {
		return fmt.Errorf("document acl not found: %w", pgx.ErrNoRows)
	}
	return nil
}

// GetByUser retrieves the ACL entry for a specific (org, document, user) tuple.
func (r *AclRepository) GetByUser(ctx context.Context, orgID, documentID, userID string) (*DocumentAcl, error) {
	row := r.db.Pool.QueryRow(ctx,
		`SELECT acl_id, org_id, document_id, user_id, permission_level, created_at
		 FROM document_acl WHERE org_id=$1 AND document_id=$2 AND user_id=$3`,
		orgID, documentID, userID,
	)
	acl := &DocumentAcl{}
	err := row.Scan(&acl.AclID, &acl.OrgID, &acl.DocumentID, &acl.UserID, &acl.PermissionLevel, &acl.CreatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("document acl not found: %w", pgx.ErrNoRows)
		}
		return nil, fmt.Errorf("failed to get document acl: %w", err)
	}
	return acl, nil
}