package learning

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"regexp"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

var idempotencyKeyPattern = regexp.MustCompile(`^[A-Za-z0-9._:-]{16,128}$`)

const maxAttemptEvents = 2000

type Attempt struct {
	ID           string     `json:"id"`
	ClassID      string     `json:"classId"`
	AssignmentID string     `json:"assignmentId"`
	Mode         string     `json:"mode"`
	Status       string     `json:"status"`
	StartedAt    time.Time  `json:"startedAt"`
	FinishedAt   *time.Time `json:"finishedAt,omitempty"`
}

type AttemptEvent struct {
	ID         string    `json:"id"`
	Sequence   int       `json:"sequence"`
	Type       string    `json:"type"`
	StepID     *string   `json:"stepId,omitempty"`
	OccurredAt time.Time `json:"occurredAt"`
}

type AttemptProgress struct {
	AttemptID                 string     `json:"attemptId"`
	LearnerID                 string     `json:"learnerId"`
	Mode                      string     `json:"mode"`
	Status                    string     `json:"status"`
	StartedAt                 time.Time  `json:"startedAt"`
	FinishedAt                *time.Time `json:"finishedAt,omitempty"`
	TotalSteps                int        `json:"totalSteps"`
	SelfReportedCompleteSteps int        `json:"selfReportedCompleteSteps"`
}

func HashIdempotencyKey(key string) (string, error) {
	key = strings.TrimSpace(key)
	if !idempotencyKeyPattern.MatchString(key) {
		return "", ErrInvalid
	}
	sum := sha256.Sum256([]byte(key))
	return hex.EncodeToString(sum[:]), nil
}

func requestFingerprint(parts ...string) string {
	sum := sha256.Sum256([]byte(strings.Join(parts, "\n")))
	return hex.EncodeToString(sum[:])
}

func validAttemptMode(mode string) bool {
	switch mode {
	case "guide", "demonstrate", "practice":
		return true
	default:
		return false
	}
}

func validAttemptEvent(eventType, stepID string) bool {
	switch eventType {
	case "step_started", "assistance_used", "learner_reported_complete", "blocked":
		return strings.TrimSpace(stepID) != "" && len(stepID) <= 80
	case "attempt_finished", "attempt_abandoned":
		return strings.TrimSpace(stepID) == ""
	default:
		return false
	}
}

func (r *Repository) CreateAttempt(ctx context.Context, orgID, classID, assignmentID, learnerID, mode, idempotencyHash string) (Attempt, bool, error) {
	var attempt Attempt
	created := false
	if !validAttemptMode(mode) {
		return attempt, false, ErrInvalid
	}
	fingerprint := requestFingerprint("create-attempt-v1", classID, assignmentID, mode)
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		role, err := activeClassRole(ctx, tx, orgID, classID, learnerID)
		if err != nil {
			return err
		}
		if role != "student" {
			return ErrForbidden
		}
		var verifiedAssignment string
		err = tx.QueryRow(ctx, `SELECT id FROM learning_assignments
WHERE org_id=$1 AND class_id=$2 AND id=$3`, orgID, classID, assignmentID).Scan(&verifiedAssignment)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}

		err = tx.QueryRow(ctx, `INSERT INTO learning_attempts
(org_id,class_id,assignment_id,learner_id,mode,idempotency_hash,request_hash)
VALUES($1,$2,$3,$4,$5,$6,$7)
ON CONFLICT (org_id,learner_id,idempotency_hash) DO NOTHING
RETURNING id,status,started_at,finished_at`,
			orgID, classID, assignmentID, learnerID, mode, idempotencyHash, fingerprint).
			Scan(&attempt.ID, &attempt.Status, &attempt.StartedAt, &attempt.FinishedAt)
		if err == nil {
			attempt.ClassID, attempt.AssignmentID, attempt.Mode = classID, assignmentID, mode
			created = true
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return err
		}
		var existingHash string
		err = tx.QueryRow(ctx, `SELECT id,class_id,assignment_id,mode,status,request_hash,started_at,finished_at
FROM learning_attempts WHERE org_id=$1 AND learner_id=$2 AND idempotency_hash=$3`,
			orgID, learnerID, idempotencyHash).
			Scan(&attempt.ID, &attempt.ClassID, &attempt.AssignmentID, &attempt.Mode, &attempt.Status, &existingHash, &attempt.StartedAt, &attempt.FinishedAt)
		if err != nil {
			return err
		}
		if existingHash != fingerprint {
			return ErrIdempotencyConflict
		}
		return nil
	})
	if err != nil {
		return Attempt{}, false, err
	}
	return attempt, created, nil
}

