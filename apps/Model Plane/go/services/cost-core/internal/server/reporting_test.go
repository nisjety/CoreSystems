package server

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

type reportingLedger struct {
	*ledger.Store
	start time.Time
	fail  bool
}

func (l reportingLedger) ReportingStart(_ context.Context, org string) (*time.Time, error) {
	if l.fail {
		return nil, errors.New("source unavailable")
	}
	if org == "org-a" {
		return &l.start, nil
	}
	return nil, nil
}

func TestReportingResetPreservesHistoryAndTenantAndUserScopes(t *testing.T) {
	ctx := context.Background()
	start := time.Date(2026, 10, 5, 1, 0, 0, 0, time.UTC)
	store := reportingLedger{Store: ledger.NewStore(), start: start}
	for _, org := range []string{"org-a", "org-b"} {
		for _, user := range []string{"u1", "u2"} {
			for _, at := range []time.Time{start.Add(-time.Hour), start, start.Add(time.Hour)} {
				if err := store.RecordEntry(ctx, ledger.Entry{OrgID: org, UserID: user, InputTokens: 10, CostBasis: "metered", CreatedAt: at}); err != nil {
					t.Fatal(err)
				}
			}
		}
	}
	srv := NewServer(store)
	mux := http.NewServeMux()
	srv.RegisterRoutes(mux)
	for _, tc := range []struct {
		org, user, query string
		count            int64
		status           int
	}{
		{"org-a", "u1", "reporting=current&scope=organization", 4, 200},
		{"org-a", "u1", "reporting=current", 2, 200},
		{"org-b", "u1", "reporting=current&scope=organization", 6, 200},
		{"org-a", "u1", "scope=organization", 6, 200},
		{"org-a", "u1", "reporting=current&scope=organization&until=2026-10-05T00:00:00Z", 0, 200},
		{"org-a", "u1", "reporting=current&org_id=org-b", 0, 403},
		{"org-a", "u1", "reporting=current&user_id=u2", 0, 403},
	} {
		req := httptest.NewRequest("GET", "/api/v1/cost/aggregate?"+tc.query, nil)
		req = req.WithContext(authctx.ContextWithPrincipal(ctx, authctx.Principal{OrganizationID: tc.org, ActorID: tc.user, PrincipalType: "user", Scopes: []string{"cost:org:read"}}))
		res := httptest.NewRecorder()
		mux.ServeHTTP(res, req)
		if res.Code != tc.status {
			t.Fatalf("%s: status %d body %s", tc.query, res.Code, res.Body.String())
		}
		if res.Code == 200 {
			var body usageResponse
			if err := json.Unmarshal(res.Body.Bytes(), &body); err != nil {
				t.Fatal(err)
			}
			if body.EntryCount != tc.count {
				t.Fatalf("%s: count %d want %d", tc.query, body.EntryCount, tc.count)
			}
		}
	}
	// A baseline must never forgive quota consumption or erase audit records.
	req := httptest.NewRequest("GET", "/api/v1/cost/users", nil)
	req = req.WithContext(authctx.ContextWithPrincipal(ctx, authctx.Principal{OrganizationID: "org-a", ActorID: "u1", PrincipalType: "user", Scopes: []string{"cost:org:read"}}))
	res := httptest.NewRecorder()
	mux.ServeHTTP(res, req)
	var report struct {
		Data []ledger.UserUsage `json:"data"`
	}
	if res.Code != 200 || json.Unmarshal(res.Body.Bytes(), &report) != nil || len(report.Data) != 2 || report.Data[0].EntryCount != 2 || report.Data[1].EntryCount != 2 {
		t.Fatalf("team report did not use the same baseline: %s", res.Body.String())
	}
	u, err := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: "org-a"})
	if err != nil || u.EntryCount != 6 {
		t.Fatalf("history changed: %v %#v", err, u)
	}
	if err := store.CheckBudget(ctx, "org-a", "u1", 0, 1); err == nil {
		t.Fatal("reporting reset forgave token budget")
	}
	store.fail = true
	failing := NewServer(store)
	_, _, err = failing.reportingFilter(ctx, ledger.AggregateFilter{OrgID: "org-a"})
	if err == nil {
		t.Fatal("baseline failure must not become an empty report")
	}
}
