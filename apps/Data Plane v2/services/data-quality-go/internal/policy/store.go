package policy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// ErrNotMigrated is returned when org_processing_policy does not exist: the
// context-provenance migration (20261003130000) has not been applied to this
// database. It is the one failure that is not a 500: the deploy order, not the
// request, is wrong.
var ErrNotMigrated = errors.New("policy: org_processing_policy does not exist (apply migration 20261003130000_context_provenance)")

// Store reads and writes org_processing_policy and its audit trail.
//
// Tenancy. Every statement runs inside one orgscope.WithOrgScope transaction for
// the ONE organization the verified token belongs to, so row-level security
// (dataplane_app, NOBYPASSRLS) confines it: a PUT that named another
// organization's row could not even see it. The handler additionally refuses a
// path organization that differs from the token's, and the SQL still names the
// organization in every predicate. The policy is a backstop, not the only check.
type Store struct {
	pool *pgxpool.Pool
}

// NewStore returns a Store over the service's pool (the superuser every Data
// Plane service connects as; the scope drops it to dataplane_app per transaction).
func NewStore(pool *pgxpool.Pool) *Store { return &Store{pool: pool} }

const selectPolicySQL = `
	SELECT contextual_retrieval, graph_extraction, context_prompt_version, budget_mode,
	       monthly_budget_usd::float8, reason, updated_by, created_at, updated_at
	  FROM org_processing_policy
	 WHERE org_id = $1`

// Querier is the read surface of a scoped transaction (or a savepoint in one).
type Querier interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Load reads one organization's policy through q, which must already be inside
// that organization's scope (orgscope.WithOrgScope): the release gates read the
// policy in the same transaction as the coverage they judge by it. An organization
// without a row has the system default (Exists false).
func Load(ctx context.Context, q Querier, orgID string) (Record, error) {
	record := Default(orgID)
	p := Policy{OrgID: orgID}
	err := q.QueryRow(ctx, selectPolicySQL, orgID).Scan(
		&p.ContextualRetrieval, &p.GraphExtraction, &p.ContextPromptVersion, &p.BudgetMode,
		&p.MonthlyBudgetUSD, &p.Reason, &p.UpdatedBy, &p.CreatedAt, &p.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return record, nil
	}
	if err != nil {
		return Record{}, mapUndefinedTable(err)
	}
	return Record{Policy: p, Exists: true}, nil
}

// mapUndefinedTable turns "relation does not exist" into ErrNotMigrated and
// leaves every other error as it is.
func mapUndefinedTable(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == "42P01" {
		return fmt.Errorf("%w: %v", ErrNotMigrated, pgErr.Message)
	}
	return err
}

// Get returns the organization's policy, or the system default (Exists false)
// when it has none.
func (s *Store) Get(ctx context.Context, orgID string) (Record, error) {
	var record Record
	err := orgscope.WithOrgScope(ctx, s.pool, orgID, func(tx pgx.Tx) error {
		var err error
		record, err = Load(ctx, tx, orgID)
		return err
	})
	if err != nil {
		return Record{}, err
	}
	return record, nil
}

// snapshot is the content-free shape of a policy in the audit payload. The
// reason is the operator's own note about why; it is stored on the policy row too.
type snapshot struct {
	ContextualRetrieval  string   `json:"contextual_retrieval"`
	GraphExtraction      string   `json:"graph_extraction"`
	ContextPromptVersion *string  `json:"context_prompt_version"`
	BudgetMode           string   `json:"budget_mode"`
	MonthlyBudgetUSD     *float64 `json:"monthly_budget_usd"`
	Reason               *string  `json:"reason"`
}

func snapshotOf(r Record) *snapshot {
	if !r.Exists {
		return nil
	}
	return &snapshot{
		ContextualRetrieval:  r.ContextualRetrieval,
		GraphExtraction:      r.GraphExtraction,
		ContextPromptVersion: r.ContextPromptVersion,
		BudgetMode:           r.BudgetMode,
		MonthlyBudgetUSD:     r.MonthlyBudgetUSD,
		Reason:               r.Reason,
	}
}

