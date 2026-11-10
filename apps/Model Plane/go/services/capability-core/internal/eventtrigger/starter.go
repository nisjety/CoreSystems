package eventtrigger

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// DefaultStartInterval is how often the starter looks for authorised fires that have not started a run.
const DefaultStartInterval = 15 * time.Second

// defaultStartBatchSize is the most fires one pass starts. Each fire is its own transaction, so a slow or failing
// fire never holds the others.
const defaultStartBatchSize = 10

// FireKeyPrefix begins the fire key of every run a trigger starts: `trigger:<trigger id>:<webhook event id>`. A cron
// slot's key is its RFC 3339 time and Run now's begins `manual:`, so the three never collide. session-core stamps the
// run's origin from this prefix on the key it is bound to, never from a request field.
const FireKeyPrefix = "trigger:"

// The outcomes the starter adds to a fire it can never start. Both are final, so the fire is not claimed again.
const (
	// OutcomeFireKeyInvalid marks a fire whose key could not be a scheduled-run identifier.
	OutcomeFireKeyInvalid = "fire_key_invalid"
	// OutcomeApprovalExpired marks a fire whose bound schedule's consent contract has lapsed.
	OutcomeApprovalExpired = "approval_expired"
)

// maxFireKeyLen is the longest scheduled-run identifier session-core accepts.
const maxFireKeyLen = 256

// StartCandidate is one authorised fire with the schedule its trigger names.
type StartCandidate struct {
	FireID         string
	TriggerID      string
	WebhookEventID string
	Schedule       cron.DueSchedule
}

// starterLedger opens one transaction of start work.
type starterLedger interface {
	begin(ctx context.Context) (startBatch, error)
}

// startBatch is one transaction that starts at most one fire: the claim, the task, and the record of the task commit
// together, so a failure leaves the fire authorised and unstarted for the next pass.
type startBatch interface {
	// next claims one authorised, unstarted fire whose trigger is active and names a live schedule, skipping the fire
	// ids in excluded. It returns nil when there is none.
	next(ctx context.Context, excluded []string) (*StartCandidate, error)
	// insert writes the scheduled-run task and its pending cron fire, and returns the task id.
	insert(ctx context.Context, candidate StartCandidate, fireKey string, now time.Time) (string, error)
	// markStarted records the task on the fire.
	markStarted(ctx context.Context, fireID, taskID string, now time.Time) error
	// refuse settles a fire that can never start.
	refuse(ctx context.Context, fireID, outcome string, now time.Time) error
	commit(ctx context.Context) error
	rollback(ctx context.Context)
}

// Starter starts one budgeted scheduled run for each authorised fire of a trigger that names a schedule. It writes the
// same task and cron fire rows a cron fire writes (cron.InsertFire), under a fire key that carries the trigger and the
// webhook event, so the workflow dispatcher asks Control afresh and session-core prepares the run exactly as for a cron
// fire. The starter holds and passes on no credential. The run is bounded by the existing run budget and by the hourly
// trigger and org limits the sweeper already applied when it authorised the fire; the starter adds no cap of its own.
type Starter struct {
	ledger    starterLedger
	interval  time.Duration
	batchSize int
	now       func() time.Time
}

// NewStarter builds the starter over the fire table. It needs a database and nothing else: authority is decided by the
// sweeper before a fire is authorised, and again by Control when the dispatcher hands the task to the workflow.
func NewStarter(pool *pgxpool.Pool) (*Starter, error) {
	if pool == nil {
		return nil, errors.New("event trigger starter needs a database")
	}
	return newStarter(pgStarterStore{pool: pool}), nil
}

func newStarter(l starterLedger) *Starter {
	return &Starter{
		ledger:    l,
		interval:  DefaultStartInterval,
		batchSize: defaultStartBatchSize,
		now:       time.Now,
	}
}

// Start runs passes until ctx is cancelled. A failed pass is logged, not fatal.
func (s *Starter) Start(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	s.runAndLog(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.runAndLog(ctx)
		}
	}
}

func (s *Starter) runAndLog(ctx context.Context) {
	started, err := s.RunOnce(ctx)
	if err != nil {
		slog.Warn("event trigger start pass reported errors", "started", started, "error", err)
	}
	if started > 0 {
		slog.Info("event trigger start pass started runs", "count", started)
	}
}

// RunOnce starts up to one batch of authorised fires and returns how many tasks it wrote. A fire whose own writes fail
// is rolled back alone, stays authorised and unstarted, is skipped for the rest of the pass and is retried by the next
// one; the pass goes on to the other fires and names each failure in the returned error.
func (s *Starter) RunOnce(ctx context.Context) (int, error) {
	started := 0
	var failed []error
	excluded := []string{}
	for attempt := 0; attempt < s.batchSize; attempt++ {
		fireID, wrote, err := s.startOne(ctx, excluded)
		if err != nil {
			failed = append(failed, err)
			if fireID == "" {
				break
			}
			excluded = append(excluded, fireID)
			continue
		}
		if fireID == "" {
			break
		}
		if wrote {
			started++
		}
	}
	return started, errors.Join(failed...)
}

