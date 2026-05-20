package store

import (
	"context"
	_ "embed"

	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed schema.sql
var schemaSQL string

// Migrate applies the bundled SQL schema. The DDL is idempotent
// (everything uses `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT
// EXISTS`) so repeated invocations are safe.
//
// In Phase A this is called from `main` on startup. Once we adopt a
// real migration runner (e.g. tern or goose), this function will move
// behind that abstraction and become a no-op.
func Migrate(ctx context.Context, pool *pgxpool.Pool) error {
	_, err := pool.Exec(ctx, schemaSQL)
	return err
}
