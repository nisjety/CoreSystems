package eval

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// harness-record-v1 is the request body of POST /v1/evals/results. Its producer is
// scripts/eval-gate.py (--records-out); this package is the consumer. The directory below is vendored
// byte for byte under apps/Data Plane v2/scripts/tests/fixtures/harness-record-v1/ with the same
// SHA256SUMS (scripts/check-eval-contract.sh is the drift check). Both sides must accept the two valid
// records and refuse every invalid.* file: that is how the two implementations of the rules are kept equal.

const recordGoldens = "testdata/harness-record-v1"

func goldenBytes(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(recordGoldens, name))
	if err != nil {
		t.Fatalf("read golden %s: %v", name, err)
	}
	return raw
}

func TestTheValidRecordGoldensAreAccepted(t *testing.T) {
	for _, name := range []string{"record.cell.golden.json", "record.promotion.golden.json"} {
		t.Run(name, func(t *testing.T) {
			record, err := ValidateRecord(goldenBytes(t, name))
			if err != nil {
				t.Fatalf("%s was refused: %v", name, err)
			}
			if !strings.HasPrefix(record.Strategy, "harness:") {
				t.Fatalf("strategy = %q", record.Strategy)
			}
			if record.Corpus == "" || len(record.Scorecard) == 0 {
				t.Fatalf("record = %+v: the corpus and the scorecard must survive validation", record)
			}
			var card map[string]any
			if err := json.Unmarshal(record.Scorecard, &card); err != nil || card["queries_run"] == nil {
				t.Fatalf("the stored scorecard is not the one that was sent: %v %v", err, card)
			}
		})
	}
}

func TestEveryInvalidRecordGoldenIsRefusedForItsReason(t *testing.T) {
	expectations := map[string]string{
		"invalid.strategy-not-harness.json": "strategy",
		"invalid.metric-out-of-range.json":  "mean_ndcg_at_10",
		"invalid.details-present.json":      "details",
		"invalid.class-sum-mismatch.json":   "by_class",
		"invalid.unknown-field.json":        "unknown field",
		"invalid.verdict.json":              "verdict",
	}
	entries, err := os.ReadDir(recordGoldens)
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, entry := range entries {
		if !strings.HasPrefix(entry.Name(), "invalid.") {
			continue
		}
		seen++
		needle, ok := expectations[entry.Name()]
		if !ok {
			t.Errorf("%s has no expectation in this test: add it here and in scripts/tests/test_eval_gate_promotion.py", entry.Name())
			continue
		}
		t.Run(entry.Name(), func(t *testing.T) {
			_, err := ValidateRecord(goldenBytes(t, entry.Name()))
			if err == nil {
				t.Fatalf("%s was accepted", entry.Name())
			}
			var invalid *InvalidRecordError
			if !asInvalid(err, &invalid) {
				t.Fatalf("error = %T %v, want *InvalidRecordError", err, err)
			}
			if !strings.Contains(strings.Join(invalid.Problems, " | "), needle) {
				t.Fatalf("problems %v do not mention %q", invalid.Problems, needle)
			}
		})
	}
	if seen != len(expectations) {
		t.Fatalf("the directory holds %d invalid.* files, this test expects %d", seen, len(expectations))
	}
}

func asInvalid(err error, target **InvalidRecordError) bool { return errors.As(err, target) }