type auditPayload struct {
	Before        *snapshot `json:"before"`
	After         *snapshot `json:"after"`
	Reason        *string   `json:"reason"`
	PrincipalType string    `json:"principal_type"`
}

const upsertPolicySQL = `
	INSERT INTO org_processing_policy
	       (org_id, contextual_retrieval, graph_extraction, context_prompt_version,
	        budget_mode, monthly_budget_usd, reason, updated_by)
	VALUES ($1, $2, $3, $4, $5, $6::float8::numeric, $7, $8)
	ON CONFLICT (org_id) DO UPDATE SET
	       contextual_retrieval   = EXCLUDED.contextual_retrieval,
	       graph_extraction       = EXCLUDED.graph_extraction,
	       context_prompt_version = EXCLUDED.context_prompt_version,
	       budget_mode            = EXCLUDED.budget_mode,
	       monthly_budget_usd     = EXCLUDED.monthly_budget_usd,
	       reason                 = EXCLUDED.reason,
	       updated_by             = EXCLUDED.updated_by
	RETURNING created_at, updated_at`

const insertAuditSQL = `
	INSERT INTO admin_audit_log (org_id, actor, action, target_kind, target_id, request_id, payload, outcome)
	VALUES ($1, $2, $3, 'org_processing_policy', $1, $4, $5::jsonb, 'ok')`

// Put replaces the organization's policy with the (already validated) input and
// writes the audit row in the SAME transaction: there is no policy change without
// its audit record and no audit record of a change that did not happen. An input
// that asks for the default removes the row.
//
// Concurrent writes to one organization are serialized by a transaction-scoped
// advisory lock, so the "before" a change records is the state it really replaced.
func (s *Store) Put(ctx context.Context, orgID string, in Input, actor Actor) (Result, error) {
	var result Result
	err := orgscope.WithOrgScope(ctx, s.pool, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx,
			"SELECT pg_advisory_xact_lock(hashtextextended('org_processing_policy:' || $1::text, 0))", orgID); err != nil {
			return fmt.Errorf("policy: lock: %w", err)
		}
		before, err := Load(ctx, tx, orgID)
		if err != nil {
			return err
		}

		after := Default(orgID)
		action := "reset"
		if in.IsDefault() {
			if _, err := tx.Exec(ctx, "DELETE FROM org_processing_policy WHERE org_id = $1", orgID); err != nil {
				return fmt.Errorf("policy: reset: %w", mapUndefinedTable(err))
			}
		} else {
			action = "set"
			after = Record{Exists: true, Policy: Policy{
				OrgID:                orgID,
				ContextualRetrieval:  in.ContextualRetrieval,
				GraphExtraction:      in.GraphExtraction,
				ContextPromptVersion: in.ContextPromptVersion,
				BudgetMode:           in.BudgetMode,
				MonthlyBudgetUSD:     in.MonthlyBudgetUSD,
				Reason:               in.Reason,
				UpdatedBy:            actor.ID,
			}}
			if err := tx.QueryRow(ctx, upsertPolicySQL,
				orgID, in.ContextualRetrieval, in.GraphExtraction, in.ContextPromptVersion,
				in.BudgetMode, in.MonthlyBudgetUSD, in.Reason, actor.ID,
			).Scan(&after.CreatedAt, &after.UpdatedAt); err != nil {
				return fmt.Errorf("policy: write: %w", mapUndefinedTable(err))
			}
		}

		payload, err := json.Marshal(auditPayload{
			Before:        snapshotOf(before),
			After:         snapshotOf(after),
			Reason:        in.Reason,
			PrincipalType: actor.Type,
		})
		if err != nil {
			return fmt.Errorf("policy: encode audit payload: %w", err)
		}
		var requestID *string
		if actor.RequestID != "" {
			requestID = &actor.RequestID
		}
		if _, err := tx.Exec(ctx, insertAuditSQL,
			orgID, actor.ID, "org_processing_policy."+action, requestID, string(payload)); err != nil {
			return fmt.Errorf("policy: audit: %w", err)
		}
		result = Result{Action: action, Before: before, After: after}
		return nil
	})
	if err != nil {
		return Result{}, err
	}
	return result, nil
}
