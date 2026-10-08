package gates

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/dbtest"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// These tests run the REAL release gates against Postgres 16 with the real migrations
// (MIGRATED_TEST_DATABASE_URL, see internal/dbtest): the real v_context_coverage view (security
// invoker), the real org_processing_policy table, the real documents/knowledge_units constraints,
// and the real orgscope helper, which runs every gate as the NOBYPASSRLS role dataplane_app. They
// skip without the variable and FAIL under REQUIRE_CONTRACT (set by the postgres-contract-tests job).

func uniqueOrg(prefix string) string {
	return prefix + "-" + strings.ReplaceAll(uuid.NewString(), "-", "")[:12]
}

const seededContext = "Situating sentence."

func contextHash() string {
	sum := sha256.Sum256([]byte(seededContext))
	return hex.EncodeToString(sum[:])
}

// A seeded organization: documents and units as the embedding pipeline leaves them.
type seeded struct {
	t    *testing.T
	pool *pgxpool.Pool
	org  string
}

func seed(t *testing.T, pool *pgxpool.Pool, prefix string) seeded {
	t.Helper()
	s := seeded{t: t, pool: pool, org: uniqueOrg(prefix)}
	t.Cleanup(func() {
		ctx := context.Background()
		// Units go with their documents (ON DELETE CASCADE).
		_, _ = pool.Exec(ctx, "DELETE FROM documents WHERE org_id = $1", s.org)
		_, _ = pool.Exec(ctx, "DELETE FROM org_processing_policy WHERE org_id = $1", s.org)
	})
	return s
}

func (s seeded) document(classification string, deleted bool) string {
	s.t.Helper()
	id := "doc-" + uuid.NewString()
	dbtest.Exec(s.t, s.pool,
		`INSERT INTO documents (document_id, org_id, source, type, title, content, status, zdr_classification, deleted_at)
		 VALUES ($1, $2, 'test', 'text', 'Title', 'Content', 'indexed', $3, CASE WHEN $4::boolean THEN now() END)`,
		id, s.org, classification, deleted)
	return id
}

// units inserts n embedded units of the document in the given context state.
//
//	ready, ready_unverified, incoherent, legacy, pending, skipped_policy, skipped_zdr, none
func (s seeded) units(document string, n int, state string) {
	s.t.Helper()
	var (
		context_  *string
		status    *string
		errClass  *string
		embedHash *string
	)
	str := func(v string) *string { return &v }
	switch state {
	case "ready":
		context_, status, embedHash = str(seededContext), str("ready"), str(contextHash())
	case "ready_unverified":
		context_, status = str(seededContext), str("ready")
	case "incoherent":
		context_, status, embedHash = str(seededContext), str("ready"), str(strings.Repeat("0", 64))
	case "legacy":
		context_, status = str(seededContext), str("legacy")
	case "pending":
		status = str("pending")
	case "skipped_policy":
		status = str("skipped_policy")
	case "skipped_zdr":
		status, errClass = str("skipped_zdr"), str("zdr_org")
	case "none":
	default:
		s.t.Fatalf("unknown unit state %q", state)
	}
	dbtest.Exec(s.t, s.pool,
		`INSERT INTO knowledge_units (knowledge_id, document_id, org_id, chunk_index, text, embedding_status,
		                             chunk_context, context_status, context_error_class, embedded_context_hash)
		 SELECT gen_random_uuid()::text, $1::text, $2::text, g, 'chunk text', 'done', $3::text, $4::text, $5::text, $6::text
		   FROM generate_series(1, $7::int) AS g`,
		document, s.org, context_, status, errClass, embedHash, n)
}

func (s seeded) policy(contextual string) {
	s.t.Helper()
	reason := any(nil)
	if contextual == "off" {
		reason = "integration test"
	}
	dbtest.Exec(s.t, s.pool,
		`INSERT INTO org_processing_policy (org_id, contextual_retrieval, reason, updated_by) VALUES ($1, $2, $3, 'test')`,
		s.org, contextual, reason)
}

