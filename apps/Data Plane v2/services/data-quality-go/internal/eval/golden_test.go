package eval

import (
	"math"
	"testing"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

func refs(docIDs ...string) []RetrievedRef {
	out := make([]RetrievedRef, 0, len(docIDs))
	for _, id := range docIDs {
		out = append(out, RetrievedRef{DocumentID: id})
	}
	return out
}

func almost(t *testing.T, name string, got, want float64) {
	t.Helper()
	if math.Abs(got-want) > 1e-9 {
		t.Fatalf("%s = %v, want %v", name, got, want)
	}
}

func TestNormalizeQuery(t *testing.T) {
	cases := map[string]string{
		"  How does   Billing WORK? ": "how does billing work?",
		"single":                      "single",
		"":                            "",
	}
	for in, want := range cases {
		if got := NormalizeQuery(in); got != want {
			t.Fatalf("NormalizeQuery(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestGoldenMetricsPerfectRetrieval(t *testing.T) {
	// Both relevant docs retrieved at ranks 1-2 → recall 1, nDCG 1, MRR 1.
	recall, ndcg, mrr := goldenMetrics(refs("d1", "d2", "x1"), []string{"d1", "d2"})
	almost(t, "recall", recall, 1.0)
	almost(t, "ndcg", ndcg, 1.0)
	almost(t, "mrr", mrr, 1.0)
}

func TestGoldenMetricsPartialAndRanked(t *testing.T) {
	// One of two relevant docs found, at rank 3.
	recall, ndcg, mrr := goldenMetrics(refs("x1", "x2", "d1"), []string{"d1", "d2"})
	almost(t, "recall", recall, 0.5)
	// DCG = 1/log2(4); IDCG = 1/log2(2) + 1/log2(3).
	wantNDCG := (1.0 / math.Log2(4)) / (1.0/math.Log2(2) + 1.0/math.Log2(3))
	almost(t, "ndcg", ndcg, wantNDCG)
	almost(t, "mrr", mrr, 1.0/3.0)
}

func TestGoldenMetricsNoHitsAndEmptyJudgment(t *testing.T) {
	recall, ndcg, mrr := goldenMetrics(refs("x1", "x2"), []string{"d1"})
	almost(t, "recall", recall, 0)
	almost(t, "ndcg", ndcg, 0)
	almost(t, "mrr", mrr, 0)

	recall, ndcg, mrr = goldenMetrics(refs("x1"), nil)
	almost(t, "recall", recall, 0)
	almost(t, "ndcg", ndcg, 0)
	almost(t, "mrr", mrr, 0)
}

func TestGoldenMetricsMatchesKnowledgeIDToo(t *testing.T) {
	// Judgments may name chunk (knowledge) ids, not just documents.
	retrieved := []RetrievedRef{{KnowledgeID: "k9", DocumentID: "dX"}}
	recall, _, mrr := goldenMetrics(retrieved, []string{"k9"})
	almost(t, "recall", recall, 1.0)
	almost(t, "mrr", mrr, 1.0)
}

func TestGoldenMetricsOnlyTop10Counts(t *testing.T) {
	// Relevant doc at rank 11 → outside the @10 cutoff.
	retrieved := refs("x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "d1")
	recall, ndcg, mrr := goldenMetrics(retrieved, []string{"d1"})
	almost(t, "recall", recall, 0)
	almost(t, "ndcg", ndcg, 0)
	almost(t, "mrr", mrr, 0)
}

func TestScoreLabelsGoldenVsProxy(t *testing.T) {
	traces := []RetrievalTrace{
		{Query: "Judged Query", TotalMS: 10, Candidates: 5, Retrieved: refs("d1")},
		{Query: "unjudged query", TotalMS: 20, Candidates: 5},
	}
	golden := map[string][]string{"judged query": {"d1"}}

	sc := score("hybrid", traces, golden)

	if sc.QueriesRun != 2 || sc.GoldenQueries != 1 {
		t.Fatalf("queries_run=%d golden_queries=%d, want 2/1", sc.QueriesRun, sc.GoldenQueries)
	}
	if sc.Details[0].MetricSource != model.MetricSourceGolden {
		t.Fatalf("judged query labeled %q", sc.Details[0].MetricSource)
	}
	almost(t, "judged recall", sc.Details[0].RecallAt10, 1.0)
	if sc.Details[1].MetricSource != model.MetricSourceProxy {
		t.Fatalf("unjudged query labeled %q", sc.Details[1].MetricSource)
	}
	// Proxy path unchanged: min(5/10, 1) = 0.5.
	almost(t, "proxy recall", sc.Details[1].RecallAt10, 0.5)
}
