package gates

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
)

const goldenDir = "testdata/index-parity-v1"

func readGolden(t *testing.T, name string) []byte {
	t.Helper()
	body, err := os.ReadFile(filepath.Join(goldenDir, name))
	if err != nil {
		t.Fatalf("read golden %s: %v", name, err)
	}
	return body
}

// The goldens are the contract with quickwit-adapter-rs (CR-T9a vendors the directory byte
// for byte). Their sha256 pins are checked here, and a golden that this service's reader does
// not fully understand fails: a field the producer adds must be read, or ignored on purpose.
func TestTheParityGoldensMatchTheirPins(t *testing.T) {
	pins := map[string]string{}
	for _, line := range strings.Split(strings.ReplaceAll(string(readGolden(t, "SHA256SUMS")), "\r", ""), "\n") {
		fields := strings.Fields(line)
		if len(fields) != 2 {
			continue
		}
		pins[strings.TrimPrefix(fields[1], "*")] = fields[0]
	}
	entries, err := os.ReadDir(goldenDir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.Name() == "SHA256SUMS" {
			continue
		}
		sum := sha256.Sum256(readGolden(t, entry.Name()))
		if want := pins[entry.Name()]; hex.EncodeToString(sum[:]) != want {
			t.Errorf("%s does not match its pin in SHA256SUMS (edit the golden and its pin together, and tell the adapter's copy)", entry.Name())
		}
		delete(pins, entry.Name())
	}
	for name := range pins {
		t.Errorf("SHA256SUMS pins %s, which is not in the directory", name)
	}
}

func decodeStrict(t *testing.T, body []byte) ParityReport {
	t.Helper()
	var report ParityReport
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&report); err != nil {
		t.Fatalf("the golden has a field the reader does not know: %v", err)
	}
	return report
}

func TestTheGoldensAreTheContractTheReaderUnderstands(t *testing.T) {
	inSync := decodeStrict(t, readGolden(t, "response.in-sync.golden.json"))
	if inSync.Contract != ParityContract || inSync.OrgID != "org-parity-example" || inSync.Postgres.EligibleUnits != 207 {
		t.Fatalf("in-sync golden = %+v", inSync)
	}
	drift := decodeStrict(t, readGolden(t, "response.drift.golden.json"))
	if drift.Difference.MissingUnits != 155 || drift.Quickwit.DistinctUnits != 52 {
		t.Fatalf("drift golden = %+v", drift)
	}
}

func goldenReport(t *testing.T, name string) *ParityReport {
	t.Helper()
	report := decodeStrict(t, readGolden(t, name))
	return &report
}

const exampleOrg = "org-parity-example"

var matchingPostgres = PostgresSide{EligibleUnits: 207, EligibleWithContext: 206}

func TestAnInSyncIndexPassesTheGate(t *testing.T) {
	gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, goldenReport(t, "response.in-sync.golden.json"), nil)
	if !gate.Passed || gate.Status != StatusPassed || gate.Gate != GateSparseIndexInSync || gate.Value != "207/207" {
		t.Fatalf("gate = %+v, want a pass", gate)
	}
}

// The state of 2026-10-06: 52 of 207 units are in Quickwit. The gate must fail, and say how far.
func TestAQuickwitCountMismatchFailsTheGate(t *testing.T) {
	gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, goldenReport(t, "response.drift.golden.json"), nil)
	if gate.Passed || gate.Status != StatusFailed {
		t.Fatalf("gate = %+v, want failed", gate)
	}
	if gate.Value != "52/207" || !strings.Contains(gate.Message, "155 missing") {
		t.Fatalf("gate = %+v: the message must say how many units are missing", gate)
	}
}

