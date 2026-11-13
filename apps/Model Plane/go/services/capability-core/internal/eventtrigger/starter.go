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

// fireStartWindow is how long after its authorisation a fire may still start a run. The hourly budgets count a fire in
// the window it was authorised in (fireWindow), so a run started after that window would run outside the budget that
// admitted it. A run switch turned on after a long pause must not start the runs of every event that piled up while it
// was off: a fire the starter reaches later is refused OutcomeFireExpired instead, and is never started.
const fireStartWindow = fireWindow

// FireKeyPrefix begins the fire key of every run a trigger starts: `trigger:<trigger id>:<webhook event id>`. A cron
// slot's key is its RFC 3339 time and Run now's begins `manual:`, so the three never collide. session-core stamps the
// run's origin from this prefix on the key it is bound to, never from a request field.
const FireKeyPrefix = "trigger:"

// The outcomes the starter adds to a fire it can never start. All are final, so the fire is not claimed again. A fire
// whose trigger is paused or disabled gets OutcomeTriggerNotActive (fire.go), the outcome the sweeper gives such a fire
// when it is still pending.
const (
	// OutcomeFireKeyInvalid marks a fire whose key could not be a scheduled-run identifier.
	OutcomeFireKeyInvalid = "fire_key_invalid"
	// OutcomeApprovalExpired marks a fire whose bound schedule's consent contract has lapsed.
	OutcomeApprovalExpired = "approval_expired"
	// OutcomeScheduleDisabled marks a fire whose bound schedule is switched off or auto-paused when the starter reaches
	// it. The fire is refused, not left authorised, so switching the schedule back on never starts a run for an event
	// that arrived while it was off.
	OutcomeScheduleDisabled = "schedule_disabled"
	// OutcomeFireExpired marks a fire the starter reaches more than fireStartWindow after its authorisation. The fire is
	// refused, not left authorised, so no later start runs for an event the budget that admitted it no longer covers.
	OutcomeFireExpired = "fire_expired"
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
	// next claims one authorised, unstarted fire, authorised no earlier than notBefore, whose trigger is active and names a
	// live, enabled schedule, skipping the fire ids in excluded. It returns nil when there is none.
	next(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error)
	// nextDisabled claims one authorised, unstarted fire under the same conditions as next, except that its schedule is
	// switched off or auto-paused. The caller refuses it. It returns nil when there is none.
	nextDisabled(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error)
	// nextInactive claims one authorised, unstarted fire of a paused or disabled trigger that names a schedule, skipping
	// the fire ids in excluded. The caller refuses it. It returns nil when there is none.
	nextInactive(ctx context.Context, excluded []string) (*StartCandidate, error)
	// nextExpired claims one authorised, unstarted fire of an active trigger that names a schedule, authorised before
	// notBefore, whatever its schedule's state, skipping the fire ids in excluded. The caller refuses it. It returns nil
	// when there is none.
	nextExpired(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error)
	// insert writes the scheduled-run task and its pending cron fire, and returns the task id.
	insert(ctx context.Context, candidate StartCandidate, fireKey string, now time.Time) (string, error)
	// markStarted records the task on the fire.
	markStarted(ctx context.Context, fireID, taskID string, now time.Time) error
	// refuse settles a fire that can never start.
	refuse(ctx context.Context, fireID, outcome string, now time.Time) error
	commit(ctx context.Context) error
	rollback(ctx context.Context)
}

