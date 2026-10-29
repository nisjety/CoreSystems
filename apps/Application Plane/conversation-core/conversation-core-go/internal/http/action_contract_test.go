package http

import (
	"bytes"
	"encoding/json"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
)

func TestActionContractsAreOwnerIssuedAndActorScoped(t *testing.T) {
	router := newRouter(NewHandler(nil, nil), testVerifier(t))
	request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/action-contracts", nil)
	signConversationRequest(t, request, nil, "verevon-gateway", testGatewaySecret, "user_1", "org_1", "member")

	response := performRequest(router, request)
	body := response.Body.String()
	for _, want := range []string{
		`"action_id":"tickets.create"`,
		`"owner_plane":"application"`,
		`"eligible_actor_types":["human"]`,
		`"required_service_identity":"verevon-gateway"`,
		`"idempotency":"caller_supplied"`,
		`"receipt_contract":"durable_owner_receipt"`,
		`"schema_sha256":"sha256:`,
		`"conversation_id"`,
	} {
		if !strings.Contains(body, want) {
			t.Fatalf("owner-issued action contract missing %s: %s", want, body)
		}
	}
	if response.Code != stdhttp.StatusOK {
		t.Fatalf("status = %d, body = %s", response.Code, body)
	}
}

func TestActionContractSchemaDigestIsCanonical(t *testing.T) {
	left := canonicalSchemaSHA256(json.RawMessage(`{"type":"object","properties":{"b":{"type":"string"},"a":{"type":"number"}}}`))
	right := canonicalSchemaSHA256(json.RawMessage(`
        { "properties": { "a": { "type": "number" }, "b": { "type": "string" } }, "type": "object" }
    `))
	if left == "" || left != right {
		t.Fatalf("canonical digest mismatch: %q vs %q", left, right)
	}
}

func TestTicketCreateContractDescribesButDoesNotEnableTheModelPath(t *testing.T) {
	contract := ticketCreateActionContract()
	if len(contract.ActorRequirements) != 2 {
		t.Fatalf("actor requirements = %#v", contract.ActorRequirements)
	}
	model := contract.ActorRequirements[1]
	if model.ActorType != "model" || model.Availability != "not_enabled" ||
		model.RequiredServiceIdentity != "execution-core" ||
		model.RequiredDelegation != "control_target_action_decision" ||
		model.HTTPMethod != stdhttp.MethodPost || model.Path != "/internal/v1/agent-ticket-operations" {
		t.Fatalf("unexpected disabled Model requirement: %#v", model)
	}
	for _, actorType := range contract.EligibleActorTypes {
		if actorType == "model" {
			t.Fatalf("disabled Model path must not become eligible: %#v", contract)
		}
	}
}

func TestPublicTicketRouteRejectsATrustedExecutionWorkload(t *testing.T) {
	// Future private Model ingress needs execution-core to be a recognized
	// transport principal. Recognition alone must never widen the existing
	// human BFF route, even if the workload signs a complete user/org header
	// set. The target-action decision route will be a separate endpoint.
	const executionSecret = "execution-test-secret-at-least-32-bytes"
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "conversation-core",
		Keys: map[string]string{
			"verevon-gateway": testGatewaySecret,
			"execution-core":  executionSecret,
		},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	body := []byte(`{"conversation_id":"conversation-1","idempotency_key":"ticket-agent-1"}`)
	request := httptest.NewRequest(stdhttp.MethodPost, "/api/v1/tickets", bytes.NewReader(body))
	signConversationRequest(t, request, body, "execution-core", executionSecret, "user_1", "org_1", "member")

	response := performRequest(newRouter(NewHandler(nil, nil), verifier), request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, want public human route denial; body=%s", response.Code, response.Body.String())
	}
}

func TestActionContractsRejectUntrustedCallers(t *testing.T) {
	router := newRouter(NewHandler(nil, nil), testVerifier(t))
	request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/action-contracts", nil)
	signConversationRequest(t, request, nil, "conversation-ingest", testIngestSecret, "user_1", "org_1", "member")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, body = %s", response.Code, response.Body.String())
	}
}
