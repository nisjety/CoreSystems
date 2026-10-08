package billing

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"time"

	"github.com/jackc/pgx/v5"
)

func lockAllowanceOrg(ctx context.Context, tx pgx.Tx, org string) error {
	if err := validateUsageDimension("org_id", org, 128, false); err != nil {
		return fmt.Errorf("%w: invalid organization", ErrAllowanceInvalid)
	}
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1,0))`, org); err != nil {
		return err
	}
	var deleted bool
	if err := tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM billing_organization_tombstones WHERE org_id=$1)`, org).Scan(&deleted); err != nil {
		return err
	}
	if deleted {
		return ErrOrganizationDeleted
	}
	return nil
}

func (r *Repository) ConfigureAllowance(ctx context.Context, org string, p AllowancePolicy, now time.Time) error {
	if err := p.Validate(); err != nil {
		return err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	if err = lockAllowanceOrg(ctx, tx, org); err != nil {
		return err
	}
	body, _ := json.Marshal(p)
	var same bool
	err = tx.QueryRow(ctx, `SELECT policy=$3::jsonb FROM billing_allowance_policies WHERE org_id=$1 AND metric=$2`, org, p.Metric, body).Scan(&same)
	if err == nil {
		if !same {
			return ErrAllowanceConflict
		}
		return tx.Commit(ctx)
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return err
	}
	// Activation never imports unclassified historical usage or silently grants
	// credits retroactively. An exact retry above remains valid after activation.
	if p.StartsAt.Before(now) {
		return fmt.Errorf("%w: starts_at must be in the future", ErrAllowanceInvalid)
	}
	var exists bool
	if err = tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM billing_accounts WHERE org_id=$1 AND subscription_state IN ('active','trialing'))`, org).Scan(&exists); err != nil {
		return err
	}
	if !exists {
		return ErrNotFound
	}
	_, err = tx.Exec(ctx, `INSERT INTO billing_allowance_policies(org_id,metric,policy) VALUES($1,$2,$3::jsonb)`, org, p.Metric, body)
	if err != nil {
		return err
	}
	// Include already received future-dated entries when a policy is activated.
	var fractional bool
	if err = tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM billing_usage_events WHERE org_id=$1 AND metric=$2 AND occurred_at >= $3 AND quantity<>trunc(quantity))`, org, p.Metric, p.StartsAt).Scan(&fractional); err != nil {
		return err
	}
	if fractional {
		return ErrAllowanceInvalid
	}
	rows, err := tx.Query(ctx, `SELECT event_id,quantity::bigint,occurred_at FROM billing_usage_events WHERE org_id=$1 AND metric=$2 AND occurred_at >= $3 ORDER BY id LIMIT 100001`, org, p.Metric, p.StartsAt)
	if err != nil {
		return err
	}
	entries := []AllowanceEntry{}
	for rows.Next() {
		var eventID *string
		var e AllowanceEntry
		if err = rows.Scan(&eventID, &e.Units, &e.OccurredAt); err != nil {
			rows.Close()
			return err
		}
		if eventID == nil {
			rows.Close()
			return ErrAllowanceInvalid
		}
		hash := sha256.Sum256([]byte(*eventID))
		e.ID = "usage:" + hex.EncodeToString(hash[:])
		e.Kind = "usage"
		entries = append(entries, e)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	if len(entries) > 100000 {
		return ErrAllowanceCapacity
	}
	for _, e := range entries {
		if err = appendAllowanceEntry(ctx, tx, org, p.Metric, e); err != nil {
			return err
		}
	}
	return tx.Commit(ctx)
}