// withScope runs fn inside the real org scope: one transaction as the runtime role dataplane_app.
func withScope(ctx context.Context, pool *pgxpool.Pool, org string, fn func(q queryer) error) error {
	return orgscope.WithOrgScope(ctx, pool, org, func(tx pgx.Tx) error { return fn(tx) })
}

type fakeParity struct {
	report *ParityReport
	err    error
}

func (f fakeParity) Fetch(context.Context) (*ParityReport, error) { return f.report, f.err }

func gateNamed(t *testing.T, report *model.GateReport, name string) model.GateResult {
	t.Helper()
	for _, gate := range report.Gates {
		if gate.Gate == name {
			return gate
		}
	}
	t.Fatalf("the report has no gate %q: %+v", name, report.Gates)
	return model.GateResult{}
}

func inSyncWith(org string, eligible, withContext int64) *ParityReport {
	report := &ParityReport{Contract: ParityContract, OrgID: org}
	report.Postgres.EligibleUnits, report.Postgres.EligibleWithContext = eligible, withContext
	report.Quickwit.Documents, report.Quickwit.DistinctUnits, report.Quickwit.WithContextBody = eligible, eligible, withContext
	return report
}

// contextual_coverage_ok on real rows, in both directions, through the real view and the real scope.
func TestContextualCoverageGateOnRealRows(t *testing.T) {
	pool := dbtest.Pool(t)
	checker := NewChecker(pool, WithCoverageThreshold(99))
	ctx := context.Background()

	cases := []struct {
		name       string
		setup      func(s seeded)
		wantStatus string
		wantPassed bool
		wantValue  string
		wantInMsg  string
	}{
		{"98 percent fails", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 196, "ready")
			s.units(d, 4, "pending")
		}, StatusFailed, false, "98.00", "4 pending"},
		{"exactly 99 percent passes", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 198, "ready")
			s.units(d, 2, "pending")
		}, StatusPassed, true, "99.00", ""},
		{"the live shape: legacy and one raw unit fail", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 20, "legacy")
			s.units(d, 1, "pending")
		}, StatusFailed, false, "0.00", "20 legacy"},
		{"an off policy passes and says so", func(s seeded) {
			s.policy("off")
			d := s.document("internal", false)
			s.units(d, 50, "skipped_policy")
		}, StatusSkippedPolicy, true, "n/a", "OFF"},
		{"stale skipped_policy units are not excused once the policy is not off", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 10, "ready")
			s.units(d, 190, "skipped_policy")
		}, StatusFailed, false, "5.00", "skipped_policy while the policy is not off"},
		{"zero-retention skips leave the denominator", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 100, "ready")
			s.units(d, 100, "skipped_zdr")
		}, StatusPassed, true, "100.00", ""},
		{"unverified and incoherent ready units are not trusted", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 195, "ready")
			s.units(d, 3, "ready_unverified")
			s.units(d, 2, "incoherent")
		}, StatusFailed, false, "97.50", "2 vector incoherent"},
		{"restricted and deleted documents are outside the population", func(s seeded) {
			d := s.document("internal", false)
			s.units(d, 100, "ready")
			s.units(s.document("restricted", false), 30, "pending")
			s.units(s.document("internal", true), 30, "pending")
		}, StatusPassed, true, "100.00", ""},
		{"an organization with nothing indexed passes vacuously", func(s seeded) {}, StatusPassed, true, "n/a", ""},
	}
	for _, c := range cases {
		s := seed(t, pool, "cov")
		c.setup(s)
		report, err := checker.CheckReleaseGates(ctx, s.org)
		if err != nil {
			t.Fatalf("%s: CheckReleaseGates: %v", c.name, err)
		}
		gate := gateNamed(t, report, GateContextualCoverage)
		if gate.Status != c.wantStatus || gate.Passed != c.wantPassed || gate.Value != c.wantValue || !strings.Contains(gate.Message, c.wantInMsg) {
			t.Errorf("%s: gate = %+v, want status %s passed %v value %s containing %q",
				c.name, gate, c.wantStatus, c.wantPassed, c.wantValue, c.wantInMsg)
		}
	}
}