// startOne claims and starts one fire in its own transaction. It returns the fire id it touched ("" when none was
// due), whether it wrote a task, and the error that rolled the fire back.
func (s *Starter) startOne(ctx context.Context, excluded []string) (string, bool, error) {
	b, err := s.ledger.begin(ctx)
	if err != nil {
		return "", false, err
	}
	defer b.rollback(ctx)

	candidate, err := b.next(ctx, excluded)
	if err != nil {
		return "", false, err
	}
	if candidate == nil {
		return "", false, nil
	}
	now := s.now().UTC()
	fail := func(err error) (string, bool, error) {
		return candidate.FireID, false, fmt.Errorf("start trigger %s fire %s: %w", candidate.TriggerID, candidate.FireID, err)
	}

	fireKey := FireKeyPrefix + candidate.TriggerID + ":" + candidate.WebhookEventID
	if !validFireKey(fireKey) {
		if err := b.refuse(ctx, candidate.FireID, OutcomeFireKeyInvalid, now); err != nil {
			return fail(err)
		}
		if err := b.commit(ctx); err != nil {
			return fail(err)
		}
		return candidate.FireID, false, nil
	}
	if cron.ApprovalContractLapsed(candidate.Schedule.Template, now) {
		if err := b.refuse(ctx, candidate.FireID, OutcomeApprovalExpired, now); err != nil {
			return fail(err)
		}
		if err := b.commit(ctx); err != nil {
			return fail(err)
		}
		return candidate.FireID, false, nil
	}

	taskID, err := b.insert(ctx, *candidate, fireKey, now)
	if err != nil {
		return fail(err)
	}
	if err := b.markStarted(ctx, candidate.FireID, taskID, now); err != nil {
		return fail(err)
	}
	if err := b.commit(ctx); err != nil {
		return fail(err)
	}
	return candidate.FireID, true, nil
}

// validFireKey mirrors session-core's scheduled-run identifier rule: non-empty, trimmed, bounded, and free of the
// path and subject separators. A webhook event id outside it cannot be a fire key.
func validFireKey(key string) bool {
	return strings.TrimSpace(key) != "" &&
		key == strings.TrimSpace(key) &&
		len(key) <= maxFireKeyLen &&
		!strings.ContainsAny(key, "/.*> \t\r\n")
}

// pgStarterStore is the Postgres start path. It reads the session-core trigger table and the cron schedule table in the
// same database, as the sweeper and the cron sweeper do.
type pgStarterStore struct {
	pool *pgxpool.Pool
}

func (s pgStarterStore) begin(ctx context.Context) (startBatch, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin trigger fire start: %w", err)
	}
	return pgStartBatch{tx: tx}, nil
}

type pgStartBatch struct {
	tx pgx.Tx
}

// next claims one authorised fire. The trigger must still be active and name a schedule that is live, in the fire's
// org, and the trigger member's own in the trigger's Space, which is what the binding was checked against. A fire that
// fails any of these is left authorised and untouched. A row another replica holds is skipped, so replicas never start
// one fire twice.
func (b pgStartBatch) next(ctx context.Context, excluded []string) (*StartCandidate, error) {
	if excluded == nil {
		// A NULL array would make the exclusion unknown and match nothing.
		excluded = []string{}
	}
	var c StartCandidate
	err := b.tx.QueryRow(ctx, `
		SELECT f.id, f.trigger_id, f.webhook_event_id,
		       s.id, s.org_id, s.space_ref, s.creator_subject_id, s.template_digest, s.task_template
		FROM event_trigger_fires f
		JOIN event_triggers t ON t.id = f.trigger_id
		JOIN cron_schedules s ON s.id = t.schedule_id
		WHERE f.status = 'authorized' AND f.task_id IS NULL
		  AND NOT (f.id = ANY($1::text[]))
		  AND t.status = 'active' AND t.schedule_id IS NOT NULL
		  AND s.deleted_at IS NULL
		  AND s.org_id = f.org_id
		  AND s.space_ref = t.space_ref
		  AND s.creator_subject_id = t.member_user_id
		ORDER BY f.settled_at, f.id
		LIMIT 1
		FOR UPDATE OF f SKIP LOCKED`, excluded).Scan(
		&c.FireID, &c.TriggerID, &c.WebhookEventID,
		&c.Schedule.ID, &c.Schedule.OrgID, &c.Schedule.SpaceRef, &c.Schedule.SubjectID,
		&c.Schedule.TemplateDigest, &c.Schedule.Template)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("claim authorised trigger fire: %w", err)
	}
	return &c, nil
}

func (b pgStartBatch) insert(ctx context.Context, candidate StartCandidate, fireKey string, now time.Time) (string, error) {
	_, taskID, err := cron.InsertFire(ctx, b.tx, candidate.Schedule, fireKey, now)
	if err != nil {
		return "", fmt.Errorf("write trigger fire task: %w", err)
	}
	return taskID, nil
}

func (b pgStartBatch) markStarted(ctx context.Context, fireID, taskID string, now time.Time) error {
	tag, err := b.tx.Exec(ctx, `
		UPDATE event_trigger_fires
		SET task_id = $2, started_at = $3
		WHERE id = $1 AND status = 'authorized' AND task_id IS NULL`, fireID, taskID, now)
	if err != nil {
		return fmt.Errorf("record trigger fire task: %w", err)
	}
	if tag.RowsAffected() != 1 {
		return errors.New("record trigger fire task: the fire is no longer authorised and unstarted")
	}
	return nil
}

func (b pgStartBatch) refuse(ctx context.Context, fireID, outcome string, now time.Time) error {
	if _, err := b.tx.Exec(ctx, `
		UPDATE event_trigger_fires
		SET status = 'refused', outcome = $2, settled_at = $3
		WHERE id = $1 AND status = 'authorized' AND task_id IS NULL`, fireID, outcome, now); err != nil {
		return fmt.Errorf("refuse trigger fire: %w", err)
	}
	return nil
}

func (b pgStartBatch) commit(ctx context.Context) error {
	if err := b.tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit trigger fire start: %w", err)
	}
	return nil
}

func (b pgStartBatch) rollback(ctx context.Context) {
	_ = b.tx.Rollback(ctx)
}
