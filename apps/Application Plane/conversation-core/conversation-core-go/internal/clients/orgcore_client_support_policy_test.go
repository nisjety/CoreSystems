package clients

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

func TestSupportPolicyUsesExactOrgAndRoleCapabilityEndpoints(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("X-Service-Id") != "conversation-core" || request.Header.Get("X-Service-Token") != "service-token" {
			t.Fatalf("missing machine credentials")
		}
		switch request.URL.Path {
		case "/internal/orgs/org-1":
			writer.Header().Set("Content-Type", "application/json")
			_, _ = writer.Write([]byte(`{"id":"org-1","metadata":{"interactiveRetention":{"zdr":false},"supportAi":{"mode":"review"}}}`))
		case "/internal/orgs/org-1/roles/member/capabilities":
			writer.Header().Set("Content-Type", "application/json")
			_, _ = writer.Write([]byte(`{"capabilities":["support:recurrence:read"]}`))
		default:
			http.NotFound(writer, request)
		}
	}))
	defer server.Close()

	client := &OrgCoreClient{
		baseURL:          server.URL,
		servicePrincipal: "conversation-core",
		serviceToken:     "service-token",
		httpClient:       server.Client(),
	}
	policy, err := client.SupportPolicy(t.Context(), "org-1", "member")
	if err != nil {
		t.Fatalf("SupportPolicy() error = %v", err)
	}
	if policy.ZDREnabled || !policy.AIReviewEnabled || !policy.RecurrenceAllowed {
		t.Fatalf("policy = %+v, want review-enabled non-ZDR recurrence access", policy)
	}
}

func TestAllowAIProposalFailsClosedForZDR(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path != "/internal/orgs/org-1" {
			http.NotFound(writer, request)
			return
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"id":"org-1","metadata":{"interactiveRetention":{"zdr":true}}}`))
	}))
	defer server.Close()

	client := &OrgCoreClient{baseURL: server.URL, httpClient: server.Client()}
	if err := client.AllowAIProposal(t.Context(), "org-1"); !errors.Is(err, conversation.ErrZDRAIProposalForbidden) {
		t.Fatalf("AllowAIProposal() error = %v, want ZDR denial", err)
	}
}
