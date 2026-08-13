package http

import (
	"encoding/json"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"
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

func TestActionContractsRejectUntrustedCallers(t *testing.T) {
	router := newRouter(NewHandler(nil, nil), testVerifier(t))
	request := httptest.NewRequest(stdhttp.MethodGet, "/api/v1/action-contracts", nil)
	signConversationRequest(t, request, nil, "conversation-ingest", testIngestSecret, "user_1", "org_1", "member")

	response := performRequest(router, request)
	if response.Code != stdhttp.StatusForbidden {
		t.Fatalf("status = %d, body = %s", response.Code, response.Body.String())
	}
}
