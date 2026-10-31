//go:build liveeval

package eval

// The live mode runs the seeded cases that reach a real review model, under a
// spend budget, and writes the run as a content-free evidence directory that
// scripts/eval-evidence.sh validate accepts. It needs the owner's approval of the
// budget and a pinned review model, so it never runs in the ordinary test pass:
//
//	LEARNING_REVIEW_MODEL=<pinned model> INFERENCE_CORE_ADDR=<host:port> \
//	LEARNING_EVAL_USD_PER_MTOK=<blended USD per million tokens> \
//	LEARNING_EVAL_SUITE_REVISION=<40-hex commit> \
//	[LEARNING_EVAL_MAX_USD=0.50] [LEARNING_EVAL_EVIDENCE_DIR=<new empty directory>] \
//	go test -tags liveeval -count=1 -v -run TestLiveSeededCases ./internal/learning/eval/
//
// Upstream cases are not in the suite: nothing in this package can replay them.
// Scripted cases replay their own reply, so they cost nothing and stay in.

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"sync"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

const (
	// approvedMaxUSD is the most a live run may spend: the budget the owner approved.
	approvedMaxUSD = 0.50
	// liveOutputCap is the output ceiling a review call is priced at. It matches
	// the reviewer's own limit on a reply.
	liveOutputCap = 4096
	// liveMaxWallMS bounds the wall clock of one case in the evidence contract.
	liveMaxWallMS = 600000
	// liveMaxActions bounds the proposals of one case in the evidence contract.
	liveMaxActions = 64
	liveSuiteID    = "learning-review-seeded"
)

var suiteRevision = regexp.MustCompile(`^[0-9a-f]{40}$`)

// liveConfig is the run's configuration, read once from the environment.
type liveConfig struct {
	addr       string
	model      string
	maxMicro   int64
	usdPerMTok float64
	revision   string
	evidence   string
}

func liveConfigFromEnv(t *testing.T) liveConfig {
	t.Helper()
	cfg := liveConfig{
		addr:     os.Getenv("INFERENCE_CORE_ADDR"),
		model:    os.Getenv("LEARNING_REVIEW_MODEL"),
		revision: os.Getenv("LEARNING_EVAL_SUITE_REVISION"),
		evidence: os.Getenv("LEARNING_EVAL_EVIDENCE_DIR"),
	}
	if cfg.addr == "" || cfg.model == "" {
		t.Fatal("INFERENCE_CORE_ADDR and LEARNING_REVIEW_MODEL are required for the live run")
	}
	if !suiteRevision.MatchString(cfg.revision) {
		t.Fatal("LEARNING_EVAL_SUITE_REVISION must be the 40-hex commit the suite runs from")
	}
	usd := approvedMaxUSD
	if raw := os.Getenv("LEARNING_EVAL_MAX_USD"); raw != "" {
		parsed, err := strconv.ParseFloat(raw, 64)
		if err != nil || parsed <= 0 || parsed > approvedMaxUSD {
			t.Fatalf("LEARNING_EVAL_MAX_USD must be above 0 and at most %.2f, the approved budget", approvedMaxUSD)
		}
		usd = parsed
	}
	cfg.maxMicro = int64(math.Round(usd * 1e6))
	if cfg.maxMicro < 1 {
		t.Fatal("LEARNING_EVAL_MAX_USD is below one micro-USD, so no call could be paid for")
	}
	raw := os.Getenv("LEARNING_EVAL_USD_PER_MTOK")
	price, err := strconv.ParseFloat(raw, 64)
	if err != nil || price <= 0 {
		t.Fatal("LEARNING_EVAL_USD_PER_MTOK must be the positive blended price of the pinned model per million tokens")
	}
	cfg.usdPerMTok = price
	return cfg
}

// spendLedger prices each live call and refuses one that could take the run past
// its budget. A call is reserved at its worst case (the whole prompt plus the
// output ceiling) before it is made, and settled to the tokens it reports after.
// A call that fails keeps its reservation, so the ledger never understates spend.
// Micro-USD per token equals the price in USD per million tokens.
type spendLedger struct {
	mu          sync.Mutex
	inner       inferer
	maxMicro    int64
	perToken    float64
	spent       int64
	caseIn      int
	caseOut     int
	caseMicro   int64
	caseReplies []byte
}

// begin resets the per-case totals before a case is replayed.
func (l *spendLedger) begin() {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.caseIn, l.caseOut, l.caseMicro, l.caseReplies = 0, 0, 0, nil
}

func (l *spendLedger) priceMicro(tokens int) int64 {
	return int64(math.Ceil(float64(tokens) * l.perToken))
}