func TestEveryKindOfDifferenceFailsTheGate(t *testing.T) {
	for name, mutate := range map[string]func(*ParityReport){
		"a missing unit": func(r *ParityReport) {
			r.Difference.MissingUnits = 1
			r.Quickwit.DistinctUnits--
		},
		"a stale unit": func(r *ParityReport) {
			r.Difference.UnexpectedUnits = 1
			r.Quickwit.DistinctUnits++
		},
		"a context body missing from the index": func(r *ParityReport) {
			r.Difference.ContextBodyMissing = 1
			r.Quickwit.WithContextBody--
		},
		"a context body the database does not have": func(r *ParityReport) {
			r.Difference.ContextBodyUnexpected = 1
			r.Quickwit.WithContextBody++
		},
	} {
		t.Run(name, func(t *testing.T) {
			report := goldenReport(t, "response.in-sync.golden.json")
			mutate(report)
			gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil)
			if gate.Passed || gate.Status != StatusFailed {
				t.Fatalf("gate = %+v, want failed", gate)
			}
		})
	}
}

// Offsetting errors must not hide each other: one unit missing and one stale is still a failure
// even though the totals agree.
func TestOffsettingErrorsStillFail(t *testing.T) {
	report := goldenReport(t, "response.in-sync.golden.json")
	report.Difference.MissingUnits = 1
	report.Difference.UnexpectedUnits = 1
	gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil)
	if gate.Passed {
		t.Fatalf("gate = %+v: a missing and a stale unit cancelled out", gate)
	}
}

// Duplicate documents await a merge; retrieval de-duplicates, so they are said, not failed.
func TestDuplicateDocumentsAreReportedNotFailed(t *testing.T) {
	report := goldenReport(t, "response.in-sync.golden.json")
	report.Quickwit.Documents = 215
	gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil)
	if !gate.Passed || !strings.Contains(gate.Message, "8 duplicate documents") {
		t.Fatalf("gate = %+v", gate)
	}
}

// The unavailable cases: not a pass, whatever the reason the evidence cannot be read.
func TestAnUnreadableEndpointIsUnavailableNeverAPass(t *testing.T) {
	report := goldenReport(t, "response.in-sync.golden.json")
	for name, run := range map[string]func() (PostgresSide, error, *ParityReport, error){
		"the endpoint cannot be reached": func() (PostgresSide, error, *ParityReport, error) {
			return matchingPostgres, nil, nil, unavailable("down")
		},
		"a plain error": func() (PostgresSide, error, *ParityReport, error) {
			return matchingPostgres, nil, nil, errors.New("boom")
		},
		"no report at all": func() (PostgresSide, error, *ParityReport, error) { return matchingPostgres, nil, nil, nil },
		"the database side cannot be read": func() (PostgresSide, error, *ParityReport, error) {
			return PostgresSide{}, errors.New("boom"), report, nil
		},
		"a contract this reader cannot read": func() (PostgresSide, error, *ParityReport, error) {
			other := *report
			other.Contract = "index-parity-v2"
			return matchingPostgres, nil, &other, nil
		},
		"no contract field": func() (PostgresSide, error, *ParityReport, error) {
			other := *report
			other.Contract = ""
			return matchingPostgres, nil, &other, nil
		},
	} {
		t.Run(name, func(t *testing.T) {
			pg, pgErr, rep, fetchErr := run()
			gate := EvaluateSparseParity(exampleOrg, pg, pgErr, rep, fetchErr)
			if gate.Passed || gate.Status != StatusUnavailable {
				t.Fatalf("gate = %+v, want unavailable and not passed", gate)
			}
			if !strings.Contains(gate.Message, "never reported as passing") {
				t.Fatalf("message = %q", gate.Message)
			}
		})
	}
}

func TestAReportForAnotherOrganizationIsNeverTrustedOrEchoed(t *testing.T) {
	report := goldenReport(t, "response.in-sync.golden.json")
	report.OrgID = "org-someone-else"
	gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil)
	if gate.Passed || gate.Status != StatusFailed {
		t.Fatalf("gate = %+v, want failed", gate)
	}
	if strings.Contains(gate.Message, "org-someone-else") || gate.Value != "" {
		t.Fatalf("the other organization leaked into the gate: %+v", gate)
	}
}

