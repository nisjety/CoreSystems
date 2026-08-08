package http

import (
	"context"
	"strings"
	"testing"
	"time"

	stdhttp "net/http"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type recordingCSATOutcomeRepository struct {
	conversation.Repository
	ticket  *conversation.Ticket
	pref    *conversation.CSATPreference
	outcome *conversation.TicketCSATOutcome
}

func (r *recordingCSATOutcomeRepository) GetTicket(_ context.Context, orgID, ticketID string) (*conversation.Ticket, error) {
	if r.ticket == nil || r.ticket.OrgID != orgID || r.ticket.ID != ticketID {
		return nil, conversation.ErrNotFound
	}
	copy := *r.ticket
	return &copy, nil
}

func (r *recordingCSATOutcomeRepository) GetConversationCSATPreference(_ context.Context, orgID, conversationID string) (*conversation.CSATPreference, error) {
	if r.pref == nil || r.pref.OrgID != orgID || r.pref.ConversationID != conversationID {
		return nil, conversation.ErrNotFound
	}
	copy := *r.pref
	return &copy, nil
}

func (r *recordingCSATOutcomeRepository) UpsertTicketCSATOutcome(_ context.Context, input conversation.TicketCSATOutcomeInput) (*conversation.TicketCSATOutcome, error) {
	now := time.Now().UTC()
	r.outcome = &conversation.TicketCSATOutcome{OrgID: input.OrgID, TicketID: input.TicketID, ConversationID: r.ticket.ConversationID, Score: input.Score, RecordedBy: input.RecordedBy, RecordedAt: &now}
	copy := *r.outcome
	return &copy, nil
}

func TestTicketCSATOutcomeRouteRecordsOnlyAnExplicitScore(t *testing.T) {
	repository := &recordingCSATOutcomeRepository{
		ticket: &conversation.Ticket{ID: "ticket_1", OrgID: "org-1", ConversationID: "conversation_1", Status: "resolved"},
		pref:   &conversation.CSATPreference{OrgID: "org-1", ConversationID: "conversation_1", ContactID: "contact_1", OptedIn: true},
	}
	router := newRouter(NewHandler(nil, conversation.NewService(repository, nil)), testVerifier(t))
	body := []byte(`{"score":5}`)
	response := performRequest(router, signedAgentRequest(t, stdhttp.MethodPut, "/api/v1/tickets/ticket_1/csat-outcome", body))

	if response.Code != stdhttp.StatusOK {
		t.Fatalf("status = %d, want 200; body=%s", response.Code, response.Body.String())
	}
	if repository.outcome == nil || repository.outcome.Score != 5 || repository.outcome.RecordedBy != "user-1" {
		t.Fatalf("recorded outcome = %#v, want score and verified actor", repository.outcome)
	}
	if strings.Contains(response.Body.String(), "survey") || strings.Contains(response.Body.String(), "sent") {
		t.Fatalf("response claimed an unimplemented survey delivery: %s", response.Body.String())
	}
}

func TestTicketCSATOutcomeRouteRejectsMissingScore(t *testing.T) {
	router := newRouter(NewHandler(nil, conversation.NewService(&recordingCSATOutcomeRepository{}, nil)), testVerifier(t))
	body := []byte(`{}`)
	response := performRequest(router, signedAgentRequest(t, stdhttp.MethodPut, "/api/v1/tickets/ticket_1/csat-outcome", body))
	if response.Code != stdhttp.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
}
