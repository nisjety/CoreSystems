package handler

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gates"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// eval-evidence-v1 is the contract between this service (producer) and the retrieval-quality
// harness gate, scripts/eval-gate.py (consumer, CR-T14): the response bodies of
//
//	GET /v1/quality/context-coverage   (the counts, the policy and how the 99 percent rule judged them)
//	GET /v1/quality/gates              (the release-gate report, with sparse_index_in_sync)
//
// The goldens in testdata/eval-evidence-v1/ are produced here, by the REAL route table, the REAL
// handler and the REAL rules (gates.EvaluateContextCoverage, gates.EvaluateSparseParity): never
// written by hand. The consumer vendors the directory byte for byte under
// apps/Data Plane v2/scripts/tests/fixtures/eval-evidence-v1/ with the same SHA256SUMS, and
// scripts/check-eval-contract.sh is the repo-level drift check.
//
// To change a golden on purpose: run this test with UPDATE_EVAL_EVIDENCE_GOLDEN=1, copy the
// directory to the consumer, refresh SHA256SUMS in both, and say why in the commit.

const evalEvidenceDir = "testdata/eval-evidence-v1"

const (
	evalOrgBaseline   = "org-corpus-baseline"
	evalOrgContextual = "org-corpus-contextual"
)

var evalEvidenceTokens = tokenVerifier{
	"seeder-baseline":   serviceClaims("corpus-seeder", evalOrgBaseline, adminScope),
	"seeder-contextual": serviceClaims("corpus-seeder", evalOrgContextual, adminScope),
}

type fixedCoverage struct {
	reports map[string]*gates.CoverageReport
}

func (f fixedCoverage) ContextCoverage(_ context.Context, orgID string) (*gates.CoverageReport, error) {
	return f.reports[orgID], nil
}

func coverageReportFor(org string, counts gates.CoverageCounts, setting gates.PolicySetting) *gates.CoverageReport {
	report := &gates.CoverageReport{OrgID: org, Counts: counts, Policy: setting}
	report.Evaluation = gates.EvaluateContextCoverage(counts, setting, gates.DefaultCoverageThresholdPercent)
	return report
}

// evalEvidenceCases are the organizations' states the harness must be able to read. The numbers
// are the shape of the eval corpus (the same 1,164 chunks in both organizations), not measurements.
func evalEvidenceCases() (coverage map[string]*gates.CoverageReport, tokens map[string]string) {
	contextualReady := coverageReportFor(evalOrgContextual,
		gates.CoverageCounts{Eligible: 1164, Ready: 1164},
		gates.PolicySetting{ContextualRetrieval: "inherit", Source: "default"})
	contextualBelow := coverageReportFor(evalOrgContextual,
		gates.CoverageCounts{Eligible: 1164, Ready: 1100, Legacy: 24, Pending: 40},
		gates.PolicySetting{ContextualRetrieval: "inherit", Source: "default"})
	baselineOff := coverageReportFor(evalOrgBaseline,
		gates.CoverageCounts{Eligible: 1164, SkippedPolicy: 1164},
		gates.PolicySetting{ContextualRetrieval: "off", Source: "policy"})
	return map[string]*gates.CoverageReport{
			"coverage.contextual-ready.golden.json":           contextualReady,
			"coverage.contextual-below-threshold.golden.json": contextualBelow,
			"coverage.baseline-off.golden.json":               baselineOff,
		}, map[string]string{
			"coverage.contextual-ready.golden.json":           "seeder-contextual",
			"coverage.contextual-below-threshold.golden.json": "seeder-contextual",
			"coverage.baseline-off.golden.json":               "seeder-baseline",
		}
}

func evalEvidenceGateReports() map[string]model.GateReport {
	ready := coverageReportFor(evalOrgContextual,
		gates.CoverageCounts{Eligible: 1164, Ready: 1164},
		gates.PolicySetting{ContextualRetrieval: "inherit", Source: "default"})
	pg := gates.PostgresSide{EligibleUnits: 1164, EligibleWithContext: 1164}
	var inSync gates.ParityReport
	inSync.Contract = gates.ParityContract
	inSync.OrgID = evalOrgContextual
	inSync.CheckedAt = "2026-10-07T03:20:00Z"
	inSync.Postgres.EligibleUnits, inSync.Postgres.EligibleWithContext = 1164, 1164
	inSync.Quickwit.Documents, inSync.Quickwit.DistinctUnits, inSync.Quickwit.WithContextBody = 1164, 1164, 1164

	parityOK := gates.EvaluateSparseParity(evalOrgContextual, pg, nil, &inSync, nil)
	parityUnavailable := gates.EvaluateSparseParity(evalOrgContextual, pg, nil, nil, &gates.UnavailableError{Reason: "SPARSE_INDEX_PARITY_URL is not configured"})

	report := func(parity model.GateResult) model.GateReport {
		results := []model.GateResult{ready.Evaluation.GateOf(), parity}
		all := true
		for _, result := range results {
			all = all && result.Passed
		}
		return model.GateReport{AllPassed: all, Gates: results}
	}
	return map[string]model.GateReport{
		"gates.in-sync.golden.json":            report(parityOK),
		"gates.parity-unavailable.golden.json": report(parityUnavailable),
	}
}

