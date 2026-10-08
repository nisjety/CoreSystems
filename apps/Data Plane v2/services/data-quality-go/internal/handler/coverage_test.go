package handler

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/gates"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

type fakeCoverage struct {
	report *gates.CoverageReport
	err    error
	orgs   []string
}

func (f *fakeCoverage) ContextCoverage(_ context.Context, orgID string) (*gates.CoverageReport, error) {
	f.orgs = append(f.orgs, orgID)
	if f.err != nil {
		return nil, f.err
	}
	return f.report, nil
}

func coverageRouter(reader CoverageReader) http.Handler {
	router := chi.NewRouter()
	h := &QualityHandler{}
	if reader != nil {
		h.WithCoverageReader(reader)
	}
	MountProtectedRoutes(router, authctx.Middleware(testTokens), h)
	return router
}

const coveragePath = "/v1/quality/context-coverage"

func TestTheCoverageEndpointRequiresAuthenticationAndTheAdminScope(t *testing.T) {
	reader := &fakeCoverage{report: &gates.CoverageReport{}}
	router := coverageRouter(reader)
	if got := call(t, router, http.MethodGet, coveragePath, "", ""); got.Code != http.StatusUnauthorized {
		t.Fatalf("no token = %d, want 401", got.Code)
	}
	if got := call(t, router, http.MethodGet, coveragePath, "member-user-a", ""); got.Code != http.StatusForbidden {
		t.Fatalf("a member without the admin scope = %d, want 403", got.Code)
	}
	if got := call(t, router, http.MethodGet, coveragePath, "operator-a", "", "X-Org-ID", "org-b"); got.Code != http.StatusForbidden {
		t.Fatalf("a forged tenant header = %d, want 403", got.Code)
	}
	if len(reader.orgs) != 0 {
		t.Fatalf("a refused request reached the reader: %v", reader.orgs)
	}
}

// The organization is the verified token's. There is no org parameter, and a query string that
// names another organization is ignored.
func TestTheCoverageEndpointReadsTheTokensOrganizationAndNoOther(t *testing.T) {
	report := &gates.CoverageReport{
		OrgID:  "org-a",
		Counts: gates.CoverageCounts{Eligible: 207, Legacy: 206, Pending: 1},
		Policy: gates.PolicySetting{ContextualRetrieval: "inherit", Source: "default"},
	}
	report.Evaluation = gates.EvaluateContextCoverage(report.Counts, report.Policy, 99)
	reader := &fakeCoverage{report: report}

	got := call(t, coverageRouter(reader), http.MethodGet, coveragePath+"?org_id=org-b&org=org-b", "operator-a", "")
	if got.Code != http.StatusOK {
		t.Fatalf("GET = %d %s", got.Code, got.Raw)
	}
	if len(reader.orgs) != 1 || reader.orgs[0] != "org-a" {
		t.Fatalf("the reader was asked for %v, want exactly the token's org-a", reader.orgs)
	}
	counts, _ := got.Body["counts"].(map[string]any)
	if counts["eligible"] != float64(207) || counts["legacy"] != float64(206) || counts["pending"] != float64(1) {
		t.Fatalf("counts = %v", counts)
	}
	evaluation, _ := got.Body["evaluation"].(map[string]any)
	if evaluation["status"] != "failed" || evaluation["denominator"] != float64(207) {
		t.Fatalf("evaluation = %v, want the failed 0/207 of the live state", evaluation)
	}
	if got.Body["org_id"] != "org-a" {
		t.Fatalf("org_id = %v", got.Body["org_id"])
	}
	// An organization admin may read their own coverage: it is not a policy change.
	if got := call(t, coverageRouter(reader), http.MethodGet, coveragePath, "admin-user-a", ""); got.Code != http.StatusOK {
		t.Fatalf("an org admin's GET = %d", got.Code)
	}
}

func TestTheCoverageEndpointMapsAMissingViewTo503AndOtherFailuresTo500(t *testing.T) {
	missing := &fakeCoverage{err: fmt.Errorf("wrapped: %w", gates.ErrCoverageUnavailable)}
	got := call(t, coverageRouter(missing), http.MethodGet, coveragePath, "operator-a", "")
	if got.Code != http.StatusServiceUnavailable || errorCode(got) != "coverage_unavailable" {
		t.Fatalf("a missing view = %d %s, want 503 coverage_unavailable", got.Code, got.Raw)
	}
	missingPolicy := &fakeCoverage{err: fmt.Errorf("wrapped: %w", policy.ErrNotMigrated)}
	if got := call(t, coverageRouter(missingPolicy), http.MethodGet, coveragePath, "operator-a", ""); got.Code != http.StatusServiceUnavailable {
		t.Fatalf("a missing policy table = %d, want 503", got.Code)
	}
	broken := &fakeCoverage{err: errors.New("ERROR: deadlock detected (SQLSTATE 40P01)")}
	got = call(t, coverageRouter(broken), http.MethodGet, coveragePath, "operator-a", "")
	if got.Code != http.StatusInternalServerError || strings.Contains(got.Raw, "deadlock") {
		t.Fatalf("a read failure = %d %s, want 500 without database detail", got.Code, got.Raw)
	}
}

func TestTheCoverageEndpointWithoutAReaderAnswers503(t *testing.T) {
	if got := call(t, coverageRouter(nil), http.MethodGet, coveragePath, "operator-a", ""); got.Code != http.StatusServiceUnavailable {
		t.Fatalf("no reader = %d, want 503", got.Code)
	}
}
