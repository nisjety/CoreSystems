package api

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestProviderWriteRequiresValidAttestationAndScopedPresenterBeforeProvider(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.WriteHeader(http.StatusAccepted)
	}))
	defer providerServer.Close()

	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-write-attested"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	_, err = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-write-attested", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{"mail.send"},
		EncryptedAccessToken: encryptedToken, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: oauthService,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	idempotencyKey := "conversation:org-1:human-intent-1"
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-write-attested",
		ProviderKey: "microsoft", Operation: "mail.send", Params: map[string]any{}, Body: body, IdempotencyKey: idempotencyKey,
	}
	writeAttestation := signAPIWriteAttestation(t, privateKey, apiHumanIntentClaims(t, binding))

	call := func(payload map[string]any) *http.Response {
		t.Helper()
		raw, err := json.Marshal(payload)
		if err != nil {
			t.Fatalf("Marshal request error: %v", err)
		}
		req := httptest.NewRequest("POST", "/api/v1/connections/conn-write-attested/actions", strings.NewReader(string(raw)))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		response, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		return response
	}

	for _, test := range []struct {
		name    string
		payload map[string]any
		code    string
	}{
		{name: "missing attestation", payload: map[string]any{"operation": "mail.send", "approvalId": "made-up", "idempotencyKey": idempotencyKey, "body": body}, code: "write_attestation_required"},
		{name: "malformed attestation", payload: map[string]any{"operation": "mail.send", "writeAttestation": "not.a.jws", "idempotencyKey": idempotencyKey, "body": body}, code: "write_attestation_invalid"},
		{name: "whitespace wrapped attestation", payload: map[string]any{"operation": "mail.send", "writeAttestation": " " + writeAttestation, "idempotencyKey": idempotencyKey, "params": map[string]any{}, "body": body}, code: "write_attestation_invalid"},
	} {
		t.Run(test.name, func(t *testing.T) {
			response := call(test.payload)
			if response.StatusCode != fiber.StatusForbidden {
				defer response.Body.Close()
				t.Fatalf("status = %d, want 403", response.StatusCode)
			}
			if code := readAPIErrorCode(t, response); code != test.code {
				t.Fatalf("error code = %q, want %q", code, test.code)
			}
		})
	}
	if providerCalls != 0 {
		t.Fatalf("provider calls before valid attestation = %d, want 0", providerCalls)
	}

	response := call(map[string]any{
		"operation": "mail.send", "writeAttestation": writeAttestation, "idempotencyKey": idempotencyKey, "params": map[string]any{}, "body": body,
	})
	if response.StatusCode != fiber.StatusOK {
		defer response.Body.Close()
		t.Fatalf("valid attested write status = %d, want 200", response.StatusCode)
	}
	_ = response.Body.Close()
	if providerCalls != 1 {
		t.Fatalf("provider calls = %d, want 1", providerCalls)
	}
}

