package http

import (
	"context"
	"fmt"
	stdhttp "net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type outcomeRepository struct {
	conversation.Repository
	org, actor string
	calls      int
	fail       bool
}

func (r *outcomeRepository) ReadOutcomeReport(_ context.Context, org, actor string, from, to *time.Time) (*conversation.OutcomeReport, error) {
	r.org, r.actor = org, actor
	r.calls++
	if r.fail {
		return nil, fmt.Errorf("source unavailable")
	}
	return &conversation.OutcomeReport{OrgID: org, ActorUserID: actor, From: from, To: to, Source: "conversation-core", CheckedAt: time.Now().UTC()}, nil
}

func TestOutcomeReadUsesSignedScopeAndNeverFabricatesSourceZeros(t *testing.T) {
	for _, tc := range []struct {
		name, query, service, secret, org, actor string
		status                                   int
		fail                                     bool
	}{
		{"organization", "?org_id=other-org&user_id=other-user", "verevon-gateway", testGatewaySecret, "org-1", "", 200, false},
		{"personal", "?scope=me&org_id=other-org&user_id=other-user", "verevon-gateway", testGatewaySecret, "org-1", "user-1", 200, false},
		{"bad scope", "?scope=global", "verevon-gateway", testGatewaySecret, "org-1", "", 400, false},
		{"bad time", "?from=yesterday", "verevon-gateway", testGatewaySecret, "org-1", "", 400, false},
		{"reversed time", "?from=2026-10-05T00:00:00Z&to=2026-10-04T00:00:00Z", "verevon-gateway", testGatewaySecret, "org-1", "", 422, false},
		{"no org", "", "verevon-gateway", testGatewaySecret, "", "", 400, false},
		{"wrong service", "", "model-gateway", testModelSecret, "org-1", "", 403, false},
		{"unsigned", "", "", "", "org-1", "", 401, false},
		{"source failure", "", "verevon-gateway", testGatewaySecret, "org-1", "", 500, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			repo := &outcomeRepository{fail: tc.fail}
			router := newRouter(NewHandler(nil, conversation.NewService(repo, nil)), testVerifier(t))
			request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/insights/outcomes"+tc.query, nil)
			if tc.service != "" {
				signConversationRequest(t, request, nil, tc.service, tc.secret, "user-1", tc.org, "member")
			}
			response := performRequest(router, request)
			if response.Code != tc.status {
				t.Fatalf("status=%d want=%d body=%s", response.Code, tc.status, response.Body.String())
			}
			if tc.status == 200 && (repo.org != tc.org || repo.actor != tc.actor) {
				t.Fatalf("scope=%q/%q want=%q/%q", repo.org, repo.actor, tc.org, tc.actor)
			}
			if tc.status != 200 && !tc.fail && repo.calls != 0 {
				t.Fatal("invalid request reached the source")
			}
		})
	}
}