// If the adapter and this service count differently they use different eligibility rules,
// and parity against either is meaningless.
func TestDisagreeingEligibilityCountsFailTheGate(t *testing.T) {
	for name, pg := range map[string]PostgresSide{
		"units":        {EligibleUnits: 206, EligibleWithContext: 206},
		"with context": {EligibleUnits: 207, EligibleWithContext: 205},
	} {
		t.Run(name, func(t *testing.T) {
			gate := EvaluateSparseParity(exampleOrg, pg, nil, goldenReport(t, "response.in-sync.golden.json"), nil)
			if gate.Passed || gate.Status != StatusFailed || !strings.Contains(gate.Message, "disagree") {
				t.Fatalf("gate = %+v", gate)
			}
		})
	}
}

func TestAReportThatDoesNotAddUpOrIsNegativeIsNotTrusted(t *testing.T) {
	report := goldenReport(t, "response.in-sync.golden.json")
	report.Quickwit.DistinctUnits = 200 // 207 - 0 + 0 != 200
	if gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil); gate.Passed || !strings.Contains(gate.Message, "does not add up") {
		t.Fatalf("gate = %+v", gate)
	}
	report = goldenReport(t, "response.in-sync.golden.json")
	report.Difference.MissingUnits = -1
	report.Difference.UnexpectedUnits = 1
	if gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, nil); gate.Passed {
		t.Fatalf("a negative count passed: %+v", gate)
	}
}

// ---- the HTTP client, through the real authentication middleware ---------------------

type parityVerifier struct{}

func (parityVerifier) Verify(token string) (*authctx.Claims, error) {
	if token != "caller-token" {
		return nil, errors.New("invalid token")
	}
	claims := &authctx.Claims{OrgID: exampleOrg, ServiceID: "svc", PrincipalType: "service", Scopes: []string{"data:quality:admin"}, Verified: true}
	claims.Subject = "svc"
	return claims, nil
}

// fetchAs runs Fetch inside a request that went through the real middleware with the given
// bearer, which is the only way the caller's credential reaches the context.
func fetchAs(t *testing.T, source ParitySource, bearer string) (*ParityReport, error) {
	t.Helper()
	var (
		report *ParityReport
		err    error
	)
	handler := authctx.Middleware(parityVerifier{})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		report, err = source.Fetch(r.Context())
	}))
	req := httptest.NewRequest(http.MethodGet, "/v1/quality/gates", nil)
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)
	if bearer == "" || bearer != "caller-token" {
		return nil, errors.New("not authenticated")
	}
	return report, err
}

func serve(t *testing.T, status int, body []byte, seen *atomic.Value) *httptest.Server {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if seen != nil {
			seen.Store(r.Header.Get("Authorization") + "|" + r.Method + "|" + r.URL.RequestURI())
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write(body)
	}))
	t.Cleanup(server.Close)
	return server
}

func TestFetchForwardsTheCallersBearerAndNoOrganization(t *testing.T) {
	var seen atomic.Value
	server := serve(t, http.StatusOK, readGolden(t, "response.in-sync.golden.json"), &seen)
	report, err := fetchAs(t, NewHTTPParity(server.URL+"/admin/index-parity", nil, 0), "caller-token")
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if report.Contract != ParityContract || report.Postgres.EligibleUnits != 207 {
		t.Fatalf("report = %+v", report)
	}
	if got := seen.Load(); got != "Bearer caller-token|GET|/admin/index-parity" {
		t.Fatalf("the endpoint saw %q: the caller's bearer is forwarded unchanged, with a plain GET and no organization", got)
	}
}

func TestFetchWithoutACallerCredentialIsUnavailable(t *testing.T) {
	server := serve(t, http.StatusOK, readGolden(t, "response.in-sync.golden.json"), nil)
	source := NewHTTPParity(server.URL, nil, 0)
	_, err := source.Fetch(context.Background())
	var unavailableErr *UnavailableError
	if !errors.As(err, &unavailableErr) || !strings.Contains(unavailableErr.Reason, "no caller credential") {
		t.Fatalf("err = %v, want unavailable for a missing credential", err)
	}
}

