package http

import (
	"context"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type ticketOperationLookupRepository struct {
	conversation.Repository
	gotOrg, gotActor, gotKey string
}

func (r *ticketOperationLookupRepository) GetTicketOperation(_ context.Context, orgID, actorUserID, idempotencyKey string) (*conversation.TicketOperationReceipt, error) {
	r.gotOrg, r.gotActor, r.gotKey = orgID, actorUserID, idempotencyKey
	return &conversation.TicketOperationReceipt{
		OperationID: "ticketop_1", AuditEventID: "audit_1", Status: "completed",
		Ticket: &conversation.Ticket{ID: "ticket_1"}, Replayed: true,
	}, nil
}

func TestTicketOperationLookupUsesTheAuthenticatedActorAndNeverRetriesTheEffect(t *testing.T) {
	repository := &ticketOperationLookupRepository{}
	router := newRouter(NewHandler(nil, conversation.NewService(repository, nil)), testVerifier(t))
	request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/ticket-operations/ticket-create-001", nil)
	signConversationRequest(t, request, nil, "verevon-gateway", testGatewaySecret, "user_1", "org_1", "member")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusOK || !strings.Contains(response.Body.String(), `"operation_id":"ticketop_1"`) {
		t.Fatalf("status/body = %d/%s", response.Code, response.Body.String())
	}
	if repository.gotOrg != "org_1" || repository.gotActor != "user_1" || repository.gotKey != "ticket-create-001" {
		t.Fatalf("lookup scope = (%q, %q, %q)", repository.gotOrg, repository.gotActor, repository.gotKey)
	}
}