func (l *spendLedger) Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error) {
	reserve := l.priceMicro(estimateInputTokens(in) + liveOutputCap)
	l.mu.Lock()
	if l.spent+reserve > l.maxMicro {
		l.mu.Unlock()
		return nil, errBudgetExhausted
	}
	l.spent += reserve
	l.mu.Unlock()

	resp, err := l.inner.Infer(ctx, in, opts...)

	l.mu.Lock()
	defer l.mu.Unlock()
	if err != nil {
		return nil, err
	}
	actual := l.priceMicro(int(resp.GetInputTokens()) + int(resp.GetOutputTokens()))
	l.spent += actual - reserve
	l.caseIn += int(resp.GetInputTokens())
	l.caseOut += int(resp.GetOutputTokens())
	l.caseMicro += actual
	l.caseReplies = append(l.caseReplies, resp.GetContent()...)
	return resp, nil
}

// estimateInputTokens is a deliberately high estimate of a prompt's tokens: a
// token is taken as two characters, so the reservation overstates the spend.
func estimateInputTokens(in *mpv1.InferRequest) int {
	chars := 0
	for _, m := range in.GetMessages() {
		chars += len(m.GetContent())
	}
	for _, tool := range in.GetTools() {
		chars += len(tool.GetDescription()) + len(tool.GetParametersJson())
	}
	return chars/2 + 64
}

// liveRow is one case's line in results.tsv.
type liveRow struct {
	caseID      string
	outcome     string
	verifier    string
	wallMS      int64
	actions     int
	inputTok    int
	outputTok   int
	costMicro   int64
	traceDigest string
}

// TestLiveSeededCases runs the suite against the pinned review model under the
// spend budget. A case fails when its replay disagrees with its expectations, and
// is reported limit_reached when the budget stopped its model call.
func TestLiveSeededCases(t *testing.T) {
	cfg := liveConfigFromEnv(t)
	conn, err := grpc.NewClient(cfg.addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("dial inference-core: %v", err)
	}
	defer conn.Close()

	ledger := &spendLedger{
		inner:    mpv1.NewInferenceCoreClient(conn),
		maxMicro: cfg.maxMicro,
		perToken: cfg.usdPerMTok,
	}
	cases, err := Load(SeededDir)
	if err != nil {
		t.Fatalf("load seeded cases: %v", err)
	}

	var rows []liveRow
	var caseRows [][3]string
	for _, f := range cases {
		if f.Upstream != "" {
			continue
		}
		var model inferer = ledger
		if f.Scripted {
			model = nil
		}
		ledger.begin()
		start := time.Now()
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
		res, err := Replay(ctx, f, model, cfg.model)
		cancel()
		row := liveRow{
			caseID:      f.ID,
			wallMS:      max(1, time.Since(start).Milliseconds()),
			actions:     min(len(res.Proposals), liveMaxActions),
			inputTok:    ledger.caseIn,
			outputTok:   ledger.caseOut,
			costMicro:   ledger.caseMicro,
			traceDigest: digestHex(ledger.caseReplies),
		}
		switch {
		case err != nil:
			row.outcome, row.verifier = "failed", "failed"
			t.Errorf("%s: replay: %v", f.ID, err)
		case hasLimit(res.Outcomes):
			row.outcome, row.verifier = "limit_reached", "failed"
			t.Logf("%s: stopped by the spend budget", f.ID)
		default:
			if bad := Check(f, res, true); len(bad) == 0 {
				row.outcome, row.verifier = "passed", "passed"
			} else {
				row.outcome, row.verifier = "failed", "failed"
				for _, line := range bad {
					t.Errorf("%s: %s", f.ID, line)
				}
			}
		}
		rows = append(rows, row)
		caseRows = append(caseRows, [3]string{f.ID, f.digest, policyFingerprint(f, cfg.model)})
	}
	t.Logf("live spend: %d micro-USD of %d approved (%.4f USD)", ledger.spent, cfg.maxMicro, float64(ledger.spent)/1e6)

	if cfg.evidence != "" {
		if err := writeEvidence(cfg, caseRows, rows, ledger.maxMicro); err != nil {
			t.Fatalf("write evidence: %v", err)
		}
		t.Logf("evidence written to %s; validate it with scripts/eval-evidence.sh validate %s", cfg.evidence, cfg.evidence)
	}
}

func hasLimit(outcomes []string) bool {
	for _, o := range outcomes {
		if o == OutcomeLimitReached {
			return true
		}
	}
	return false
}