func TestFetchTurnsEveryFailureIntoUnavailable(t *testing.T) {
	for name, tc := range map[string]struct {
		status int
		body   []byte
		reason string
	}{
		"the route does not exist yet": {http.StatusNotFound, []byte(`{}`), "does not exist"},
		"not implemented":              {http.StatusNotImplemented, nil, "does not exist"},
		"the credential is refused":    {http.StatusForbidden, []byte(`{}`), "refused"},
		"unauthorized":                 {http.StatusUnauthorized, nil, "refused"},
		"a server error":               {http.StatusInternalServerError, []byte(`oops`), "HTTP 500"},
		"a redirect page":              {http.StatusFound, nil, "HTTP 302"},
		"not json":                     {http.StatusOK, []byte(`<html>`), "not the index-parity-v1 JSON"},
		"an oversized body":            {http.StatusOK, append(append([]byte(`{"x":"`), bytes.Repeat([]byte("a"), maxParityBody+10)...), []byte(`"}`)...), "larger than"},
	} {
		t.Run(name, func(t *testing.T) {
			server := serve(t, tc.status, tc.body, nil)
			report, err := fetchAs(t, NewHTTPParity(server.URL, nil, 0), "caller-token")
			var unavailableErr *UnavailableError
			if report != nil || !errors.As(err, &unavailableErr) {
				t.Fatalf("report=%v err=%v, want an UnavailableError", report, err)
			}
			if !strings.Contains(unavailableErr.Reason, tc.reason) {
				t.Fatalf("reason %q does not mention %q", unavailableErr.Reason, tc.reason)
			}
			// And the gate built on it is unavailable, never a pass.
			gate := EvaluateSparseParity(exampleOrg, matchingPostgres, nil, report, err)
			if gate.Passed || gate.Status != StatusUnavailable {
				t.Fatalf("gate = %+v", gate)
			}
		})
	}
}

func TestFetchTimesOutAndSaysSo(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case <-time.After(2 * time.Second):
		case <-r.Context().Done():
		}
	}))
	t.Cleanup(server.Close)
	started := time.Now()
	_, err := fetchAs(t, NewHTTPParity(server.URL, nil, 100*time.Millisecond), "caller-token")
	var unavailableErr *UnavailableError
	if !errors.As(err, &unavailableErr) || !strings.Contains(unavailableErr.Reason, "timed out") {
		t.Fatalf("err = %v, want a timeout", err)
	}
	if time.Since(started) > 1500*time.Millisecond {
		t.Fatalf("the fetch took %v: the timeout was not honoured", time.Since(started))
	}
}

func TestAnUnreachableEndpointIsUnavailable(t *testing.T) {
	server := httptest.NewServer(http.NotFoundHandler())
	url := server.URL
	server.Close() // nothing listens any more
	_, err := fetchAs(t, NewHTTPParity(url, nil, time.Second), "caller-token")
	var unavailableErr *UnavailableError
	if !errors.As(err, &unavailableErr) || !strings.Contains(unavailableErr.Reason, "could not be reached") {
		t.Fatalf("err = %v", err)
	}
	// The reason is content-free: it carries no URL, host or credential.
	if strings.Contains(unavailableErr.Error(), "127.0.0.1") || strings.Contains(unavailableErr.Error(), "caller-token") {
		t.Fatalf("the reason leaks: %v", unavailableErr)
	}
}

// A redirect would carry the caller's bearer wherever the endpoint points: it is not followed.
func TestARedirectIsNotFollowedSoTheBearerGoesNowhereElse(t *testing.T) {
	var elsewhere atomic.Int32
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		elsewhere.Add(1)
	}))
	t.Cleanup(other.Close)
	redirecting := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, other.URL+"/collect", http.StatusTemporaryRedirect)
	}))
	t.Cleanup(redirecting.Close)

	_, err := fetchAs(t, NewHTTPParity(redirecting.URL, nil, time.Second), "caller-token")
	var unavailableErr *UnavailableError
	if !errors.As(err, &unavailableErr) || !strings.Contains(unavailableErr.Reason, "HTTP 307") {
		t.Fatalf("err = %v, want unavailable for the redirect", err)
	}
	if elsewhere.Load() != 0 {
		t.Fatal("the redirect target was contacted with the caller's bearer")
	}
}
