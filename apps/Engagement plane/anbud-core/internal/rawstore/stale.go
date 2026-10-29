package rawstore

import (
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5/pgconn"
)

// sqlStateRunNotRunning is the SQLSTATE that evidence_run_guard() raises (migration
// 0005): object_not_in_prerequisite_state. The guard refuses an evidence row whose
// run_id does not name a run in state running.
const sqlStateRunNotRunning = "55000"

// staleOr maps the error of an evidence insert. When the database's run guard refused
// the row because its run is not running, the result wraps ErrStaleWrite and keeps the
// database error in the chain: its message names the table and the run id and carries
// no row data. Any other error comes back as it was.
func staleOr(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == sqlStateRunNotRunning {
		return fmt.Errorf("%w: %w", ErrStaleWrite, err)
	}
	return err
}
