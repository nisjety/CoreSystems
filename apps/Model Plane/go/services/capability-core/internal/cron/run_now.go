package cron

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"regexp"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// Run now (EP-6-S2) makes one manual fire of a schedule. It takes the path a cron slot
// takes and no other: a fresh Control fire decision before the task exists, then
// InsertFire, and the workflow dispatcher asks Control twice more before the Temporal
// handoff. Nothing here starts a workflow.

const (
	// ManualFireKeyPrefix marks a fire made by Run now. The rest of the key is the
	// caller's request id, which makes a retried request find its first fire.
	ManualFireKeyPrefix = "manual:"
	// runNowInFlightWindow is how long a pending or running manual fire blocks the next
	// one (OD-9). An older fire no longer blocks, so a fire whose run was lost cannot
	// keep the button refused for ever.
	runNowInFlightWindow = 15 * time.Minute
	// runNowHourlyLimit is the most manual fires of one schedule in a rolling hour (OD-9).
	// Each fire costs inference, and the creator's stored authority is spent on demand.
	runNowHourlyLimit = 6
)

// runNowRequestID is a client-generated id: a UUID in practice. The fire key carries it
// to Control and Session Core, whose scheduled-run decision refuses a "/" in a fire key.
var runNowRequestID = regexp.MustCompile(`^[A-Za-z0-9-]{16,64}$`)

// RunNowDB opens the one transaction a manual fire is made in. *pgxpool.Pool satisfies it.
type RunNowDB interface {
	Begin(context.Context) (pgx.Tx, error)
}

// RunNowRequest is one press of Run now. ActorID is the verified caller, never a value
// from the request body.
type RunNowRequest struct {
	OrgID      string
	ScheduleID string
	ActorID    string
	RequestID  string
}

// RunNowResult is the manual fire a request made or, for a retried request id, found.
type RunNowResult struct {
	FireID       string `json:"fire_id"`
	FireKey      string `json:"fire_key"`
	TaskID       string `json:"task_id"`
	Status       string `json:"status"`
	Deduplicated bool   `json:"deduplicated"`
}

// RunNowRefusal is a typed refusal: the HTTP status and the closed code the hub shows
// its copy for. Message is fixed text and never carries schedule content.
type RunNowRefusal struct {
	HTTPStatus int
	Code       string
	Message    string
}

func runNowRefused(status int, code, message string) *RunNowRefusal {
	return &RunNowRefusal{HTTPStatus: status, Code: code, Message: message}
}

