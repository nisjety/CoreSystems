package server

import (
	"context"
	"encoding/json"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestAnalyticsScopeBaselineAndBillingTypes(t *testing.T) {
	start := time.Date(2025, 1, 2, 0, 0, 0, 0, time.UTC)
	store := reportingLedger{Store: ledger.NewStore(), start: start}
	for _, org := range []string{"org-a", "org-b"} {
		for _, user := range []string{"u1", "u2"} {
			for _, at := range []time.Time{start.Add(-time.Minute), start, start.Add(time.Hour)} {
				for _, basis := range []string{"metered", "subscription", ""} {
					err := store.RecordEntry(context.Background(), ledger.Entry{OrgID: org, UserID: user, Model: "model-1", ProviderUsed: "provider", InputTokens: 100, OutputTokens: 2, CostUSD: 0.1, CostBasis: basis, CreatedAt: at})
					if err != nil {
						t.Fatal(err)
					}
				}
			}
		}
	}
	mux := http.NewServeMux()
	NewServer(store).RegisterRoutes(mux)
	for _, tc := range []struct {
		query, org, user string
		admin            bool
		status           int
		requests         int64
	}{
		{"scope=organization", "org-a", "u1", true, 200, 12},
		{"scope=me", "org-a", "u1", true, 200, 6},
		{"", "org-b", "u2", false, 200, 9},
		{"scope=organization", "org-a", "u1", false, 403, 0},
		{"org_id=org-b", "org-a", "u1", true, 403, 0},
		{"user_id=u2", "org-a", "u1", true, 403, 0},
		{"scope=organization&user_id=u2", "org-a", "u1", true, 400, 0},
		{"scope=invalid", "org-a", "u1", true, 400, 0},
		{"until=2025-01-01T23:00:00Z", "org-a", "u1", true, 200, 0},
		{"reporting=all", "org-a", "u1", true, 200, 6},
	} {
		p := authctx.Principal{OrganizationID: tc.org, ActorID: tc.user, PrincipalType: "user"}
		if tc.admin {
			p.Scopes = []string{"cost:org:read"}
		}
		req := httptest.NewRequest("GET", "/api/v1/cost/analytics?"+tc.query, nil)
		req = req.WithContext(authctx.ContextWithPrincipal(req.Context(), p))
		res := httptest.NewRecorder()
		mux.ServeHTTP(res, req)
		if res.Code != tc.status {
			t.Fatalf("%s: %d %s", tc.query, res.Code, res.Body.String())
		}
		if tc.status != 200 {
			continue
		}
		var result struct {
			Data struct {
				Buckets []ledger.UsageBucket `json:"buckets"`
			}
		}
		if err := json.Unmarshal(res.Body.Bytes(), &result); err != nil {
			t.Fatal(err)
		}
		var count int64
		for _, b := range result.Data.Buckets {
			count += b.Requests
			if b.InputTokens != b.Requests*100 || b.OutputTokens != b.Requests*2 {
				t.Fatalf("wrong tokens: %#v", b)
			}
			if b.Basis != "metered" && b.MeteredCostUSD != 0 {
				t.Fatal("unknown/subscription cost counted as API spend")
			}
		}
		if count != tc.requests {
			t.Fatalf("%s: count %d want %d", tc.query, count, tc.requests)
		}
	}
}