func TestLegacyProviderWriteRouteFailsClosedBeforeProvider(t *testing.T) {
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusAccepted)
	}))
	defer providerServer.Close()

	cfg, repo, oauthService := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-legacy-write", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", Status: "active", Capabilities: []string{"mail.send"},
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Actions: actions.NewService(cfg, providerServer.Client())})
	req := httptest.NewRequest("POST", "/integrations/ms-graph/mail/send?connectionId=conn-legacy-write", strings.NewReader(`{"message":{"subject":"hello"},"approvalRef":"made-up"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if response.StatusCode != fiber.StatusForbidden {
		defer response.Body.Close()
		t.Fatalf("status = %d, want 403", response.StatusCode)
	}
	if providerCalled {
		t.Fatal("legacy write route reached provider without bearer and attestation")
	}
}

func TestProviderWriteTokenLookupFailureKeepsExactAttestedRetrySafe(t *testing.T) {
	providerCalls := 0
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalls++
		w.WriteHeader(http.StatusAccepted)
	}))
	defer providerServer.Close()
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	rotatedPublicKey, rotatedPrivateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey rotated error: %v", err)
	}
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.AllowLegacyTenantKey = false
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	trustedKeys, err := json.Marshal([]map[string]string{
		{"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(publicKey)},
		{"issuer": "conversation-core", "kid": "conversation-write-2026-08", "public_key": base64.StdEncoding.EncodeToString(rotatedPublicKey)},
	})
	if err != nil {
		t.Fatalf("Marshal trusted keys error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = string(trustedKeys)
	connection := store.Connection{
		ID: "conn-token-retry", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{"mail.send"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: oauthService,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})
	body := map[string]any{"message": map[string]any{"subject": "retry after token broker recovery"}}
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: connection.ID,
		ProviderKey: "microsoft", Operation: "mail.send", Params: map[string]any{}, Body: body,
		IdempotencyKey: "conversation:org-1:token-retry-1",
	}
	claims := apiHumanIntentClaims(t, binding)
	claims.AuthorizationID = "human-intent-token-retry-1"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-token-retry-1"
	payload := map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": binding.IdempotencyKey, "params": map[string]any{}, "body": body,
	}
	call := func() *http.Response {
		t.Helper()
		raw, _ := json.Marshal(payload)
		req := httptest.NewRequest("POST", "/api/v1/connections/"+connection.ID+"/actions", strings.NewReader(string(raw)))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Authorization", "Bearer conversation-service-token")
		response, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		return response
	}

	first := call()
	if first.StatusCode != fiber.StatusServiceUnavailable {
		defer first.Body.Close()
		t.Fatalf("token lookup failure status = %d, want 503", first.StatusCode)
	}
	if code := readAPIErrorCode(t, first); code != "action_pre_provider_retryable" {
		t.Fatalf("token lookup failure code = %q, want action_pre_provider_retryable", code)
	}
	if providerCalls != 0 {
		t.Fatalf("provider calls before token recovery = %d, want 0", providerCalls)
	}
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	connection.EncryptedAccessToken, err = vault.Encrypt("access-token", []byte(connection.ID))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatalf("UpsertConnection recovered token error: %v", err)
	}
	freshClaims := claims
	freshClaims.JWTID = "attestation-token-retry-2"
	payload["writeAttestation"] = signAPIWriteAttestationWithKID(t, rotatedPrivateKey, "conversation-write-2026-08", freshClaims)
	retry := call()
	if retry.StatusCode != fiber.StatusOK {
		defer retry.Body.Close()
		t.Fatalf("exact retry status = %d, want 200", retry.StatusCode)
	}
	_ = retry.Body.Close()
	if providerCalls != 1 {
		t.Fatalf("provider calls after exact retry = %d, want 1", providerCalls)
	}
}

func TestUnknownOperationFailsClosedBeforeOAuthOrProvider(t *testing.T) {
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusOK)
	}))
	defer providerServer.Close()
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	_, _ = repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-unknown-action", ProviderKey: "microsoft", ConnectorType: "microsoft-graph", OrganizationID: "org-1", Status: "active",
	})
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Actions: actions.NewService(cfg, providerServer.Client())})
	req := httptest.NewRequest("POST", "/api/v1/connections/conn-unknown-action/actions", strings.NewReader(`{"operation":"mail.delete-all"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if response.StatusCode != fiber.StatusBadRequest {
		defer response.Body.Close()
		t.Fatalf("status = %d, want 400", response.StatusCode)
	}
	if code := readAPIErrorCode(t, response); code != "operation_not_supported" {
		t.Fatalf("error code = %q, want operation_not_supported", code)
	}
	if providerCalled {
		t.Fatal("unknown operation reached provider")
	}
}

func apiTrustedKeysJSON(t *testing.T, publicKey ed25519.PublicKey) string {
	t.Helper()
	raw, err := json.Marshal([]map[string]string{{
		"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(publicKey),
	}})
	if err != nil {
		t.Fatalf("Marshal keys error: %v", err)
	}
	return string(raw)
}

func apiHumanIntentClaims(t *testing.T, binding attestation.Binding) attestation.Claims {
	t.Helper()
	digest, err := attestation.PayloadSHA256(binding)
	if err != nil {
		t.Fatalf("PayloadSHA256 error: %v", err)
	}
	issuedAt := time.Now().UTC().Add(-time.Second)
	return attestation.Claims{
		Version: 1, Issuer: "conversation-core", Audience: "integration-corev2", PresenterService: binding.PresenterService,
		AuthorizationKind: attestation.AuthorizationHumanIntent, AuthorizationID: "human-intent-1", ActionID: "human-intent-1",
		OrganizationID: binding.OrganizationID, ConnectionID: binding.ConnectionID, ProviderKey: binding.ProviderKey,
		Operation: binding.Operation, ActorID: "user-1", PayloadSHA256: digest, IdempotencyKey: binding.IdempotencyKey,
		JWTID: "attestation-1", IssuedAt: issuedAt.Unix(), NotBefore: issuedAt.Unix(), ExpiresAt: issuedAt.Add(30 * time.Second).Unix(),
	}
}

func signAPIWriteAttestation(t *testing.T, privateKey ed25519.PrivateKey, claims attestation.Claims) string {
	return signAPIWriteAttestationWithKID(t, privateKey, "conversation-write-2026-07", claims)
}

func signAPIWriteAttestationWithKID(t *testing.T, privateKey ed25519.PrivateKey, keyID string, claims attestation.Claims) string {
	t.Helper()
	headerJSON, err := json.Marshal(attestation.Header{Algorithm: "EdDSA", Type: attestation.AttestationType, KeyID: keyID})
	if err != nil {
		t.Fatalf("Marshal header error: %v", err)
	}
	claimsJSON, err := json.Marshal(claims)
	if err != nil {
		t.Fatalf("Marshal claims error: %v", err)
	}
	header := base64.RawURLEncoding.EncodeToString(headerJSON)
	payload := base64.RawURLEncoding.EncodeToString(claimsJSON)
	signingInput := header + "." + payload
	return signingInput + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(privateKey, []byte(signingInput)))
}
