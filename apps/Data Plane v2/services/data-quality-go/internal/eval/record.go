package eval

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// RecordSchema is the contract of a harness record. The wire contract is pinned by
// testdata/harness-record-v1/ (valid records, and the invalid ones this service must refuse), which the
// producer, scripts/eval-gate.py, vendors byte for byte; scripts/check-eval-contract.sh is the repo-level
// drift check.
const RecordSchema = "harness-record-v1"

// MaxRecordBytes bounds a record: it is a summary of a run, a few hundred bytes. The per-query rows stay
// in the run's own artifacts.
const MaxRecordBytes = 64 << 10

const (
	maxReasons      = 20
	maxReasonRunes  = 500
	maxConfigPairs  = 16
	maxConfigValue  = 200
	harnessStrategy = "harness:"
)

var (
	strategyPattern = regexp.MustCompile(`^harness:[a-z0-9][a-z0-9._-]{0,62}$`)
	cellPattern     = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]{0,62}$`)
	runIDPattern    = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{3,63}$`)
	classPattern    = regexp.MustCompile(`^[a-z][a-z0-9_]{0,47}$`)
	corpusPattern   = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._@:/-]{0,199}$`)
	digestPattern   = regexp.MustCompile(`^[0-9a-f]{64}$`)
	configKey       = regexp.MustCompile(`^[a-z][a-z0-9_]{0,31}$`)
)

var (
	harnessKinds    = map[string]bool{"cell": true, "promotion": true}
	harnessReranks  = map[string]bool{"on": true, "off": true, "default": true}
	harnessOrgs     = map[string]bool{"baseline": true, "contextual": true}
	harnessVerdicts = map[string]bool{"pass": true, "fail": true, "unbaselined": true, "promote": true, "hold": true}
)

// HarnessScorecard is the scorecard of a harness record: the service's own Scorecard (so the same
// reader that shows an in-service run shows a recorded one) plus what only the harness knows.
type HarnessScorecard struct {
	model.Scorecard
	Harness HarnessMeta `json:"harness"`
}

// HarnessMeta says what was measured, on which corpus, and how the gate judged it.
type HarnessMeta struct {
	Schema     string            `json:"schema"`
	Kind       string            `json:"kind"`
	Cell       string            `json:"cell"`
	RunID      string            `json:"run_id"`
	OrgLabel   string            `json:"org_label"`
	QueryClass string            `json:"query_class"`
	Rerank     string            `json:"rerank"`
	ZeroResult int               `json:"zero_result"`
	Corpus     HarnessCorpus     `json:"corpus"`
	Gate       HarnessGate       `json:"gate"`
	Config     map[string]string `json:"config,omitempty"`
}

// HarnessCorpus identifies the frozen corpus the run was measured on.
type HarnessCorpus struct {
	ID             string `json:"id"`
	ManifestDigest string `json:"manifest_digest"`
	Documents      int    `json:"documents"`
}

// HarnessGate is the gate's verdict on the run, with the reasons it gave.
type HarnessGate struct {
	Verdict string   `json:"verdict"`
	Reasons []string `json:"reasons"`
}

// Record is a validated record, ready to store. Scorecard is the scorecard exactly as it was sent
// (compacted), which is what the run row will hold.
type Record struct {
	Strategy  string
	Corpus    string
	Scorecard json.RawMessage
	Harness   HarnessMeta
}

// InvalidRecordError is a record the service refuses. The problems are content-free: they name fields
// and rules, never values.
type InvalidRecordError struct{ Problems []string }

func (e *InvalidRecordError) Error() string {
	return "invalid evaluation record: " + strings.Join(e.Problems, "; ")
}

type recordRequest struct {
	Strategy  string          `json:"strategy"`
	Corpus    string          `json:"corpus"`
	Scorecard json.RawMessage `json:"scorecard"`
}

func invalid(format string, args ...any) *InvalidRecordError {
	return &InvalidRecordError{Problems: []string{fmt.Sprintf(format, args...)}}
}

func strictDecode(raw []byte, into any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(into); err != nil {
		return err
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return errors.New("unexpected data after the record")
	}
	return nil
}

func finite(v float64) bool { return !math.IsNaN(v) && !math.IsInf(v, 0) }

func unit(v float64) bool { return finite(v) && v >= 0 && v <= 1 }

// ValidateRecord is the rule set of a harness record: scripts/eval-gate.py's validate_record mirrors it
// rule for rule, and the pinned goldens of both directions keep the two equal.
func ValidateRecord(raw []byte) (*Record, error) {
	if len(raw) > MaxRecordBytes {
		return nil, invalid("the record is too large (more than %d bytes): it is a summary, the per-query rows stay in the run's artifacts", MaxRecordBytes)
	}
	var req recordRequest
	if err := strictDecode(raw, &req); err != nil {
		return nil, invalid("%s", err.Error())
	}
	trimmed := bytes.TrimSpace(req.Scorecard)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return nil, invalid("scorecard is missing")
	}
	var card HarnessScorecard
	if err := strictDecode(req.Scorecard, &card); err != nil {
		return nil, invalid("scorecard: %s", err.Error())
	}

	var problems []string
	add := func(format string, args ...any) { problems = append(problems, fmt.Sprintf(format, args...)) }

	if !strategyPattern.MatchString(req.Strategy) {
		add("strategy must be harness:<cell> (lower case letters, digits, . _ -)")
	}
	if !corpusPattern.MatchString(req.Corpus) {
		add("corpus is missing or has characters outside A-Z a-z 0-9 . _ @ : / -")
	}
	if card.Strategy != req.Strategy {
		add("scorecard.strategy must equal strategy")
	}
	if len(card.Details) > 0 {
		add("details must be absent: a record is a summary, the per-query rows stay in the run's artifacts")
	}
	queries := card.QueriesRun
	if queries < 1 {
		add("queries_run must be an integer of at least 1")
	}
	if card.GoldenQueries < 1 || (queries >= 1 && card.GoldenQueries > queries) {
		add("golden_queries must be an integer from 1 to queries_run")
	}
	for _, metric := range []struct {
		name  string
		value float64
	}{
		{"mean_recall_at_10", card.MeanRecall}, {"mean_ndcg_at_10", card.MeanNDCG}, {"mean_mrr", card.MeanMRR},
	} {
		if !unit(metric.value) {
			add("%s must be a number from 0 to 1", metric.name)
		}
	}
	for _, latency := range []struct {
		name  string
		value float64
	}{{"mean_latency_ms", card.MeanLatency}, {"p95_latency_ms", card.P95Latency}} {
		if !finite(latency.value) || latency.value < 0 {
			add("%s must be a number of at least 0", latency.name)
		}
	}
	if card.ByClass != nil {
		if len(card.ByClass) == 0 {
			add("by_class must be a non-empty object when present")
		}
		total := 0
		for name, summary := range card.ByClass {
			if !classPattern.MatchString(name) {
				add("by_class has an invalid class name")
				continue
			}
			if summary.QueriesRun < 1 {
				add("by_class.%s.queries_run must be an integer of at least 1", name)
			}
			total += summary.QueriesRun
			if !unit(summary.MeanRecall) || !unit(summary.MeanNDCG) || !unit(summary.MeanMRR) {
				add("by_class.%s metrics must be numbers from 0 to 1", name)
			}
		}
		if len(card.ByClass) > 0 && card.GoldenQueries >= 1 && total != card.GoldenQueries {
			add("by_class queries add up to %d, not golden_queries %d", total, card.GoldenQueries)
		}
	}

	h := card.Harness
	if h.Schema != RecordSchema {
		add("harness.schema must be %s", RecordSchema)
	}
	if !harnessKinds[h.Kind] {
		add("harness.kind must be cell or promotion")
	}
	if !cellPattern.MatchString(h.Cell) {
		add("harness.cell is missing or invalid")
	} else if req.Strategy != harnessStrategy+h.Cell {
		add("strategy must be harness:<harness.cell>")
	}
	if !runIDPattern.MatchString(h.RunID) {
		add("harness.run_id must be 4 to 64 characters of A-Z a-z 0-9 . _ -")
	}
	if !harnessOrgs[h.OrgLabel] {
		add("harness.org_label must be baseline or contextual")
	}
	if !classPattern.MatchString(h.QueryClass) {
		add("harness.query_class is missing or invalid")
	}
	if !harnessReranks[h.Rerank] {
		add("harness.rerank must be on, off or default")
	}
	if h.ZeroResult < 0 || (queries >= 1 && h.ZeroResult > queries) {
		add("harness.zero_result must be an integer from 0 to queries_run")
	}
	if !corpusPattern.MatchString(h.Corpus.ID) {
		add("harness.corpus.id is invalid")
	}
	if !digestPattern.MatchString(h.Corpus.ManifestDigest) {
		add("harness.corpus.manifest_digest must be 64 hex characters")
	}
	if h.Corpus.Documents < 1 {
		add("harness.corpus.documents must be an integer of at least 1")
	}
	if !harnessVerdicts[h.Gate.Verdict] {
		add("harness.gate.verdict must be one of pass, fail, unbaselined, promote, hold")
	}
	if len(h.Gate.Reasons) > maxReasons {
		add("harness.gate.reasons must be at most %d texts of 1 to %d characters", maxReasons, maxReasonRunes)
	}
	for _, reason := range h.Gate.Reasons {
		if n := utf8.RuneCountInString(reason); n < 1 || n > maxReasonRunes {
			add("harness.gate.reasons must be at most %d texts of 1 to %d characters", maxReasons, maxReasonRunes)
			break
		}
	}
	if len(h.Config) > maxConfigPairs {
		add("harness.config must be at most %d short text values under lower case keys", maxConfigPairs)
	}
	for key, value := range h.Config {
		if !configKey.MatchString(key) || utf8.RuneCountInString(value) > maxConfigValue {
			add("harness.config must be at most %d short text values under lower case keys", maxConfigPairs)
			break
		}
	}
	if len(problems) > 0 {
		return nil, &InvalidRecordError{Problems: problems}
	}

	var compact bytes.Buffer
	if err := json.Compact(&compact, req.Scorecard); err != nil {
		return nil, invalid("scorecard: %s", err.Error())
	}
	return &Record{Strategy: req.Strategy, Corpus: req.Corpus, Scorecard: compact.Bytes(), Harness: h}, nil
}
