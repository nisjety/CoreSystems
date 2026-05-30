package workspace

import (
	"context"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type Binding struct {
	OrgID       string
	WorkspaceID string
	CreatedBy   string
}

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

func (r *Repository) GetBinding(ctx context.Context, orgID string) (Binding, error) {
	var binding Binding
	err := r.pool.QueryRow(ctx, `
		SELECT org_id, workspace_id, COALESCE(created_by_user_id, '')
		FROM affine_core_workspace_bindings
		WHERE org_id = $1
	`, orgID).Scan(&binding.OrgID, &binding.WorkspaceID, &binding.CreatedBy)
	return binding, err
}

func (r *Repository) UpsertBinding(ctx context.Context, binding Binding) (Binding, error) {
	err := r.pool.QueryRow(ctx, `
		INSERT INTO affine_core_workspace_bindings (org_id, workspace_id, created_by_user_id)
		VALUES ($1, $2, NULLIF($3, ''))
		ON CONFLICT (org_id) DO UPDATE
		SET workspace_id = EXCLUDED.workspace_id,
		    created_by_user_id = COALESCE(EXCLUDED.created_by_user_id, affine_core_workspace_bindings.created_by_user_id),
		    updated_at = NOW()
		RETURNING org_id, workspace_id, COALESCE(created_by_user_id, '')
	`, binding.OrgID, binding.WorkspaceID, binding.CreatedBy).Scan(&binding.OrgID, &binding.WorkspaceID, &binding.CreatedBy)
	return binding, err
}

func IsNotFound(err error) bool {
	return err == pgx.ErrNoRows
}
