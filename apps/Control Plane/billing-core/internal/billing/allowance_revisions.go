package billing

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/jackc/pgx/v5"
	"time"
)

func (r *Repository) ScheduleAllowanceRevision(ctx context.Context, org, metric string, revision AllowanceRevision, now time.Time) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	if err = lockAllowanceOrg(ctx, tx, org); err != nil {
		return err
	}
	var raw []byte
	if err = tx.QueryRow(ctx, `SELECT policy FROM billing_allowance_policies WHERE org_id=$1 AND metric=$2`, org, metric).Scan(&raw); errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	} else if err != nil {
		return err
	}
	var policy AllowancePolicy
	if err = json.Unmarshal(raw, &policy); err != nil {
		return err
	}
	if err = revision.validate(policy); err != nil {
		return err
	}
	body, err := json.Marshal(revision)
	if err != nil {
		return err
	}
	var same bool
	err = tx.QueryRow(ctx, `SELECT revision=$3::jsonb FROM billing_allowance_revisions WHERE org_id=$1 AND metric=$2 AND (version=$4 OR effective_at=$5)`, org, metric, body, revision.Version, revision.EffectiveAt).Scan(&same)
	if err == nil {
		if !same {
			return ErrAllowanceConflict
		}
		return tx.Commit(ctx)
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return err
	}
	if !revision.EffectiveAt.After(now) {
		return ErrAllowanceInvalid
	}
	// Append changes in chronological order so a retry cannot insert a new policy
	// between previously agreed future changes.
	var allowed bool
	if err = tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM billing_accounts WHERE org_id=$1 AND subscription_state IN ('active','trialing')) AND NOT EXISTS(SELECT 1 FROM billing_allowance_revisions WHERE org_id=$1 AND metric=$2 AND effective_at >= $3)`, org, metric, revision.EffectiveAt).Scan(&allowed); err != nil {
		return err
	}
	if !allowed {
		return ErrAllowanceConflict
	}
	if _, err = tx.Exec(ctx, `INSERT INTO billing_allowance_revisions(org_id,metric,version,effective_at,revision) VALUES($1,$2,$3,$4,$5::jsonb)`, org, metric, revision.Version, revision.EffectiveAt, body); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

func (s *Service) ScheduleAllowanceRevision(ctx context.Context, org, metric string, revision AllowanceRevision) error {
	return s.repo.ScheduleAllowanceRevision(ctx, org, metric, revision, time.Now().UTC())
}

func loadAllowanceRevisions(ctx context.Context, tx pgx.Tx, org, metric string) ([]AllowanceRevision, error) {
	rows, err := tx.Query(ctx, `SELECT revision FROM billing_allowance_revisions WHERE org_id=$1 AND metric=$2 ORDER BY effective_at LIMIT 1200`, org, metric)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := []AllowanceRevision{}
	for rows.Next() {
		var raw []byte
		if err = rows.Scan(&raw); err != nil {
			return nil, err
		}
		var revision AllowanceRevision
		if err = json.Unmarshal(raw, &revision); err != nil {
			return nil, err
		}
		result = append(result, revision)
	}
	return result, rows.Err()
}