func digestHex(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

// policyFingerprint names the cost and memory policy a case ran under.
func policyFingerprint(f Fixture, model string) string {
	cost := "off"
	if f.Cost != nil {
		cost = fmt.Sprintf("%d/%d/%d/%d/%s", f.Cost.MinUserTurns, f.Cost.MaxTranscriptChars, f.Cost.MaxPerUserPerDay, f.Cost.MaxPerOrgPerDay, f.Cost.Budget)
	}
	return digestHex([]byte(fmt.Sprintf("model=%s|cost=%s|controls=%t/%s", model, cost, f.Controls.LearningAllowed, f.Controls.Reason)))
}

// writeEvidence writes the four files of the evidence contract into dir: the
// manifest, the case list, the results and the checksums of those three. Nothing in
// them is content: digests, counts and outcomes only.
func writeEvidence(cfg liveConfig, caseRows [][3]string, rows []liveRow, maxMicro int64) error {
	if err := os.MkdirAll(cfg.evidence, 0o755); err != nil {
		return err
	}
	entries, err := os.ReadDir(cfg.evidence)
	if err != nil {
		return err
	}
	if len(entries) != 0 {
		return fmt.Errorf("evidence directory %s must be new and empty", cfg.evidence)
	}

	var cases bytes.Buffer
	cases.WriteString("case_id\tfixture_sha256\tpolicy_fingerprint_sha256\n")
	for _, c := range caseRows {
		fmt.Fprintf(&cases, "%s\t%s\t%s\n", c[0], c[1], c[2])
	}

	var results bytes.Buffer
	results.WriteString("case_id\tattempt\toutcome\tverifier\twall_clock_ms\taction_count\tinput_tokens\toutput_tokens\tcost_micro_usd\trecovery_count\ttrace_sha256\n")
	for _, r := range rows {
		fmt.Fprintf(&results, "%s\t1\t%s\t%s\t%d\t%d\t%d\t%d\t%d\t0\t%s\n",
			r.caseID, r.outcome, r.verifier, r.wallMS, r.actions, r.inputTok, r.outputTok, r.costMicro, r.traceDigest)
	}

	executor, err := os.ReadFile("live_test.go")
	if err != nil {
		return err
	}
	verifier := []byte{}
	for _, name := range []string{"fixture.go", "replay.go"} {
		src, err := os.ReadFile(name)
		if err != nil {
			return err
		}
		verifier = append(verifier, src...)
	}
	casesSum, resultsSum := digestHex(cases.Bytes()), digestHex(results.Bytes())
	maxTokens := int64(float64(maxMicro) / cfg.usdPerMTok)
	if maxTokens < 1 {
		return fmt.Errorf("the budget buys no tokens at the configured price")
	}
	var policies []byte
	for _, c := range caseRows {
		policies = append(policies, c[2]...)
	}
	manifest := fmt.Sprintf(`EVAL_MANIFEST_VERSION=1
SUITE_ID=%s
SUITE_REVISION=%s
EVALUATED_AT=%s
LANE=model-only
MAX_ATTEMPTS=1
MAX_WALL_CLOCK_MS=%d
MAX_ACTION_COUNT=%d
MAX_INPUT_TOKENS=%d
MAX_OUTPUT_TOKENS=%d
MAX_COST_MICRO_USD=%d
TRACE_RETENTION=zdr-safe-digest-only
MODEL_PROVIDER_FINGERPRINT_SHA256=%s
PROMPT_FINGERPRINT_SHA256=%s
TOOL_CAPABILITY_FINGERPRINT_SHA256=%s
MEMORY_RETRIEVAL_FINGERPRINT_SHA256=%s
FEEDBACK_FINGERPRINT_SHA256=%s
POLICY_FINGERPRINT_SHA256=%s
EXECUTOR_IMPLEMENTATION_SHA256=%s
INDEPENDENT_VERIFIER_IMPLEMENTATION_SHA256=%s
CASES_SHA256=%s
RESULTS_SHA256=%s
`,
		liveSuiteID, cfg.revision, time.Now().UTC().Format("2006-01-02T15:04:05Z"),
		liveMaxWallMS, liveMaxActions, maxTokens, maxTokens, maxMicro,
		digestHex([]byte("model:"+cfg.model)),
		digestHex([]byte(learning.ReviewPrompt)),
		digestHex([]byte("tool:submit_skill_review")),
		digestHex([]byte("memory:none")),
		digestHex([]byte("feedback:none")),
		digestHex(policies),
		digestHex(executor),
		digestHex(verifier),
		casesSum, resultsSum,
	)
	manifestBytes := []byte(manifest)

	files := map[string][]byte{
		"cases.tsv":         cases.Bytes(),
		"results.tsv":       results.Bytes(),
		"eval-manifest.env": manifestBytes,
	}
	for name, body := range files {
		if err := os.WriteFile(filepath.Join(cfg.evidence, name), body, 0o644); err != nil {
			return err
		}
	}
	var index bytes.Buffer
	index.WriteString("path\tsha256\tbytes\n")
	for _, name := range []string{"cases.tsv", "eval-manifest.env", "results.tsv"} {
		fmt.Fprintf(&index, "%s\t%s\t%d\n", name, digestHex(files[name]), len(files[name]))
	}
	return os.WriteFile(filepath.Join(cfg.evidence, "evidence-manifest.tsv"), index.Bytes(), 0o644)
}
