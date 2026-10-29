package http

import (
	"context"
	"encoding/json"
	stdhttp "net/http"
	"net/http/httptest"
	"testing"
)

const controlOwnerEffectReservationTestToken = "control-owner-effect-reservation-test-token-32"

func TestControlOwnerEffectReservationCoordinatorBindsBearerToControlOnly(t *testing.T) {
	commitment := testOwnerEffectReservationCommitment()
	seen := 0
	server := httptest.NewServer(stdhttp.HandlerFunc(func(writer stdhttp.ResponseWriter, request *stdhttp.Request) {
		seen++
		if request.Method != stdhttp.MethodPost {
			t.Fatalf("method = %s, want POST", request.Method)
		}
		if request.URL.Path != controlOwnerEffectReservationsPath && request.URL.Path != controlOwnerEffectReservationsPath+"/owner_effect_reservation_1/commit" {
			t.Fatalf("path = %s", request.URL.Path)
		}
		if request.Header.Get("X-Service-Id") != "conversation-core" || request.Header.Get("X-Service-Token") != controlOwnerEffectReservationTestToken {
			t.Fatalf("service authentication headers are wrong")
		}
		var body struct {
			ControlDecisionToken string                           `json:"control_decision_token"`
			Commitment           ownerEffectReservationCommitment `json:"commitment"`
		}
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatalf("decode = %v", err)
		}
		if body.ControlDecisionToken != "decision-bearer" || body.Commitment != commitment {
			t.Fatalf("body = %#v", body)
		}
		status := "reserved"
		if seen == 2 {
			status = "committed"
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"data":{"reservation_id":"owner_effect_reservation_1","operation_id":"ticketop_test","status":"` + status + `"}}`))
	}))
	defer server.Close()

	coordinator, err := NewControlOwnerEffectReservationCoordinator(server.URL, controlOwnerEffectReservationTestToken, true)
	if err != nil {
		t.Fatalf("NewControlOwnerEffectReservationCoordinator() error = %v", err)
	}
	reserved, err := coordinator.Reserve(context.Background(), "decision-bearer", commitment)
	if err != nil || reserved.Status != "reserved" {
		t.Fatalf("Reserve() = %#v, %v", reserved, err)
	}
	committed, err := coordinator.Commit(context.Background(), reserved.ReservationID, "decision-bearer", commitment)
	if err != nil || committed.Status != "committed" || seen != 2 {
		t.Fatalf("Commit() = %#v, %v; requests=%d", committed, err, seen)
	}
}

func TestControlOwnerEffectReservationCoordinatorUsesTLSAndDedicatedServiceIdentity(t *testing.T) {
	commitment := testOwnerEffectReservationCommitment()
	server := httptest.NewTLSServer(stdhttp.HandlerFunc(func(writer stdhttp.ResponseWriter, request *stdhttp.Request) {
		if request.Method != stdhttp.MethodPost {
			t.Fatalf("method = %s, want POST", request.Method)
		}
		if request.URL.Path != controlOwnerEffectReservationsPath {
			t.Fatalf("path = %s, want %s", request.URL.Path, controlOwnerEffectReservationsPath)
		}
		if request.Header.Get("X-Service-Id") != "conversation-core" {
			t.Fatalf("X-Service-Id = %q, want conversation-core", request.Header.Get("X-Service-Id"))
		}
		if request.Header.Get("X-Service-Token") != controlOwnerEffectReservationTestToken {
			t.Fatalf("X-Service-Token = %q, want dedicated Control token", request.Header.Get("X-Service-Token"))
		}
		var body struct {
			ControlDecisionToken string                           `json:"control_decision_token"`
			Commitment           ownerEffectReservationCommitment `json:"commitment"`
		}
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatalf("decode = %v", err)
		}
		if body.ControlDecisionToken != "decision-bearer" || body.Commitment != commitment {
			t.Fatalf("body = %#v", body)
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"data":{"reservation_id":"owner_effect_reservation_tls","operation_id":"ticketop_test","status":"reserved"}}`))
	}))
	defer server.Close()

	coordinator, err := NewControlOwnerEffectReservationCoordinator(server.URL, controlOwnerEffectReservationTestToken, false)
	if err != nil {
		t.Fatalf("NewControlOwnerEffectReservationCoordinator() error = %v", err)
	}
	// httptest.NewTLSServer's client trusts only this ephemeral server certificate;
	// this exercises the same HTTPS transport path without weakening production
	// certificate verification or enabling plaintext.
	coordinator.client = server.Client()
	receipt, err := coordinator.Reserve(context.Background(), "decision-bearer", commitment)
	if err != nil {
		t.Fatalf("Reserve() error = %v", err)
	}
	if receipt.ReservationID != "owner_effect_reservation_tls" || receipt.Status != "reserved" {
		t.Fatalf("Reserve() = %#v", receipt)
	}
}

func TestControlOwnerEffectReservationCoordinatorMapsDenialAndRejectsNonLoopbackPlaintext(t *testing.T) {
	server := httptest.NewServer(stdhttp.HandlerFunc(func(writer stdhttp.ResponseWriter, _ *stdhttp.Request) { writer.WriteHeader(stdhttp.StatusForbidden) }))
	defer server.Close()
	coordinator, err := NewControlOwnerEffectReservationCoordinator(server.URL, controlOwnerEffectReservationTestToken, true)
	if err != nil {
		t.Fatalf("NewControlOwnerEffectReservationCoordinator() error = %v", err)
	}
	if _, err := coordinator.Reserve(context.Background(), "decision-bearer", testOwnerEffectReservationCommitment()); err != ErrOwnerEffectReservationDenied {
		t.Fatalf("Reserve() error = %v, want denial", err)
	}
	if _, err := NewControlOwnerEffectReservationCoordinator("http://user-core:8080", controlOwnerEffectReservationTestToken, true); err == nil {
		t.Fatal("NewControlOwnerEffectReservationCoordinator() error = nil, want non-loopback plaintext rejection")
	}
}

func testOwnerEffectReservationCommitment() ownerEffectReservationCommitment {
	return ownerEffectReservationCommitment{
		OperationID: "ticketop_test", ActionID: "tickets.create",
		ActionSchemaHash: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
		PayloadDigest:    "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
		IdempotencyKey:   "ticket-agent-1", DecisionRef: "decision_1", GrantRef: "grant_0123456789abcdef",
	}
}
