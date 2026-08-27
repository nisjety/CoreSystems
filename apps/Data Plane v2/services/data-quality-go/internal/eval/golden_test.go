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

// Regression: judgments are document-level while retrieval is chunk-level, so a
// judged document normally puts several of its chunks in the top 10. Crediting
// every one of them pushed DCG past IDCG (measured 1.40 and 1.69 on two real
// 28-query runs) and the >1 clamp then reported a flawless 1.0 — a saturated
// metric that looks like a passing score and stops discriminating.
func TestGoldenMetricsDoesNotSaturateOnRepeatedChunksOfOneDocument(t *testing.T) {
	// Five chunks of the same judged document at ranks 1-5, one unrelated doc.
	retrieved := []RetrievedRef{
		{DocumentID: "d1", KnowledgeID: "d1:0"},
		{DocumentID: "d1", KnowledgeID: "d1:1"},
		{DocumentID: "d1", KnowledgeID: "d1:2"},
		{DocumentID: "d1", KnowledgeID: "d1:3"},
		{DocumentID: "d1", KnowledgeID: "d1:4"},
		{DocumentID: "x1", KnowledgeID: "x1:0"},
	}

	recall, ndcg, mrr := goldenMetrics(retrieved, []string{"d1", "d2"})

	almost(t, "recall", recall, 0.5) // d1 found, d2 not
	almost(t, "mrr", mrr, 1.0)
	// d1 is credited once, at rank 1. IDCG covers the two judged documents.
	wantNDCG := (1.0 / math.Log2(2)) / (1.0/math.Log2(2) + 1.0/math.Log2(3))
	almost(t, "ndcg", ndcg, wantNDCG)
	if ndcg >= 1.0 {
		t.Fatalf("ndcg saturated at %v: half the judged set was missed", ndcg)
	}
}

// One retrieved item satisfying BOTH a document-level and a chunk-level judgment
// still earns a single gain — otherwise one hit outscores its own ideal position.
func TestGoldenMetricsCreditsOneGainPerRetrievedItem(t *testing.T) {
	retrieved := []RetrievedRef{{DocumentID: "d1", KnowledgeID: "k1"}}

	recall, ndcg, mrr := goldenMetrics(retrieved, []string{"d1", "k1"})

	almost(t, "recall", recall, 1.0) // both judged ids matched
	almost(t, "mrr", mrr, 1.0)
	// DCG = one gain at rank 1; IDCG assumes two ideal items.
	wantNDCG := (1.0 / math.Log2(2)) / (1.0/math.Log2(2) + 1.0/math.Log2(3))
	almost(t, "ndcg", ndcg, wantNDCG)
}

// nDCG must never exceed 1 for any arrangement of duplicate chunks.
func TestGoldenMetricsNDCGStaysInRange(t *testing.T) {
	judged := []string{"d1", "d2", "d3"}
	arrangements := [][]string{
		{"d1", "d1", "d1", "d1", "d1", "d1", "d1", "d1", "d1", "d1"},
		{"d1", "d2", "d1", "d2", "d3", "d3", "d1", "d2", "d3", "d1"},
		{"d3", "d3", "d3", "d2", "d2", "d1", "x1", "x2", "x3", "x4"},
		{"d1", "d2", "d3", "x1", "x2", "x3", "x4", "x5", "x6", "x7"},
	}
	for i, docs := range arrangements {
		_, ndcg, _ := goldenMetrics(refs(docs...), judged)
		if ndcg > 1.0 || ndcg < 0 {
			t.Fatalf("arrangement %d: ndcg = %v, out of [0,1]", i, ndcg)
		}
	}
	// The all-distinct-at-top arrangement is the ideal one and must score 1.
	_, ndcg, _ := goldenMetrics(refs(arrangements[3]...), judged)
	almost(t, "ideal ndcg", ndcg, 1.0)
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
