package activities

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// F-f10-27: the scheduled-step decision call names no stored-consent contract. Control
// binds the contract only from its prepared Session Core step, so the body carries the
// step's identifiers alone. The intent keeps the contract, which verify needs.
func TestScheduledStepDecisionCallCarriesNoContractField(t *testing.T) {
	var sent map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, err := io.ReadAll(r.Body)
		if err != nil {
			t.Errorf("read decision body: %v", err)
		}
		if err := json.Unmarshal(raw, &sent); err != nil {
			t.Errorf("decode decision body: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"error":"scheduled step is not authorized"}`))
	}))
	t.Cleanup(server.Close)

	intent := validScheduledStepExecutionIntent()
	intent.ContractDigest = "sha256:" + strings.Repeat("c", 64)
	intent.ContractExpiresAt = "2026-08-16T05:00:00Z"
	authorizer, err := NewControlScheduledStepAuthorizer(server.URL, "step-token", "key-1", publicKeyBase64(t), server.Client())
	if err != nil {
		t.Fatal(err)
	}
	if _, err := authorizer.AuthorizeScheduledStep(context.Background(), intent); err == nil {
		t.Fatal("a refused decision call was not refused")
	}
	bodyIntent, ok := sent["intent"].(map[string]any)
	if !ok {
		t.Fatalf("the decision call carried no intent: %#v", sent)
	}
	for _, field := range []string{"contract_digest", "contract_expires_at"} {
		if _, present := bodyIntent[field]; present {
			t.Fatalf("the decision call named %s; Control must bind the prepared contract only", field)
		}
	}
	if bodyIntent["step_id"] != intent.StepID {
		t.Fatalf("the decision call step_id = %v, want %q", bodyIntent["step_id"], intent.StepID)
	}
}
