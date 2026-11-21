package sessionreview

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// reviewCounterSweepInterval is how often the sweeper deletes the counters that expired a day earlier.
const reviewCounterSweepInterval = time.Hour

// reviewCounterDayLayout is the UTC day a counter key ends with, as the review writes it.
const reviewCounterDayLayout = "2006-01-02"

// incrReviewCounterSQL adds one to a counter in a single statement, so replicas share one count without a lock of
// their own. An insert that meets a live row updates that row as it now stands, and a row whose window has passed
// starts again at one with a new window of $3 seconds.
const incrReviewCounterSQL = `
INSERT INTO learning_review_counters (key, org_id, count, expires_at)
VALUES ($1, $2, 1, now() + make_interval(secs => $3))
ON CONFLICT (key) DO UPDATE SET
    count = CASE WHEN learning_review_counters.expires_at <= now()
                 THEN 1 ELSE learning_review_counters.count + 1 END,
    expires_at = CASE WHEN learning_review_counters.expires_at <= now()
                      THEN now() + make_interval(secs => $3)
                      ELSE learning_review_counters.expires_at END
RETURNING count`

// sweepReviewCountersSQL deletes the counters that expired more than a day ago. A counter is read only until its
// window ends, so a row this old can no longer change any cap.
const sweepReviewCountersSQL = `DELETE FROM learning_review_counters WHERE expires_at < now() - interval '1 day'`

// errReviewCounterKey refuses a key that is not one of the two shapes the review builds. It does not repeat the key:
// the key carries identifiers, and the caller has nothing to learn from them.
var errReviewCounterKey = errors.New("sessionreview: review counter key has an unknown shape")

// PgReviewBudget counts the day's learning reviews of each owner and each organization in capability-core's own
// database (learning_review_counters, migration 0045). It holds ids and counts only, and it is the ReviewBudget the
// day caps need in order to be enforced across replicas. Build one with NewPgReviewBudget.
type PgReviewBudget struct {
	pool *pgxpool.Pool
}

// NewPgReviewBudget returns the Postgres counter store over pool and starts its sweeper, which runs until ctx is
// done. cmd/main.go builds the store only when the cost controls are on (see CostControlsFromLookup).
func NewPgReviewBudget(ctx context.Context, pool *pgxpool.Pool) *PgReviewBudget {
	budget := &PgReviewBudget{pool: pool}
	if pool != nil {
		go budget.sweepUntilDone(ctx)
	}
	return budget
}

// Incr adds one to the counter key and returns the count including it. A count starts when its first increment
// arrives and expires ttl later, and a count whose expiry has passed starts again at one. Only the two keys the
// review builds are accepted: any other key is an error, so no bug can create a counter that no organization owns.
func (b *PgReviewBudget) Incr(ctx context.Context, key string, ttl time.Duration) (int64, error) {
	org, err := organizationOfReviewCounterKey(key)
	if err != nil {
		return 0, err
	}
	if ttl <= 0 {
		return 0, errors.New("sessionreview: review counter window must be positive")
	}
	if b.pool == nil {
		return 0, errors.New("sessionreview: review counter store has no database")
	}
	var count int64
	if err := b.pool.QueryRow(ctx, incrReviewCounterSQL, key, org, ttl.Seconds()).Scan(&count); err != nil {
		return 0, fmt.Errorf("sessionreview: increment review counter: %w", err)
	}
	return count, nil
}

// sweepUntilDone deletes the expired counters every reviewCounterSweepInterval until ctx is done. A failed sweep is
// logged and retried at the next tick. An unswept row costs nothing: an expired row restarts at one if its key is
// counted again, and a later day uses new keys.
func (b *PgReviewBudget) sweepUntilDone(ctx context.Context) {
	ticker := time.NewTicker(reviewCounterSweepInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if _, err := b.sweep(ctx); err != nil && ctx.Err() == nil {
				slog.Warn("learning review counter sweep failed", "error", err)
			}
		}
	}
}

// sweep deletes the counters that expired more than a day ago and returns how many it deleted.
func (b *PgReviewBudget) sweep(ctx context.Context) (int64, error) {
	if b.pool == nil {
		return 0, errors.New("sessionreview: review counter store has no database")
	}
	tag, err := b.pool.Exec(ctx, sweepReviewCountersSQL)
	if err != nil {
		return 0, fmt.Errorf("sessionreview: sweep review counters: %w", err)
	}
	return tag.RowsAffected(), nil
}

// organizationOfReviewCounterKey returns the organization a counter key belongs to. It accepts exactly the two
// shapes the review writes: learning-review:user:<org>:<user>:<day> and learning-review:org:<org>:<day>. The
// organization and the user must be non-empty and the day must be a UTC date. Splitting on colons makes the shapes
// unambiguous, so an identifier that itself carried a colon is refused rather than read as another organization.
func organizationOfReviewCounterKey(key string) (string, error) {
	parts := strings.Split(key, ":")
	var org, user, day string
	switch {
	case len(parts) == 5 && parts[0] == "learning-review" && parts[1] == "user":
		org, user, day = parts[2], parts[3], parts[4]
	case len(parts) == 4 && parts[0] == "learning-review" && parts[1] == "org":
		org, day = parts[2], parts[3]
	default:
		return "", errReviewCounterKey
	}
	if org == "" || (len(parts) == 5 && user == "") {
		return "", errReviewCounterKey
	}
	if _, err := time.Parse(reviewCounterDayLayout, day); err != nil {
		return "", errReviewCounterKey
	}
	return org, nil
}
