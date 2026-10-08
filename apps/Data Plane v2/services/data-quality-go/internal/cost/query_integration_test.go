package cost

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/dbtest"
)

// The summary over the REAL migrated schema (MIGRATED_TEST_DATABASE_URL: init.sql and every
// migration applied by the real migrator, tests/scripts/run-postgres-contract-tests.sh), read
// through the real org-scope helper, which drops to the role row-level security applies to. The
// rows are written as the usage ledger consumer writes them.

func seedLedger(t *testing.T, org string) {
	t.Helper()
	pool := dbtest.Pool(t)
	dbtest.Exec(t, pool, `DELETE FROM cost_events WHERE org_id = $1`, org)
	// Version 1: the count-only events of the producers that predate the ledger.
	dbtest.Exec(t, pool, `INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key, feature)
		VALUES ('embedding', 'embed-v-4-0', $1, 'user-a', 10, 400, $1::text || ':v1-a', 'embedding'),
		       ('embedding', 'embed-v-4-0', $1, 'user-b', 5, 200, $1::text || ':v1-b', 'embedding'),
		       ('rerank', 'rerank-v3', $1, NULL, 3, 96, $1::text || ':v1-c', 'rerank')`, org)
	// Version 2: exact LLM usage, an estimated embedding and a zero-retention embedding.
	dbtest.Exec(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, schema_version, feature, provider_used,
			input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
		VALUES ('llm', 'gpt-4o-mini', $1, 3, 1272, $1::text || ':v2-a', 2, 'contextual-retrieval', 'azure-openai', 1200, 72, 512, 'metered', false, false, repeat('a', 64)),
		       ('llm', 'gpt-4o-mini', $1, 2, 700, $1::text || ':v2-b', 2, 'contextual-retrieval', 'azure-openai', 640, 60, 0, 'metered', false, false, repeat('b', 64)),
		       ('llm', 'gpt-4o', $1, 1, 780, $1::text || ':v2-c', 2, 'graph-extraction', 'azure-openai', 700, 80, 0, 'metered', false, false, repeat('c', 64)),
		       ('embedding', 'embed-v-4-0', $1, 24, 9100, $1::text || ':v2-d', 2, 'embedding', 'azure-cohere', 9100, 0, 0, 'estimated', true, false, repeat('d', 64)),
		       ('embedding', 'embed-v-4-0', $1, 4, 1500, $1::text || ':v2-e', 2, 'embedding', 'azure-cohere', 1500, 0, 0, 'estimated', true, true, repeat('e', 64))`, org)
}

func TestSummaryReportsExactTokensByFeatureAndNeverPresentsAnEstimateAsExact(t *testing.T) {
	org := "org-cost-summary-" + strings.ReplaceAll(fmt.Sprint(time.Now().UnixNano()), "-", "")
	seedLedger(t, org)
	pool := dbtest.Pool(t)
	t.Cleanup(func() { dbtest.Exec(t, pool, `DELETE FROM cost_events WHERE org_id = $1`, org) })

	summary, err := NewQuery(pool).Summary(context.Background(), org, time.Now().Add(-time.Hour), time.Now().Add(time.Hour))
	if err != nil {
		t.Fatalf("summary: %v", err)
	}

	type key struct{ eventType, feature, model string }
	got := map[key]Bucket{}
	for _, bucket := range summary.Buckets {
		got[key{bucket.EventType, bucket.Feature, bucket.Model}] = bucket
	}
	want := map[key]Bucket{
		// Two calls' worth of exact contextual-retrieval usage, summed: the cache reads are detail
		// of the input tokens and are reported beside them.
		{"llm", "contextual-retrieval", "gpt-4o-mini"}: {EventCount: 2, UnitsTotal: 5, TokensTotal: 1972, InputTokens: 1840, OutputTokens: 132, CacheReadTokens: 512, Estimated: false},
		{"llm", "graph-extraction", "gpt-4o"}:          {EventCount: 1, UnitsTotal: 1, TokensTotal: 780, InputTokens: 700, OutputTokens: 80, Estimated: false},
		// Embedding: the version 1 rows (no exact numbers) and the version 2 ones (an estimate)
		// are one bucket, estimated; one of its events is a zero-retention organization's.
		{"embedding", "embedding", "embed-v-4-0"}: {EventCount: 4, UnitsTotal: 43, TokensTotal: 11200, InputTokens: 10600, Estimated: true, ZDREvents: 1},
		{"rerank", "rerank", "rerank-v3"}:         {EventCount: 1, UnitsTotal: 3, TokensTotal: 96, Estimated: true},
	}
	if len(got) != len(want) {
		t.Fatalf("buckets = %+v, want %d", summary.Buckets, len(want))
	}
	for k, w := range want {
		g, ok := got[k]
		if !ok {
			t.Errorf("no bucket %+v", k)
			continue
		}
		if g.EventCount != w.EventCount || g.UnitsTotal != w.UnitsTotal || g.TokensTotal != w.TokensTotal ||
			g.InputTokens != w.InputTokens || g.OutputTokens != w.OutputTokens || g.CacheReadTokens != w.CacheReadTokens ||
			g.Estimated != w.Estimated || g.ZDREvents != w.ZDREvents {
			t.Errorf("bucket %+v = %+v, want %+v", k, g, w)
		}
	}
	// The totals keep their old meaning: every bucket, summed.
	if summary.EventCount != 8 || summary.UnitsTotal != 52 || summary.TokensTotal != 14048 {
		t.Errorf("totals = %d events, %d units, %d tokens", summary.EventCount, summary.UnitsTotal, summary.TokensTotal)
	}
	// The order is by tokens, largest first, as it has always been.
	for i := 1; i < len(summary.Buckets); i++ {
		if summary.Buckets[i].TokensTotal > summary.Buckets[i-1].TokensTotal {
			t.Fatalf("buckets are not ordered by tokens: %+v", summary.Buckets)
		}
	}
}

// One organization's summary never contains another's: the statement runs inside the real
// org-scope helper (as dataplane_app, row-level security in force) and also names the
// organization.
func TestSummaryOfOneOrganizationNeverContainsAnothers(t *testing.T) {
	suffix := strings.ReplaceAll(fmt.Sprint(time.Now().UnixNano()), "-", "")
	orgA, orgB := "org-cost-a-"+suffix, "org-cost-b-"+suffix
	seedLedger(t, orgA)
	seedLedger(t, orgB)
	pool := dbtest.Pool(t)
	t.Cleanup(func() {
		dbtest.Exec(t, pool, `DELETE FROM cost_events WHERE org_id = ANY($1)`, []string{orgA, orgB})
	})
	// Org B spends far more of the same thing.
	dbtest.Exec(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, schema_version, feature, provider_used,
			input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
		VALUES ('llm', 'gpt-4o', $1, 99, 999999, $1::text || ':big', 2, 'graph-extraction', 'azure-openai', 900000, 99999, 0, 'metered', false, false, repeat('f', 64))`, orgB)

	summary, err := NewQuery(pool).Summary(context.Background(), orgA, time.Now().Add(-time.Hour), time.Now().Add(time.Hour))
	if err != nil {
		t.Fatalf("summary: %v", err)
	}
	if summary.OrgID != orgA || summary.TokensTotal != 14048 {
		t.Fatalf("org A's summary = %+v: it contains org B's spend or lost its own", summary)
	}
}