// mutate decodes the valid cell record, applies one change and returns the bytes.
func mutate(t *testing.T, change func(body map[string]any)) []byte {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(goldenBytes(t, "record.cell.golden.json"), &body); err != nil {
		t.Fatal(err)
	}
	change(body)
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func card(body map[string]any) map[string]any    { return body["scorecard"].(map[string]any) }
func harness(body map[string]any) map[string]any { return card(body)["harness"].(map[string]any) }

func TestEachRuleRefusesItsOwnViolation(t *testing.T) {
	cases := []struct {
		name   string
		change func(body map[string]any)
		want   string
	}{
		{"strategy without the prefix", func(b map[string]any) { b["strategy"] = "contextual"; card(b)["strategy"] = "contextual" }, "strategy"},
		{"strategy upper case", func(b map[string]any) {
			b["strategy"] = "harness:Contextual"
			card(b)["strategy"] = "harness:Contextual"
		}, "strategy"},
		{"scorecard strategy differs", func(b map[string]any) { card(b)["strategy"] = "harness:other" }, "scorecard.strategy"},
		{"strategy is not the cell", func(b map[string]any) { harness(b)["cell"] = "lexical-contextual" }, "harness.cell"},
		{"corpus with a space", func(b map[string]any) { b["corpus"] = "a corpus" }, "corpus"},
		{"zero queries", func(b map[string]any) { card(b)["queries_run"] = 0 }, "queries_run"},
		{"golden queries above queries", func(b map[string]any) { card(b)["golden_queries"] = 11 }, "golden_queries"},
		{"recall above 1", func(b map[string]any) { card(b)["mean_recall_at_10"] = 1.01 }, "mean_recall_at_10"},
		{"mrr below 0", func(b map[string]any) { card(b)["mean_mrr"] = -0.1 }, "mean_mrr"},
		{"negative latency", func(b map[string]any) { card(b)["p95_latency_ms"] = -1 }, "p95_latency_ms"},
		{"empty by_class", func(b map[string]any) { card(b)["by_class"] = map[string]any{} }, "by_class"},
		{"by_class bad name", func(b map[string]any) {
			c := card(b)["by_class"].(map[string]any)
			c["Natural Language"] = c["natural_language"]
			delete(c, "natural_language")
		}, "by_class"},
		{"wrong schema", func(b map[string]any) { harness(b)["schema"] = "harness-record-v2" }, "harness.schema"},
		{"wrong kind", func(b map[string]any) { harness(b)["kind"] = "run" }, "harness.kind"},
		{"short run id", func(b map[string]any) { harness(b)["run_id"] = "abc" }, "harness.run_id"},
		{"unknown organization label", func(b map[string]any) { harness(b)["org_label"] = "production" }, "harness.org_label"},
		{"unknown rerank", func(b map[string]any) { harness(b)["rerank"] = "maybe" }, "harness.rerank"},
		{"more zero results than queries", func(b map[string]any) { harness(b)["zero_result"] = 11 }, "harness.zero_result"},
		{"digest not hex", func(b map[string]any) {
			harness(b)["corpus"].(map[string]any)["manifest_digest"] = strings.Repeat("g", 64)
		}, "manifest_digest"},
		{"no documents", func(b map[string]any) { harness(b)["corpus"].(map[string]any)["documents"] = 0 }, "documents"},
		{"too many reasons", func(b map[string]any) {
			reasons := make([]any, 21)
			for i := range reasons {
				reasons[i] = "r"
			}
			harness(b)["gate"].(map[string]any)["reasons"] = reasons
		}, "reasons"},
		{"a reason that is too long", func(b map[string]any) {
			harness(b)["gate"].(map[string]any)["reasons"] = []any{strings.Repeat("x", 501)}
		}, "reasons"},
		{"an empty reason", func(b map[string]any) { harness(b)["gate"].(map[string]any)["reasons"] = []any{""} }, "reasons"},
		{"a config value that is too long", func(b map[string]any) { harness(b)["config"] = map[string]any{"model": strings.Repeat("m", 201)} }, "config"},
		{"a config key in capitals", func(b map[string]any) { harness(b)["config"] = map[string]any{"Model": "x"} }, "config"},
		{"an organization named in the body", func(b map[string]any) { b["org_id"] = "org-b" }, "unknown field"},
		{"extra data after the record", nil, "after the record"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var raw []byte
			if tc.change == nil {
				raw = append(mutate(t, func(map[string]any) {}), []byte(` {"strategy":"x"}`)...)
			} else {
				raw = mutate(t, tc.change)
			}
			_, err := ValidateRecord(raw)
			if err == nil {
				t.Fatalf("%s was accepted", tc.name)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error %q does not mention %q", err, tc.want)
			}
		})
	}
}

func TestNaNAndInfinityCannotBeStored(t *testing.T) {
	// JSON has no NaN, but a hand-built body can carry one as a string or as 1e999; both must be refused.
	for _, value := range []string{`"NaN"`, `1e999`} {
		raw := strings.Replace(string(goldenBytes(t, "record.cell.golden.json")), `"mean_ndcg_at_10": 0.776186,
    "mean_recall_at_10"`, `"mean_ndcg_at_10": `+value+`,
    "mean_recall_at_10"`, 1)
		if raw == string(goldenBytes(t, "record.cell.golden.json")) {
			t.Fatal("the replacement did not apply: the golden's layout changed, fix this test")
		}
		if _, err := ValidateRecord([]byte(raw)); err == nil {
			t.Fatalf("a record with mean_ndcg_at_10 = %s was accepted", value)
		}
	}
}

func TestARecordIsBounded(t *testing.T) {
	big := append([]byte(`{"strategy":"harness:x","corpus":"c","scorecard":{"pad":"`), []byte(strings.Repeat("x", MaxRecordBytes))...)
	big = append(big, []byte(`"}}`)...)
	_, err := ValidateRecord(big)
	if err == nil || !strings.Contains(err.Error(), "too large") {
		t.Fatalf("an oversized record: err = %v, want a size refusal", err)
	}
}

func TestTheDirectoryIsPinnedByItsSHA256SUMS(t *testing.T) {
	sums, err := os.ReadFile(filepath.Join(recordGoldens, "SHA256SUMS"))
	if err != nil {
		t.Fatalf("SHA256SUMS is missing: %v", err)
	}
	pins := map[string]string{}
	for _, line := range strings.Split(strings.ReplaceAll(string(sums), "\r", ""), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 2 {
			pins[strings.TrimPrefix(fields[1], "*")] = fields[0]
		}
	}
	entries, err := os.ReadDir(recordGoldens)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.Name() == "SHA256SUMS" {
			continue
		}
		digest := sha256.Sum256(goldenBytes(t, entry.Name()))
		pinned, ok := pins[entry.Name()]
		if !ok {
			t.Errorf("%s is not pinned", entry.Name())
			continue
		}
		if pinned != hex.EncodeToString(digest[:]) {
			t.Errorf("%s does not match its pin: edit a golden and its pin together, in both directories", entry.Name())
		}
		delete(pins, entry.Name())
	}
	for name := range pins {
		t.Errorf("SHA256SUMS pins %s, which is not in the directory", name)
	}
}