// Legacy units are not ready until the verification pass runs; after it, the same organization passes.
func TestLegacyUnitsBecomeReadyAfterTheVerificationPass(t *testing.T) {
	pool := dbtest.Pool(t)
	checker := NewChecker(pool, WithCoverageThreshold(99))
	ctx := context.Background()
	s := seed(t, pool, "legacy")
	d := s.document("internal", false)
	s.units(d, 40, "legacy")
	s.units(d, 1, "pending")

	before, err := checker.ContextCoverage(ctx, s.org)
	if err != nil {
		t.Fatal(err)
	}
	if before.Counts.Eligible != 41 || before.Counts.Legacy != 40 || before.Counts.Pending != 1 || before.Evaluation.Status != StatusFailed {
		t.Fatalf("before = %+v", before)
	}

	dbtest.Exec(t, pool, `UPDATE knowledge_units SET context_status = 'ready', embedded_context_hash = $2
		 WHERE org_id = $1 AND context_status = 'legacy'`, s.org, contextHash())
	dbtest.Exec(t, pool, `UPDATE knowledge_units SET chunk_context = $2, context_status = 'ready', embedded_context_hash = $3
		 WHERE org_id = $1 AND context_status = 'pending'`, s.org, seededContext, contextHash())
	after, err := checker.ContextCoverage(ctx, s.org)
	if err != nil {
		t.Fatal(err)
	}
	if after.Counts.Ready != 41 || after.Evaluation.Status != StatusPassed || after.Evaluation.ReadyTrusted != 41 {
		t.Fatalf("after = %+v", after)
	}
}

// Another organization's rows never reach a report: the counts are exactly this organization's, and
// the endpoint's report agrees with the gate.
func TestCoverageIsScopedToTheCallersOrganization(t *testing.T) {
	pool := dbtest.Pool(t)
	checker := NewChecker(pool, WithCoverageThreshold(99))
	ctx := context.Background()
	a, b := seed(t, pool, "scope-a"), seed(t, pool, "scope-b")
	a.units(a.document("internal", false), 7, "ready")
	b.units(b.document("internal", false), 300, "pending")

	reportA, err := checker.ContextCoverage(ctx, a.org)
	if err != nil {
		t.Fatal(err)
	}
	if reportA.OrgID != a.org || reportA.Counts.Eligible != 7 || reportA.Counts.Ready != 7 || reportA.Counts.Pending != 0 {
		t.Fatalf("org A's coverage = %+v: it must be exactly org A's rows", reportA)
	}
	gates, err := checker.CheckReleaseGates(ctx, a.org)
	if err != nil {
		t.Fatal(err)
	}
	gate := gateNamed(t, gates, GateContextualCoverage)
	if gate != reportA.Evaluation.GateOf() {
		t.Fatalf("the gate %+v and the coverage endpoint %+v disagree", gate, reportA.Evaluation.GateOf())
	}
}