// Starter starts one budgeted scheduled run for each authorised fire of a trigger that names a schedule. It refuses a
// fire whose schedule is switched off or auto-paused, a fire whose trigger is paused or disabled, and a fire authorised
// more than fireStartWindow earlier. It writes the same task and cron fire rows a cron fire writes (cron.InsertFire),
// under a fire key that carries the trigger and the webhook event, so the workflow dispatcher asks Control afresh and
// session-core prepares the run exactly as for a cron fire. The starter holds and passes on no credential. The run is
// bounded by the existing run budget and by the hourly trigger and org limits the sweeper already applied when it
// authorised the fire; the starter adds no budget of its own, and the window keeps each start inside the hour it was
// admitted in.
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
// due), whether it wrote a task, and the error that rolled the fire back. A fire it refuses is settled the same way and
// reports no task.
func (s *Starter) startOne(ctx context.Context, excluded []string) (string, bool, error) {
	b, err := s.ledger.begin(ctx)
	if err != nil {
		return "", false, err
	}
	defer b.rollback(ctx)

	// The clock and the window come first: a fire authorised before notBefore is outside the window it may still start in.
	now := s.now().UTC()
	notBefore := now.Add(-fireStartWindow)

	// nextInactive comes first, so a fire of a paused or disabled trigger is refused in the first pass that finds it, not
	// after the enabled fires ahead of it. Then next claims a fire of an active trigger whose schedule is enabled and whose
	// authorisation is inside the window, and nextDisabled one whose schedule is switched off or auto-paused. Last comes
	// nextExpired, so a backlog of expired fires never holds back a fire that can still start. The starter refuses the
	// inactive, disabled and expired ones.
	candidate, err := b.nextInactive(ctx, excluded)
	if err != nil {
		return "", false, err
	}
	triggerInactive := candidate != nil
	if candidate == nil {
		candidate, err = b.next(ctx, excluded, notBefore)
		if err != nil {
			return "", false, err
		}
	}
	scheduleDisabled := false
	if candidate == nil {
		candidate, err = b.nextDisabled(ctx, excluded, notBefore)
		if err != nil {
			return "", false, err
		}
		scheduleDisabled = candidate != nil
	}
	fireExpired := false
	if candidate == nil {
		candidate, err = b.nextExpired(ctx, excluded, notBefore)
		if err != nil {
			return "", false, err
		}
		fireExpired = candidate != nil
	}
	if candidate == nil {
		return "", false, nil
	}
	fail := func(err error) (string, bool, error) {
		return candidate.FireID, false, fmt.Errorf("start trigger %s fire %s: %w", candidate.TriggerID, candidate.FireID, err)
	}

	// refuseWith settles the claimed fire as refused with outcome, and commits that alone.
	refuseWith := func(outcome string) (string, bool, error) {
		if err := b.refuse(ctx, candidate.FireID, outcome, now); err != nil {
			return fail(err)
		}
		if err := b.commit(ctx); err != nil {
			return fail(err)
		}
		return candidate.FireID, false, nil
	}

	// The checks run in this order, as the sweeper checks the trigger first. The window comes second and needs nothing of
	// the schedule, so an expired fire is refused without reading its key, its consent or its schedule, which only a start
	// needs. The fire key, the consent and the schedule follow. The first that fails names the refusal.
	fireKey := FireKeyPrefix + candidate.TriggerID + ":" + candidate.WebhookEventID
	switch {
	case triggerInactive:
		return refuseWith(OutcomeTriggerNotActive)
	case fireExpired:
		return refuseWith(OutcomeFireExpired)
	case !validFireKey(fireKey):
		return refuseWith(OutcomeFireKeyInvalid)
	case cron.ApprovalContractLapsed(candidate.Schedule.Template, now):
		return refuseWith(OutcomeApprovalExpired)
	case scheduleDisabled:
		return refuseWith(OutcomeScheduleDisabled)
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

// next claims one authorised fire whose schedule is enabled, authorised no earlier than notBefore. The trigger must still be
// active and name a schedule that is live and enabled, in the fire's org, and the trigger member's own in the trigger's
// Space, which is what the binding was checked against. A fire that fails any of these is left authorised and untouched,
// except the three the starter refuses: one whose schedule is only switched off or auto-paused (nextDisabled), one whose
// trigger is paused or disabled (nextInactive), and one authorised before the window (nextExpired).
func (b pgStartBatch) next(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	return b.claim(ctx, excluded, true, notBefore)
}

// nextDisabled claims one authorised fire under the same conditions as next, except that its schedule is switched off or
// auto-paused. The starter refuses it, so switching that schedule back on never starts a run for the old event.
func (b pgStartBatch) nextDisabled(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	return b.claim(ctx, excluded, false, notBefore)
}

// nextInactive claims one authorised fire of a trigger that names a schedule and is paused or disabled. The starter
// refuses it. A paused or disabled trigger must not start its events' runs, so the fire is settled now rather than left
// authorised, where re-activating the trigger would start the old event's run. Only the trigger is read: a refused fire
// never starts, so the schedule's own state does not matter. A trigger that names no schedule starts nothing, so its
// fires are left as they are.
func (b pgStartBatch) nextInactive(ctx context.Context, excluded []string) (*StartCandidate, error) {
	if excluded == nil {
		// A NULL array would make the exclusion unknown and match nothing.
		excluded = []string{}
	}
	var c StartCandidate
	err := b.tx.QueryRow(ctx, `
		SELECT f.id, f.trigger_id, f.webhook_event_id
		FROM event_trigger_fires f
		JOIN event_triggers t ON t.id = f.trigger_id
		WHERE f.status = 'authorized' AND f.task_id IS NULL
		  AND NOT (f.id = ANY($1::text[]))
		  AND t.status <> 'active' AND t.schedule_id IS NOT NULL
		ORDER BY f.settled_at, f.id
		LIMIT 1
		FOR UPDATE OF f, t SKIP LOCKED`, excluded).Scan(&c.FireID, &c.TriggerID, &c.WebhookEventID)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("claim authorised fire of an inactive trigger: %w", err)
	}
	return &c, nil
}

// nextExpired claims one authorised fire of an active trigger that names a schedule, authorised before notBefore. It takes
// the fire whatever its schedule's state, since a refused fire never starts and needs only its trigger. The starter refuses
// it fire_expired, so a run switch turned on after a long pause never starts the run of an event the budget that admitted
// it no longer covers.
func (b pgStartBatch) nextExpired(ctx context.Context, excluded []string, notBefore time.Time) (*StartCandidate, error) {
	if excluded == nil {
		// A NULL array would make the exclusion unknown and match nothing.
		excluded = []string{}
	}
	var c StartCandidate
	err := b.tx.QueryRow(ctx, `
		SELECT f.id, f.trigger_id, f.webhook_event_id
		FROM event_trigger_fires f
		JOIN event_triggers t ON t.id = f.trigger_id
		WHERE f.status = 'authorized' AND f.task_id IS NULL
		  AND NOT (f.id = ANY($1::text[]))
		  AND t.status = 'active' AND t.schedule_id IS NOT NULL
		  AND f.settled_at < $2
		ORDER BY f.settled_at, f.id
		LIMIT 1
		FOR UPDATE OF f, t SKIP LOCKED`, excluded, notBefore).Scan(&c.FireID, &c.TriggerID, &c.WebhookEventID)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("claim authorised fire older than the start window: %w", err)
	}
	return &c, nil
}

// claim is the query behind next and nextDisabled. Both claim the same fires under the same conditions and differ only
// in the bound schedule's enabled state, so the two cannot drift apart. Both take only fires authorised no earlier than
// notBefore: an older fire is nextExpired's. The schedule and the trigger rows are locked with the fire, as the cron
// sweeper locks the schedules it fires and session-core locks a trigger it updates, so a switch-off or a pause either waits
// for the task to be written or is seen by the claim. A row another replica holds is skipped, so replicas never start one
// fire twice.
func (b pgStartBatch) claim(ctx context.Context, excluded []string, enabled bool, notBefore time.Time) (*StartCandidate, error) {
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
		  AND s.enabled = $2
		  AND f.settled_at >= $3
		  AND s.org_id = f.org_id
		  AND s.space_ref = t.space_ref
		  AND s.creator_subject_id = t.member_user_id
		ORDER BY f.settled_at, f.id
		LIMIT 1
		FOR UPDATE OF f, s, t SKIP LOCKED`, excluded, enabled, notBefore).Scan(
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