func appendAllowanceEntry(ctx context.Context, tx pgx.Tx, org, metric string, e AllowanceEntry) error {
	if err := e.Validate(); err != nil {
		return err
	}
	result, err := tx.Exec(ctx, `INSERT INTO billing_allowance_entries(org_id,metric,entry_id,kind,units,occurred_at,expires_at,reason)
 VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING`, org, metric, e.ID, e.Kind, e.Units, e.OccurredAt, e.ExpiresAt, e.Reason)
	if err != nil {
		return err
	}
	if result.RowsAffected() == 1 {
		return nil
	}
	var same bool
	err = tx.QueryRow(ctx, `SELECT kind=$4 AND units=$5 AND occurred_at=$6 AND expires_at IS NOT DISTINCT FROM $7::timestamptz AND reason=$8
 FROM billing_allowance_entries WHERE org_id=$1 AND metric=$2 AND entry_id=$3`, org, metric, e.ID, e.Kind, e.Units, e.OccurredAt, e.ExpiresAt, e.Reason).Scan(&same)
	if err != nil {
		return err
	}
	if !same {
		return ErrAllowanceConflict
	}
	return nil
}

func (r *Repository) GrantAllowance(ctx context.Context, org, metric string, e AllowanceEntry) error {
	if e.Kind != "grant" || e.Reason == "" {
		return fmt.Errorf("%w: grant and audit reason required", ErrAllowanceInvalid)
	}
	if err := e.Validate(); err != nil {
		return err
	}
	// Grants cannot masquerade as a source invocation.
	if len(e.ID) >= 6 && e.ID[:6] == "usage:" {
		return ErrAllowanceInvalid
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	if err = lockAllowanceOrg(ctx, tx, org); err != nil {
		return err
	}
	var active bool
	if err = tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM billing_accounts WHERE org_id=$1 AND subscription_state IN ('active','trialing'))`, org).Scan(&active); err != nil {
		return err
	}
	if !active {
		return ErrAllowanceInvalid
	}
	var raw []byte
	if err = tx.QueryRow(ctx, `SELECT policy FROM billing_allowance_policies WHERE org_id=$1 AND metric=$2`, org, metric).Scan(&raw); errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	} else if err != nil {
		return err
	}
	var p AllowancePolicy
	if err = json.Unmarshal(raw, &p); err != nil {
		return err
	}
	if e.OccurredAt.Before(p.StartsAt) || (p.EndsAt != nil && !e.OccurredAt.Before(*p.EndsAt)) {
		return ErrAllowanceInvalid
	}
	if err = appendAllowanceEntry(ctx, tx, org, metric, e); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

// Called inside RecordUsage's transaction under the organization lifecycle
// lock. Source usage and credit consumption cannot commit independently.
func recordAllowanceUsage(ctx context.Context, tx pgx.Tx, e UsageEvent) error {
	var raw []byte
	err := tx.QueryRow(ctx, `SELECT policy FROM billing_allowance_policies WHERE org_id=$1 AND metric=$2`, e.OrgID, e.Metric).Scan(&raw)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	var p AllowancePolicy
	if err = json.Unmarshal(raw, &p); err != nil {
		return err
	}
	if e.OccurredAt.Before(p.StartsAt) {
		return nil
	}
	if math.Trunc(e.Quantity) != e.Quantity {
		return fmt.Errorf("%w: this meter requires integer units", ErrAllowanceInvalid)
	}
	hash := sha256.Sum256([]byte(e.EventID))
	return appendAllowanceEntry(ctx, tx, e.OrgID, e.Metric, AllowanceEntry{ID: "usage:" + hex.EncodeToString(hash[:]), Kind: "usage", Units: int64(e.Quantity), OccurredAt: e.OccurredAt})
}

func (r *Repository) Allowances(ctx context.Context, org string, now time.Time) ([]AllowanceReport, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback(ctx)
	if err = lockAllowanceOrg(ctx, tx, org); err != nil {
		return nil, err
	}
	rows, err := tx.Query(ctx, `SELECT policy FROM billing_allowance_policies WHERE org_id=$1 ORDER BY metric`, org)
	if err != nil {
		return nil, err
	}
	policies := []AllowancePolicy{}
	for rows.Next() {
		var raw []byte
		if err = rows.Scan(&raw); err != nil {
			rows.Close()
			return nil, err
		}
		var p AllowancePolicy
		if err = json.Unmarshal(raw, &p); err != nil {
			rows.Close()
			return nil, err
		}
		policies = append(policies, p)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return nil, err
	}
	reports := make([]AllowanceReport, 0, len(policies))
	pauses := []AllowancePause{}
	rows, err = tx.Query(ctx, `SELECT started_at,ended_at FROM billing_allowance_pauses WHERE org_id=$1 ORDER BY started_at`, org)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var pause AllowancePause
		if err = rows.Scan(&pause.From, &pause.To); err != nil {
			rows.Close()
			return nil, err
		}
		pauses = append(pauses, pause)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return nil, err
	}
	for _, p := range policies {
		p.Pauses = pauses
		p.Revisions, err = loadAllowanceRevisions(ctx, tx, org, p.Metric)
		if err != nil {
			return nil, err
		}
		rows, err = tx.Query(ctx, `SELECT entry_id,kind,units,occurred_at,expires_at,reason FROM billing_allowance_entries WHERE org_id=$1 AND metric=$2 AND occurred_at<=$3 ORDER BY occurred_at,entry_id LIMIT 100001`, org, p.Metric, now)
		if err != nil {
			return nil, err
		}
		entries := []AllowanceEntry{}
		for rows.Next() {
			var e AllowanceEntry
			if err = rows.Scan(&e.ID, &e.Kind, &e.Units, &e.OccurredAt, &e.ExpiresAt, &e.Reason); err != nil {
				rows.Close()
				return nil, err
			}
			entries = append(entries, e)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return nil, err
		}
		report, err := CalculateAllowance(p, entries, now)
		if err != nil {
			return nil, err
		}
		data, err := json.Marshal(report)
		if err != nil {
			return nil, err
		}
		if _, err = tx.Exec(ctx, `INSERT INTO billing_allowance_snapshots(org_id,metric,snapshot,computed_at) VALUES($1,$2,$3::jsonb,$4)
  ON CONFLICT(org_id,metric) DO UPDATE SET snapshot=EXCLUDED.snapshot,computed_at=EXCLUDED.computed_at`, org, p.Metric, data, now); err != nil {
			return nil, err
		}
		reports = append(reports, report)
	}
	if err = tx.Commit(ctx); err != nil {
		return nil, err
	}
	return reports, nil
}

func (s *Service) Allowances(ctx context.Context, org string) ([]AllowanceReport, error) {
	return s.repo.Allowances(ctx, org, time.Now().UTC())
}
func (s *Service) ConfigureAllowance(ctx context.Context, org string, p AllowancePolicy) error {
	return s.repo.ConfigureAllowance(ctx, org, p, time.Now().UTC())
}
func (s *Service) GrantAllowance(ctx context.Context, org, metric string, e AllowanceEntry) error {
	return s.repo.GrantAllowance(ctx, org, metric, e)
}

// Refresh persisted projections in bounded batches. Reads also reconcile, so
// scheduler downtime never skips renewals or requires synthetic usage events.
func (s *Service) RefreshAllowances(ctx context.Context) error {
	rows, err := s.repo.pool.Query(ctx, `SELECT p.org_id FROM billing_allowance_policies p LEFT JOIN billing_allowance_snapshots s USING(org_id,metric)
 WHERE NOT EXISTS(SELECT 1 FROM billing_organization_tombstones t WHERE t.org_id=p.org_id)
 GROUP BY p.org_id ORDER BY MIN(COALESCE(s.computed_at,'epoch'::timestamptz)) LIMIT 100`)
	if err != nil {
		return err
	}
	orgs := []string{}
	for rows.Next() {
		var org string
		if err = rows.Scan(&org); err != nil {
			rows.Close()
			return err
		}
		orgs = append(orgs, org)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	var failures []error
	for _, org := range orgs {
		if _, err = s.Allowances(ctx, org); err != nil {
			failures = append(failures, err)
		}
	}
	return errors.Join(failures...)
}
