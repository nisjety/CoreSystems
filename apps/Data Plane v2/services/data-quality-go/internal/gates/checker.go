package gates

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

type Checker struct {
	pool *pgxpool.Pool
	// coverageThreshold is the percentage contextual_coverage_ok requires.
	coverageThreshold float64
	// parity reads the keyword index's parity report. Nil when the endpoint is
	// not configured: sparse_index_in_sync then reports "unavailable".
	parity ParitySource
}

// Option configures a Checker.
type Option func(*Checker)

// WithCoverageThreshold sets the percentage of eligible units that must have a
// verified context (CONTEXT_COVERAGE_THRESHOLD_PERCENT, default 99).
func WithCoverageThreshold(percent float64) Option {
	return func(c *Checker) { c.coverageThreshold = percent }
}

// WithParity sets where the keyword index's parity report comes from.
func WithParity(source ParitySource) Option {
	return func(c *Checker) { c.parity = source }
}

func NewChecker(pool *pgxpool.Pool, options ...Option) *Checker {
	checker := &Checker{pool: pool, coverageThreshold: DefaultCoverageThresholdPercent}
	for _, option := range options {
		option(checker)
	}
	return checker
}

// ContextCoverage reads the organization's contextual-retrieval coverage and
// judges it by the same rule as the contextual_coverage_ok gate. The organization
// comes from verified claims; the read runs in one scoped transaction.
func (c *Checker) ContextCoverage(ctx context.Context, orgID string) (*CoverageReport, error) {
	report := &CoverageReport{OrgID: orgID}
	err := orgscope.WithOrgScope(ctx, c.pool, orgID, func(tx pgx.Tx) error {
		counts, err := readCoverage(ctx, tx, orgID)
		if err != nil {
			return err
		}
		record, err := policy.Load(ctx, tx, orgID)
		if err != nil {
			return err
		}
		report.Counts = counts
		report.Policy = settingOf(record)
		return nil
	})
	if err != nil {
		return nil, err
	}
	report.Evaluation = EvaluateContextCoverage(report.Counts, report.Policy, c.coverageThreshold)
	return report, nil
}

// tolerate runs a query whose failure must NOT fail the whole gate report.
//
// Phase 1 RLS made this helper necessary. On the unscoped pool each gate was
// its own implicit transaction, so a failing statement (a column missing after
// schema drift, say) affected only that gate. Inside one scoped transaction an
// error aborts the tx, and every later statement — including the COMMIT — dies
// with "current transaction is aborted". That would silently convert a
// degraded-but-successful report into a 500. A SAVEPOINT (pgx models one as a
// nested Begin) restores the original per-gate tolerance.
func tolerate(ctx context.Context, tx pgx.Tx, run func(pgx.Tx) error) {
	sp, err := tx.Begin(ctx)
	if err != nil {
		return
	}
	if err := run(sp); err != nil {
		_ = sp.Rollback(ctx)
		return
	}
	_ = sp.Commit(ctx)
}