// RunNow makes one manual fire inside one transaction that holds the schedule's row
// lock, so two presses are serialised and the sweeper skips the row for that tick. A
// refusal is returned as a RunNowRefusal; an error means the store or the transaction
// failed and nothing was written.
//
// Only the creator may press it (OD-1): every fire runs under the creator's subject, so
// anyone else would spend that person's authority at a moment they did not choose. The
// schedule's next_fire_at and last_fire_at are left alone: the cron slots fire as before.
func RunNow(ctx context.Context, db RunNowDB, authorizer FireAuthorizer, req RunNowRequest, now time.Time) (RunNowResult, *RunNowRefusal, error) {
	if !runNowRequestID.MatchString(req.RequestID) {
		return RunNowResult{}, runNowRefused(http.StatusBadRequest, "invalid_request_id",
			"request_id must be 16 to 64 letters, digits or hyphens"), nil
	}
	if authorizer == nil {
		return RunNowResult{}, nil, fmt.Errorf("run now fire authorizer is not configured")
	}
	tx, err := db.Begin(ctx)
	if err != nil {
		return RunNowResult{}, nil, err
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var d DueSchedule
	var expr, tz string
	err = tx.QueryRow(ctx, `
		SELECT id, org_id, space_ref, creator_subject_id, template_digest,
		       schedule_expr, timezone, task_template
		FROM cron_schedules
		WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL
		FOR UPDATE
	`, req.ScheduleID, req.OrgID).Scan(&d.ID, &d.OrgID, &d.SpaceRef, &d.SubjectID, &d.TemplateDigest,
		&expr, &tz, &d.Template)
	if errors.Is(err, pgx.ErrNoRows) {
		return RunNowResult{}, runNowRefused(http.StatusNotFound, "not_found", "schedule not found"), nil
	}
	if err != nil {
		return RunNowResult{}, nil, err
	}
	if req.ActorID == "" || req.ActorID != d.SubjectID {
		return RunNowResult{}, runNowRefused(http.StatusForbidden, "run_now_not_creator",
			"only the schedule's creator can run it now"), nil
	}
	if ApprovalContractLapsed(d.Template, now) {
		return RunNowResult{}, runNowRefused(http.StatusConflict, "approval_expired",
			"the schedule's approval has expired; renew it before running it"), nil
	}
	if _, err := NextFrom(expr, tz, now); err != nil {
		return RunNowResult{}, runNowRefused(http.StatusConflict, "invalid_schedule",
			"the schedule's cron expression is invalid"), nil
	}

	fireKey := ManualFireKeyPrefix + req.RequestID
	// A retried request finds the fire its first attempt made. The row lock serialises
	// writers, so the non-unique (schedule_id, fire_key) index is enough.
	existing := RunNowResult{FireKey: fireKey, Deduplicated: true}
	err = tx.QueryRow(ctx, `
		SELECT id, COALESCE(task_id, ''), status FROM cron_fires
		WHERE schedule_id=$1 AND fire_key=$2
		ORDER BY fired_at DESC, id DESC
		LIMIT 1
	`, d.ID, fireKey).Scan(&existing.FireID, &existing.TaskID, &existing.Status)
	if err == nil {
		return existing, nil, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return RunNowResult{}, nil, err
	}

	var inFlight, lastHour int
	if err := tx.QueryRow(ctx, `
		SELECT count(*) FILTER (WHERE status IN ('pending', 'running') AND fired_at > $3),
		       count(*) FILTER (WHERE fired_at > $4)
		FROM cron_fires
		WHERE schedule_id=$1 AND fire_key LIKE $2
	`, d.ID, ManualFireKeyPrefix+"%", now.Add(-runNowInFlightWindow), now.Add(-time.Hour)).Scan(&inFlight, &lastHour); err != nil {
		return RunNowResult{}, nil, err
	}
	if inFlight > 0 {
		return RunNowResult{}, runNowRefused(http.StatusConflict, "run_now_in_flight",
			"a manual run of this schedule is still in progress"), nil
	}
	if lastHour >= runNowHourlyLimit {
		return RunNowResult{}, runNowRefused(http.StatusTooManyRequests, "run_now_rate_limited",
			"this schedule has been run manually too often in the last hour"), nil
	}

	// The row lock is held across this one Control call, as the sweeper holds its batch;
	// the authorizer's HTTP client timeout bounds it.
	if err := authorizer.AuthorizeFire(ctx, FireIntent{
		OrgID: d.OrgID, SpaceRef: d.SpaceRef, SubjectID: d.SubjectID,
		ScheduleID: d.ID, FireKey: fireKey, TemplateDigest: d.TemplateDigest,
		IdempotencyKey: d.ID + ":" + fireKey,
	}); err != nil {
		slog.Warn("manual schedule fire refused by fresh authorization", "schedule", d.ID, "error", err)
		// Unlike the sweeper, a refused manual fire does not disable the schedule: the
		// refusal is about this press, not the next slot, which Control decides afresh.
		if _, err := tx.Exec(ctx,
			`INSERT INTO cron_fires (id, schedule_id, fired_at, status, error, fire_key) VALUES ($1,$2,$3,'failed',$4,$5)`,
			"cronfire_"+uuid.New().String(), d.ID, now, "fresh schedule authorization failed", fireKey); err != nil {
			return RunNowResult{}, nil, err
		}
		if err := tx.Commit(ctx); err != nil {
			return RunNowResult{}, nil, err
		}
		return RunNowResult{}, runNowRefused(http.StatusForbidden, "fire_not_authorized",
			"the schedule is not authorized to run now"), nil
	}

	fireID, taskID, err := InsertFire(ctx, tx, d, fireKey, now)
	if err != nil {
		return RunNowResult{}, nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return RunNowResult{}, nil, err
	}
	return RunNowResult{FireID: fireID, FireKey: fireKey, TaskID: taskID, Status: "pending"}, nil, nil
}