func (r *Repository) RecordAttemptEvent(ctx context.Context, orgID, classID, attemptID, learnerID, eventType, stepID, idempotencyHash string) (AttemptEvent, bool, error) {
	var event AttemptEvent
	stepID = strings.TrimSpace(stepID)
	if !validAttemptEvent(eventType, stepID) {
		return event, false, ErrInvalid
	}
	fingerprint := requestFingerprint("attempt-event-v1", eventType, stepID)
	created := false
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		var ownerID, status string
		err := tx.QueryRow(ctx, `SELECT learner_id,status FROM learning_attempts
WHERE org_id=$1 AND class_id=$2 AND id=$3 FOR UPDATE`, orgID, classID, attemptID).Scan(&ownerID, &status)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		role, err := activeClassRole(ctx, tx, orgID, classID, learnerID)
		if err != nil {
			return err
		}
		if role != "student" || ownerID != learnerID {
			return ErrForbidden
		}
		var storedHash string
		err = tx.QueryRow(ctx, `SELECT id,sequence,event_type,step_id,occurred_at,request_hash
FROM learning_attempt_events WHERE org_id=$1 AND attempt_id=$2 AND idempotency_hash=$3`,
			orgID, attemptID, idempotencyHash).
			Scan(&event.ID, &event.Sequence, &event.Type, &event.StepID, &event.OccurredAt, &storedHash)
		if err == nil {
			if storedHash != fingerprint {
				return ErrIdempotencyConflict
			}
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return err
		}
		if status != "in_progress" {
			return ErrConflict
		}
		if eventType != "attempt_finished" && eventType != "attempt_abandoned" {
			var validStep bool
			err := tx.QueryRow(ctx, `SELECT EXISTS (
  SELECT 1 FROM learning_attempts a
  JOIN learning_assignments assignment
    ON assignment.org_id=a.org_id AND assignment.class_id=a.class_id AND assignment.id=a.assignment_id
  JOIN learning_course_revisions revision
    ON revision.org_id=assignment.org_id AND revision.class_id=assignment.class_id
   AND revision.course_id=assignment.course_id AND revision.revision=assignment.course_revision
  CROSS JOIN LATERAL jsonb_array_elements(revision.content->'steps') AS step(value)
  WHERE a.org_id=$1 AND a.class_id=$2 AND a.id=$3 AND step.value->>'id'=$4
)`, orgID, classID, attemptID, stepID).Scan(&validStep)
			if err != nil {
				return err
			}
			if !validStep {
				return ErrInvalid
			}
		}
		var sequence int
		if err := tx.QueryRow(ctx, `SELECT COALESCE(MAX(sequence),0)+1
FROM learning_attempt_events WHERE org_id=$1 AND attempt_id=$2`, orgID, attemptID).Scan(&sequence); err != nil {
			return err
		}
		if sequence > maxAttemptEvents {
			return ErrAttemptLimit
		}
		var eventStepID any
		if eventType != "attempt_finished" && eventType != "attempt_abandoned" {
			eventStepID = stepID
		}
		err = tx.QueryRow(ctx, `INSERT INTO learning_attempt_events
(org_id,class_id,attempt_id,sequence,actor_id,event_type,step_id,idempotency_hash,request_hash)
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)
RETURNING id,sequence,event_type,step_id,occurred_at`,
			orgID, classID, attemptID, sequence, learnerID, eventType, eventStepID, idempotencyHash, fingerprint).
			Scan(&event.ID, &event.Sequence, &event.Type, &event.StepID, &event.OccurredAt)
		if err != nil {
			return err
		}
		created = true
		if eventType == "attempt_finished" || eventType == "attempt_abandoned" {
			terminalStatus := "finished"
			if eventType == "attempt_abandoned" {
				terminalStatus = "abandoned"
			}
			_, err = tx.Exec(ctx, `UPDATE learning_attempts
SET status=$4,finished_at=now() WHERE org_id=$1 AND class_id=$2 AND id=$3`,
				orgID, classID, attemptID, terminalStatus)
		}
		return err
	})
	if err != nil {
		return AttemptEvent{}, false, err
	}
	return event, created, nil
}

func (r *Repository) ListOwnAttempts(ctx context.Context, orgID, classID, assignmentID, learnerID string) ([]Attempt, error) {
	var attempts []Attempt
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		role, err := activeClassRole(ctx, tx, orgID, classID, learnerID)
		if err != nil {
			return err
		}
		if role != "student" {
			return ErrForbidden
		}
		rows, err := tx.Query(ctx, `SELECT id,class_id,assignment_id,mode,status,started_at,finished_at
FROM learning_attempts a WHERE org_id=$1 AND class_id=$2 AND assignment_id=$3 AND learner_id=$4
  AND EXISTS (SELECT 1 FROM learning_class_members active_member
    WHERE active_member.org_id=a.org_id AND active_member.class_id=a.class_id
      AND active_member.user_id=a.learner_id AND active_member.active AND active_member.role='student')
ORDER BY started_at DESC,id LIMIT 100`, orgID, classID, assignmentID, learnerID)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var attempt Attempt
			if err := rows.Scan(&attempt.ID, &attempt.ClassID, &attempt.AssignmentID, &attempt.Mode, &attempt.Status, &attempt.StartedAt, &attempt.FinishedAt); err != nil {
				return err
			}
			attempts = append(attempts, attempt)
		}
		return rows.Err()
	})
	if attempts == nil {
		attempts = []Attempt{}
	}
	return attempts, err
}