// sparse_index_in_sync: the database half is counted with the adapter's eligibility predicate, and
// the gate judges the adapter's report against it, in both directions.
func TestSparseIndexGateAgainstRealPostgres(t *testing.T) {
	pool := dbtest.Pool(t)
	ctx := context.Background()
	s := seed(t, pool, "sparse")
	d := s.document("internal", false)
	s.units(d, 3, "ready")
	s.units(d, 1, "pending") // eligible, no context
	s.units(s.document("public", false), 2, "legacy")
	s.units(s.document("restricted", false), 9, "ready") // never indexed
	s.units(s.document("internal", true), 9, "ready")    // deleted document: never indexed

	// 6 eligible units (3 + 1 + 2), 5 with a context (3 + 2).
	var side PostgresSide
	if err := withScope(ctx, pool, s.org, func(q queryer) error {
		var err error
		side, err = readPostgresSide(ctx, q, s.org)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if side.EligibleUnits != 6 || side.EligibleWithContext != 5 {
		t.Fatalf("database side = %+v, want 6 eligible, 5 with a context (restricted and deleted documents excluded)", side)
	}

	run := func(source ParitySource) model.GateResult {
		options := []Option{WithCoverageThreshold(99)}
		if source != nil {
			options = append(options, WithParity(source))
		}
		report, err := NewChecker(pool, options...).CheckReleaseGates(ctx, s.org)
		if err != nil {
			t.Fatal(err)
		}
		return gateNamed(t, report, GateSparseIndexInSync)
	}

	if gate := run(fakeParity{report: inSyncWith(s.org, 6, 5)}); !gate.Passed || gate.Status != StatusPassed {
		t.Fatalf("an in-sync index = %+v, want a pass", gate)
	}
	// The adapter counts one unit fewer than the database: a predicate drift, not a pass.
	if gate := run(fakeParity{report: inSyncWith(s.org, 5, 5)}); gate.Passed || gate.Status != StatusFailed || !strings.Contains(gate.Message, "disagree") {
		t.Fatalf("a disagreeing adapter = %+v, want failed", gate)
	}
	// A Quickwit count mismatch.
	drift := inSyncWith(s.org, 6, 5)
	drift.Difference.MissingUnits = 4
	drift.Quickwit.DistinctUnits = 2
	if gate := run(fakeParity{report: drift}); gate.Passed || gate.Status != StatusFailed || gate.Value != "2/6" {
		t.Fatalf("a Quickwit mismatch = %+v, want failed 2/6", gate)
	}
	// Not configured, and a configured endpoint that cannot be read: unavailable, never a pass.
	if gate := run(nil); gate.Passed || gate.Status != StatusUnavailable {
		t.Fatalf("no parity endpoint = %+v, want unavailable", gate)
	}
	if gate := run(fakeParity{err: unavailable("the endpoint does not exist")}); gate.Passed || gate.Status != StatusUnavailable {
		t.Fatalf("an unavailable endpoint = %+v, want unavailable", gate)
	}
}

// A gate that cannot judge fails the whole report: unread evidence is never a pass. The organization
// here passes every OTHER gate (a retrieval trace, no failed embedding, full coverage), and the
// control run with a readable, in-sync index proves it, so the report can only fail because of the
// unavailable gate. (A seeded organization without retrieval traces fails retrieval_traces_exist, and
// an assertion made on such a report would pass whether or not an unavailable gate counted.)
func TestAnUnavailableGateFailsTheWholeReport(t *testing.T) {
	pool := dbtest.Pool(t)
	ctx := context.Background()
	s := seed(t, pool, "unavail")
	t.Cleanup(func() { _, _ = pool.Exec(context.Background(), "DELETE FROM retrieval_runs WHERE org_id = $1", s.org) })
	s.units(s.document("internal", false), 100, "ready")
	dbtest.Exec(t, pool,
		`INSERT INTO retrieval_runs (org_id, query, total_ms, candidate_count_reranked) VALUES ($1, 'a question', 40, 3)`, s.org)

	// The control: every gate passes when the parity endpoint answers and agrees.
	control, err := NewChecker(pool, WithCoverageThreshold(99), WithParity(fakeParity{report: inSyncWith(s.org, 100, 100)})).CheckReleaseGates(ctx, s.org)
	if err != nil {
		t.Fatal(err)
	}
	if !control.AllPassed {
		t.Fatalf("the control organization does not pass every gate, so this test would prove nothing: %+v", control.Gates)
	}

	for name, options := range map[string][]Option{
		"no endpoint configured":        {WithCoverageThreshold(99)},
		"an endpoint that is not there": {WithCoverageThreshold(99), WithParity(fakeParity{err: unavailable("the endpoint does not exist")})},
	} {
		report, err := NewChecker(pool, options...).CheckReleaseGates(ctx, s.org)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if report.AllPassed {
			t.Fatalf("%s: a report with an unavailable gate passed: %+v", name, report.Gates)
		}
		for _, gate := range report.Gates {
			if gate.Gate != GateSparseIndexInSync && !gate.Passed {
				t.Fatalf("%s: gate %q failed as well, so the report would fail without the unavailable gate: %+v", name, gate.Gate, gate)
			}
		}
		if gate := gateNamed(t, report, GateSparseIndexInSync); gate.Passed || gate.Status != StatusUnavailable {
			t.Fatalf("%s: gate = %+v, want unavailable", name, gate)
		}
	}
}
