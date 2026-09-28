package mailtask

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// PostgresRepository is the durable Repository backing Service, storing rows
// in mail_task_addresses (migration 029_mail_task_addresses.up.sql).
type PostgresRepository struct {
	db *database.DB
}

// NewRepository wires a PostgresRepository against the shared pool.
func NewRepository(db *database.DB) *PostgresRepository {
	return &PostgresRepository{db: db}
}

func (r *PostgresRepository) ActiveForUser(ctx context.Context, orgID, userID string) (*Address, error) {
	row := r.db.Pool.QueryRow(ctx, `
		SELECT alias, org_id, user_id, created_at, revoked_at
		FROM mail_task_addresses
		WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
	`, orgID, userID)
	address, err := scanAddress(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return address, nil
}

func (r *PostgresRepository) Insert(ctx context.Context, address Address) error {
	_, err := r.db.Pool.Exec(ctx, `
		INSERT INTO mail_task_addresses (alias, org_id, user_id, created_at)
		VALUES ($1, $2, $3, $4)
	`, address.Alias, address.OrgID, address.UserID, address.CreatedAt)
	if err != nil {
		if isUniqueViolation(err) {
			return ErrAliasCollision
		}
		return err
	}
	return nil
}

func (r *PostgresRepository) RevokeActive(ctx context.Context, orgID, userID string, revokedAt time.Time) error {
	_, err := r.db.Pool.Exec(ctx, `
		UPDATE mail_task_addresses
		SET revoked_at = $3
		WHERE org_id = $1 AND user_id = $2 AND revoked_at IS NULL
	`, orgID, userID, revokedAt)
	return err
}

func (r *PostgresRepository) ByAlias(ctx context.Context, alias string) (*Address, error) {
	row := r.db.Pool.QueryRow(ctx, `
		SELECT alias, org_id, user_id, created_at, revoked_at
		FROM mail_task_addresses
		WHERE alias = $1
	`, alias)
	address, err := scanAddress(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return address, nil
}

func scanAddress(row pgx.Row) (*Address, error) {
	var address Address
	if err := row.Scan(&address.Alias, &address.OrgID, &address.UserID, &address.CreatedAt, &address.RevokedAt); err != nil {
		return nil, err
	}
	return &address, nil
}

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code == "23505"
	}
	return false
}