func (r *Repository) ListAttemptEvents(ctx context.Context, orgID, classID, attemptID, learnerID string) ([]AttemptEvent, error) {
	var events []AttemptEvent
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		var ownerID string
		err := tx.QueryRow(ctx, `SELECT learner_id FROM learning_attempts
WHERE org_id=$1 AND class_id=$2 AND id=$3`, orgID, classID, attemptID).Scan(&ownerID)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		role, err := activeClassRole(ctx, tx, orgID, classID, learnerID)
		if err != nil {
			return err
		}
		if role != "student" || ownerID != learnerID {
			return ErrForbidden
		}
		rows, err := tx.Query(ctx, `SELECT id,sequence,event_type,step_id,occurred_at
FROM learning_attempt_events WHERE org_id=$1 AND class_id=$2 AND attempt_id=$3
  AND EXISTS (SELECT 1 FROM learning_attempts attempt
    JOIN learning_class_members active_member
      ON active_member.org_id=attempt.org_id AND active_member.class_id=attempt.class_id
     AND active_member.user_id=attempt.learner_id AND active_member.active AND active_member.role='student'
    WHERE attempt.org_id=$1 AND attempt.class_id=$2 AND attempt.id=$3 AND attempt.learner_id=$4)
ORDER BY sequence LIMIT 2000`, orgID, classID, attemptID, learnerID)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var event AttemptEvent
			if err := rows.Scan(&event.ID, &event.Sequence, &event.Type, &event.StepID, &event.OccurredAt); err != nil {
				return err
			}
			events = append(events, event)
		}
		return rows.Err()
	})
	if events == nil {
		events = []AttemptEvent{}
	}
	return events, err
}

func (r *Repository) ListAssignmentProgress(ctx context.Context, orgID, classID, assignmentID, teacherID string) ([]AttemptProgress, error) {
	var progress []AttemptProgress
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := requireClassTeacher(ctx, tx, orgID, classID, teacherID); err != nil {
			return err
		}
		var assignmentExists bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM learning_assignments
WHERE org_id=$1 AND class_id=$2 AND id=$3)`, orgID, classID, assignmentID).Scan(&assignmentExists); err != nil {
			return err
		}
		if !assignmentExists {
			return ErrNotFound
		}
		rows, err := tx.Query(ctx, `SELECT a.id,a.learner_id,a.mode,a.status,a.started_at,a.finished_at,
       jsonb_array_length(revision.content->'steps') AS total_steps,
       count(DISTINCT event.step_id) FILTER (WHERE event.event_type='learner_reported_complete') AS self_reported_complete_steps
FROM learning_attempts a
JOIN learning_assignments assignment
  ON assignment.org_id=a.org_id AND assignment.class_id=a.class_id AND assignment.id=a.assignment_id
JOIN learning_course_revisions revision
  ON revision.org_id=assignment.org_id AND revision.class_id=assignment.class_id
 AND revision.course_id=assignment.course_id AND revision.revision=assignment.course_revision
JOIN learning_class_members member
  ON member.org_id=a.org_id AND member.class_id=a.class_id AND member.user_id=a.learner_id AND member.active AND member.role='student'
LEFT JOIN learning_attempt_events event ON event.org_id=a.org_id AND event.attempt_id=a.id
WHERE a.org_id=$1 AND a.class_id=$2 AND a.assignment_id=$3
  AND EXISTS (SELECT 1 FROM learning_class_members viewer
    WHERE viewer.org_id=a.org_id AND viewer.class_id=a.class_id
      AND viewer.user_id=$4 AND viewer.active AND viewer.role='teacher')
GROUP BY a.id,revision.content
ORDER BY a.started_at DESC,a.id LIMIT 500`, orgID, classID, assignmentID, teacherID)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var item AttemptProgress
			if err := rows.Scan(&item.AttemptID, &item.LearnerID, &item.Mode, &item.Status, &item.StartedAt, &item.FinishedAt, &item.TotalSteps, &item.SelfReportedCompleteSteps); err != nil {
				return err
			}
			progress = append(progress, item)
		}
		return rows.Err()
	})
	if progress == nil {
		progress = []AttemptProgress{}
	}
	return progress, err
}