// CheckReleaseGates evaluates the release gates for exactly one organization.
//
// Phase 1 RLS: orgID arrives from verified caller claims (authctx) via the
// /v1/quality/gates handler — authctx rejects any token without a non-empty
// org_id — so every statement below serves a single tenant and runs inside one
// scoped transaction. One scope rather than six also means the gates cannot
// disagree with each other about which rows exist under a concurrent write.
// The SQL still binds org_id itself; the policy is a backstop against that
// filter being dropped, not a replacement for it.
func (c *Checker) CheckReleaseGates(ctx context.Context, orgID string) (*model.GateReport, error) {
	var gates []model.GateResult
	// What the sparse_index_in_sync gate needs from the database, read inside the
	// scope below; the HTTP call to the adapter is made after the transaction has
	// ended so that no pooled connection is held across a network round trip.
	var (
		parityPG    PostgresSide
		parityPGErr error
	)

	err := orgscope.WithOrgScope(ctx, c.pool, orgID, func(tx pgx.Tx) error {
		// Gate 1: retrieval traces exist
		var traceCount int
		err := tx.QueryRow(ctx,
			"SELECT COUNT(*) FROM retrieval_runs WHERE org_id = $1", orgID).Scan(&traceCount)
		if err != nil {
			return fmt.Errorf("check traces: %w", err)
		}
		gates = append(gates, model.GateResult{
			Gate:    "retrieval_traces_exist",
			Passed:  traceCount > 0,
			Message: fmt.Sprintf("%d retrieval traces found", traceCount),
			Value:   fmt.Sprintf("%d", traceCount),
		})

		// Gate 2: no failed embeddings
		var failedEmbeddings int
		err = tx.QueryRow(ctx,
			"SELECT COUNT(*) FROM knowledge_units WHERE org_id = $1 AND embedding_status = 'failed'", orgID).Scan(&failedEmbeddings)
		if err != nil {
			return fmt.Errorf("check embeddings: %w", err)
		}
		gates = append(gates, model.GateResult{
			Gate:    "no_failed_embeddings",
			Passed:  failedEmbeddings == 0,
			Message: fmt.Sprintf("%d failed embeddings", failedEmbeddings),
			Value:   fmt.Sprintf("%d", failedEmbeddings),
		})

		// Gate 3: all indexed documents have chunks. The knowledge_units
		// sub-select carries no org filter of its own; under scope the policy
		// adds one, which only tightens a correlation that documents has
		// already narrowed to this org.
		var docsWithoutChunks int
		err = tx.QueryRow(ctx, `
			SELECT COUNT(*) FROM documents d
			WHERE d.org_id = $1 AND d.status = 'indexed' AND d.deleted_at IS NULL
			  AND NOT EXISTS (SELECT 1 FROM knowledge_units ku WHERE ku.document_id = d.document_id)
		`, orgID).Scan(&docsWithoutChunks)
		if err != nil {
			return fmt.Errorf("check chunks: %w", err)
		}
		gates = append(gates, model.GateResult{
			Gate:    "indexed_docs_have_chunks",
			Passed:  docsWithoutChunks == 0,
			Message: fmt.Sprintf("%d indexed documents missing chunks", docsWithoutChunks),
			Value:   fmt.Sprintf("%d", docsWithoutChunks),
		})

		// Gate 4: retrieval latency p95 under threshold
		var p95Ms int
		tolerate(ctx, tx, func(sp pgx.Tx) error {
			if err := sp.QueryRow(ctx, `
				SELECT COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY total_ms), 0)::INT
				FROM retrieval_runs WHERE org_id = $1
			`, orgID).Scan(&p95Ms); err != nil {
				p95Ms = 0
				return err
			}
			return nil
		})
		gates = append(gates, model.GateResult{
			Gate:    "retrieval_p95_under_2000ms",
			Passed:  p95Ms < 2000,
			Message: fmt.Sprintf("p95 retrieval latency: %dms", p95Ms),
			Value:   fmt.Sprintf("%d", p95Ms),
		})

		// Gate 5: zero query failures (no runs with 0 candidates)
		var zeroResults int
		tolerate(ctx, tx, func(sp pgx.Tx) error {
			if err := sp.QueryRow(ctx,
				"SELECT COUNT(*) FROM retrieval_runs WHERE org_id = $1 AND candidate_count_reranked = 0", orgID).Scan(&zeroResults); err != nil {
				zeroResults = 0
				return err
			}
			return nil
		})
		gates = append(gates, model.GateResult{
			Gate:    "no_zero_result_queries",
			Passed:  zeroResults == 0,
			Message: fmt.Sprintf("%d queries returned zero results", zeroResults),
			Value:   fmt.Sprintf("%d", zeroResults),
		})

		// Gate 6 (D4+D5 spec D5-10): retrieval latency p95 under 800ms — the
		// spec's hybrid release-gate value. We surface it alongside the looser
		// 2000ms gate above so operators see both signals; deploys block only
		// when this stricter gate is configured as required (env-driven).
		gates = append(gates, model.GateResult{
			Gate:    "retrieval_p95_under_800ms_spec",
			Passed:  p95Ms < 800,
			Message: fmt.Sprintf("p95 retrieval latency: %dms (D5-10 spec gate: 800ms)", p95Ms),
			Value:   fmt.Sprintf("%d", p95Ms),
		})

		// Gate 7 (CR-T11): contextual retrieval coverage. Read in a savepoint like
		// the gates above: a database that does not have the migration yet is an
		// UNAVAILABLE gate, not a failed report and never a pass.
		var (
			counts   CoverageCounts
			setting  PolicySetting
			coverErr error
		)
		tolerate(ctx, tx, func(sp pgx.Tx) error {
			if counts, coverErr = readCoverage(ctx, sp, orgID); coverErr != nil {
				return coverErr
			}
			record, err := policy.Load(ctx, sp, orgID)
			if err != nil {
				coverErr = err
				return err
			}
			setting = settingOf(record)
			return nil
		})
		if coverErr != nil {
			gates = append(gates, unavailableCoverage("the coverage view or the policy table could not be read; has migration 20261003130000_context_provenance been applied?"))
		} else {
			gates = append(gates, EvaluateContextCoverage(counts, setting, c.coverageThreshold).GateOf())
		}

		// The database half of gate 8, counted with the adapter's own eligibility
		// predicate and without the view, so it does not depend on the migration.
		tolerate(ctx, tx, func(sp pgx.Tx) error {
			parityPG, parityPGErr = readPostgresSide(ctx, sp, orgID)
			return parityPGErr
		})
		return nil
	})
	if err != nil {
		return nil, err
	}

	// Gate 8 (CR-T11): the keyword index is in sync with Postgres. The index is read
	// through the adapter's parity endpoint (data-quality-go has no Quickwit access);
	// without a configured, working endpoint the gate is "unavailable", which fails
	// the report: unread evidence is never a pass.
	if c.parity == nil {
		gates = append(gates, unavailableParity("SPARSE_INDEX_PARITY_URL is not set, so there is no parity endpoint to read"))
	} else {
		report, fetchErr := c.parity.Fetch(ctx)
		gates = append(gates, EvaluateSparseParity(orgID, parityPG, parityPGErr, report, fetchErr))
	}

	allPassed := true
	for _, g := range gates {
		if !g.Passed {
			allPassed = false
			break
		}
	}

	return &model.GateReport{AllPassed: allPassed, Gates: gates}, nil
}
