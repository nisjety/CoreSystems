package org

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

// D22l: auth-core answers 409 manual_adoption_required when SCIM asks to bind an
// existing owner or admin. The client maps that to ErrSCIMManualAdoption, not to
// the generic unavailable error, so the IdP gets a 409 for manual handling
// instead of a retry loop.
func TestAuthCoreClientMapsTheAdoptionRefusal(t *testing.T) {
	const token = "identity-token-for-the-scim-client-0123456789"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Any other route or credential answers 404 or 401, which the client maps to
		// ErrSSOUnavailable, so the assertion below fails rather than passing by accident.
		if r.Method != http.MethodPost || r.URL.Path != "/api/v1/internal/scim/memberships/provision" ||
			r.Header.Get(authCoreIdentityHeader) != token {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		w.Header().Set("content-type", "application/json")
		w.WriteHeader(http.StatusConflict)
		_, _ = w.Write([]byte(`{"code":"manual_adoption_required","message":"handled by hand"}`))
	}))
	defer server.Close()

	client := NewAuthCoreIdentityClient(server.URL, token)
	if client == nil {
		t.Fatal("the client needs a base URL and a credential")
	}
	_, err := client.ProvisionMember(context.Background(), SCIMProvisionRequest{
		OperationID:    "6f0c5f2e-7a4b-4c1d-9e3f-2a1b0c9d8e7f",
		OrganizationID: "org-1",
		ActorUserID:    "admin-1",
		Email:          "ada@corp.example.com",
		Role:           "member",
	})
	if !errors.Is(err, ErrSCIMManualAdoption) {
		t.Fatalf("ProvisionMember = %v, want ErrSCIMManualAdoption", err)
	}
	if errors.Is(err, ErrSCIMLastOwner) || errors.Is(err, ErrSCIMMembershipConflict) || errors.Is(err, ErrSSOUnavailable) {
		t.Fatalf("the adoption refusal also matches another error: %v", err)
	}
}