func prettyJSON(t *testing.T, raw []byte) []byte {
	t.Helper()
	var buf bytes.Buffer
	if err := json.Indent(&buf, raw, "", "  "); err != nil {
		t.Fatalf("indent: %v", err)
	}
	buf.WriteByte('\n')
	return buf.Bytes()
}

func sameJSON(t *testing.T, a, b []byte) bool {
	t.Helper()
	var left, right any
	if err := json.Unmarshal(a, &left); err != nil {
		t.Fatalf("decode produced body: %v", err)
	}
	if err := json.Unmarshal(b, &right); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	return reflect.DeepEqual(left, right)
}

func checkGolden(t *testing.T, name string, produced []byte) {
	t.Helper()
	path := filepath.Join(evalEvidenceDir, name)
	pretty := prettyJSON(t, produced)
	if os.Getenv("UPDATE_EVAL_EVIDENCE_GOLDEN") != "" {
		if err := os.MkdirAll(evalEvidenceDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, pretty, 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	golden, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("golden %s is missing (%v): run with UPDATE_EVAL_EVIDENCE_GOLDEN=1, then vendor the directory to scripts/tests/fixtures/eval-evidence-v1/ and refresh both SHA256SUMS", name, err)
	}
	if !sameJSON(t, produced, golden) {
		t.Fatalf("%s differs from what the real handler and rules produce now.\nproduced:\n%s\ngolden:\n%s\nA deliberate change: UPDATE_EVAL_EVIDENCE_GOLDEN=1, vendor the directory to the consumer, refresh SHA256SUMS in both",
			name, pretty, golden)
	}
}

func TestEvalEvidenceCoverageGoldensAreWhatTheRealHandlerAndRulesProduce(t *testing.T) {
	reports, tokens := evalEvidenceCases()
	router := chi.NewRouter()
	MountProtectedRoutes(router, authctx.Middleware(evalEvidenceTokens),
		(&QualityHandler{}).WithCoverageReader(fixedCoverage{reports: map[string]*gates.CoverageReport{
			evalOrgContextual: reports["coverage.contextual-ready.golden.json"],
			evalOrgBaseline:   reports["coverage.baseline-off.golden.json"],
		}}))
	// The below-threshold case needs its own reader: the same organization, a different state.
	belowRouter := chi.NewRouter()
	MountProtectedRoutes(belowRouter, authctx.Middleware(evalEvidenceTokens),
		(&QualityHandler{}).WithCoverageReader(fixedCoverage{reports: map[string]*gates.CoverageReport{
			evalOrgContextual: reports["coverage.contextual-below-threshold.golden.json"],
		}}))

	names := make([]string, 0, len(reports))
	for name := range reports {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		t.Run(name, func(t *testing.T) {
			var handler http.Handler = router
			if strings.Contains(name, "below-threshold") {
				handler = belowRouter
			}
			got := call(t, handler, http.MethodGet, coveragePath, tokens[name], "")
			if got.Code != http.StatusOK {
				t.Fatalf("GET %s = %d %s", coveragePath, got.Code, got.Raw)
			}
			checkGolden(t, name, []byte(got.Raw))
		})
	}
}

func TestEvalEvidenceGatesGoldensAreWhatTheRealRulesProduce(t *testing.T) {
	reports := evalEvidenceGateReports()
	names := make([]string, 0, len(reports))
	for name := range reports {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		t.Run(name, func(t *testing.T) {
			raw, err := json.Marshal(reports[name])
			if err != nil {
				t.Fatal(err)
			}
			checkGolden(t, name, raw)
		})
	}
	// The two goldens must differ in exactly what the harness reads: the parity gate's status.
	if !reports["gates.in-sync.golden.json"].AllPassed || reports["gates.parity-unavailable.golden.json"].AllPassed {
		t.Fatalf("the in-sync report must pass and the unavailable one must not (unread evidence is never a pass)")
	}
}

func TestEvalEvidenceDirectoryIsPinnedByItsSHA256SUMS(t *testing.T) {
	if os.Getenv("UPDATE_EVAL_EVIDENCE_GOLDEN") != "" {
		t.Skip("goldens are being regenerated; refresh SHA256SUMS next")
	}
	entries, err := os.ReadDir(evalEvidenceDir)
	if err != nil {
		t.Fatalf("read %s: %v", evalEvidenceDir, err)
	}
	pins := map[string]string{}
	sums, err := os.ReadFile(filepath.Join(evalEvidenceDir, "SHA256SUMS"))
	if err != nil {
		t.Fatalf("SHA256SUMS is missing: %v", err)
	}
	for _, line := range strings.Split(strings.ReplaceAll(string(sums), "\r", ""), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 2 {
			pins[strings.TrimPrefix(fields[1], "*")] = fields[0]
		}
	}
	for _, entry := range entries {
		if entry.Name() == "SHA256SUMS" {
			continue
		}
		raw, err := os.ReadFile(filepath.Join(evalEvidenceDir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(raw)
		pinned, ok := pins[entry.Name()]
		if !ok {
			t.Errorf("%s is not pinned in SHA256SUMS", entry.Name())
			continue
		}
		if pinned != hex.EncodeToString(digest[:]) {
			t.Errorf("%s does not match its pin in SHA256SUMS: edit a golden and its pin together", entry.Name())
		}
		delete(pins, entry.Name())
	}
	for name := range pins {
		t.Errorf("SHA256SUMS pins %s, which is not in the directory", name)
	}
}
