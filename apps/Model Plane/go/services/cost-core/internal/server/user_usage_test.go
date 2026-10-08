package server

import (
	"encoding/json"
	"net/http"
	"testing"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

func TestUserReportRequiresSignedOrgReadAndKeepsTenantBoundary(t *testing.T) {
	handler, key := protectedCostServer(t)
	writer := costToken(t, key, costTestClaims{OrgID: "org-a", ServiceID: "model-gateway", PrincipalType: "service", Scopes: []string{"cost:write"}})
	for _, body := range []string{
		`{"org_id":"org-a","user_id":"user-a","input_tokens":10,"cost_usd":1,"cost_basis":"metered"}`,
		`{"org_id":"org-a","user_id":"user-b","input_tokens":20,"cost_basis":"subscription"}`,
		`{"org_id":"org-a","input_tokens":30}`,
	} {
		if r := costRequest(handler, http.MethodPost, "/api/v1/cost/record", body, writer, "", ""); r.Code != http.StatusAccepted {
			t.Fatal(r.Code)
		}
	}
	admin := costToken(t, key, costTestClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user", Scopes: []string{"cost:org:read"}})
	member := costToken(t, key, costTestClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user"})
	for _, tc := range []struct {
		path, token string
		status      int
	}{
		{"/api/v1/cost/users", member, 403}, {"/api/v1/cost/users?org_id=org-b", admin, 403},
		{"/api/v1/cost/users?user_id=user-b", admin, 400}, {"/api/v1/cost/users?since=bad", admin, 400},
		{"/api/v1/cost/users", admin, 200},
	} {
		r := costRequest(handler, http.MethodGet, tc.path, "", tc.token, "", "")
		if r.Code != tc.status {
			t.Fatalf("%s status=%d body=%s", tc.path, r.Code, r.Body.String())
		}
		if tc.status == 200 {
			var result struct {
				Data []ledger.UserUsage `json:"data"`
			}
			if err := json.Unmarshal(r.Body.Bytes(), &result); err != nil {
				t.Fatal(err)
			}
			if len(result.Data) != 3 || result.Data[0].UserID != "" || result.Data[0].UnknownBasisEntries != 1 || result.Data[1].MeteredCostUSD != 1 || result.Data[2].SubscriptionEntries != 1 {
				t.Fatalf("report=%#v", result)
			}
		}
	}
}
