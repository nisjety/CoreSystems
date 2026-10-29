package api

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

func TestSpaceCronDeletionIsCoordinatorAndTenantBound(t *testing.T) {
	database := &recordingDatabase{execTag: "UPDATE 2"}
	cronHandler := NewCronHandler(nil)
	cronHandler.pool = database
	mux := http.NewServeMux()
	cronHandler.RegisterDeletionAdapter(mux)
	authenticated, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	body := `{"org_id":"org-a","space_ref":"space-a","owner_principal_id":"user-a","deletion_request_id":"delete-1"}`

	for _, test := range []struct {
		name      string
		token     string
		body      string
		want      int
		wantCalls int
	}{
		{name: "ordinary writer", token: sign("service", []string{authz.WriteScope}, boolPtr(false)), body: body, want: http.StatusForbidden},
		{name: "cross org", token: sign("service", []string{authz.SpaceDeletionScope}, boolPtr(false)), body: strings.Replace(body, `"org-a"`, `"org-b"`, 1), want: http.StatusForbidden},
		{name: "exact coordinator", token: sign("service", []string{authz.SpaceDeletionScope}, boolPtr(false)), body: body, want: http.StatusOK, wantCalls: 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			before := database.databaseCalls()
			request := httptest.NewRequest(http.MethodPost, "/api/v1/internal/space-deletion/cron", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+test.token)
			response := httptest.NewRecorder()
			authenticated.ServeHTTP(response, request)
			if response.Code != test.want {
				t.Fatalf("status=%d body=%s want=%d", response.Code, response.Body.String(), test.want)
			}
			if got := database.databaseCalls() - before; got != test.wantCalls {
				t.Fatalf("database calls=%d want=%d", got, test.wantCalls)
			}
			if test.want == http.StatusOK && !strings.Contains(response.Body.String(), `"request_id":"delete-1"`) {
				t.Fatalf("request-bound receipt missing: %s", response.Body.String())
			}
			if test.want == http.StatusOK && !strings.Contains(response.Body.String(), `space_cron_schedules_and_pending_fires_canceled`) {
				t.Fatalf("pending cron-fire cancellation receipt missing: %s", response.Body.String())
			}
		})
	}
}

func boolPtr(value bool) *bool { return &value }
