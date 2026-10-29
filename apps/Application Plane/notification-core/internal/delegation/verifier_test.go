package delegation

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"net/http"
	"testing"
	"time"
)

func TestVerifierAcceptsBoundDelegationAndRejectsReplay(t *testing.T) {
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	verifier, err := NewVerifier(Config{
		Audience: "notification-core",
		Keys: map[string]string{
			"verevon-gateway": "gateway-test-secret-at-least-32-bytes",
		},
		Now: func() time.Time { return now },
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}

	body := []byte(`{"organization_id":"org-1","recipient":{"kind":"user","id":"user-1"}}`)
	request := signedRequest(t, http.MethodPost, "http://notification-core:3140/api/v1/notification-requests", body, signedFields{
		serviceID:      "verevon-gateway",
		secret:         "gateway-test-secret-at-least-32-bytes",
		timestamp:      now,
		nonce:          "nonce-1234567890",
		userID:         "user-1",
		organizationID: "org-1",
		role:           "member",
	})

	principal, err := verifier.Verify(request, body)
	if err != nil {
		t.Fatalf("Verify() error = %v", err)
	}
	if principal.ServiceID != "verevon-gateway" || principal.UserID != "user-1" || principal.OrganizationID != "org-1" {
		t.Fatalf("principal = %#v", principal)
	}
	if _, err := verifier.Verify(request, body); !IsReplay(err) {
		t.Fatalf("replayed Verify() error = %v, want replay error", err)
	}
}

func TestVerifierRejectsBodyOrgAndMethodTampering(t *testing.T) {
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	verifier, err := NewVerifier(Config{
		Audience: "notification-core",
		Keys:     map[string]string{"verevon-gateway": "gateway-test-secret-at-least-32-bytes"},
		Now:      func() time.Time { return now },
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}

	body := []byte(`{"type":"support.requested"}`)
	base := signedFields{
		serviceID:      "verevon-gateway",
		secret:         "gateway-test-secret-at-least-32-bytes",
		timestamp:      now,
		nonce:          "nonce-original-123",
		userID:         "user-1",
		organizationID: "org-1",
		role:           "member",
	}

	tests := []struct {
		name   string
		mutate func(*http.Request, *[]byte)
	}{
		{name: "body", mutate: func(_ *http.Request, body *[]byte) { *body = []byte(`{"type":"forged"}`) }},
		{name: "organization", mutate: func(req *http.Request, _ *[]byte) { req.Header.Set(HeaderOrganizationID, "org-2") }},
		{name: "method", mutate: func(req *http.Request, _ *[]byte) { req.Method = http.MethodDelete }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := signedRequest(t, http.MethodPost, "http://notification-core:3140/api/v1/notification-requests", body, base)
			verifiedBody := bytes.Clone(body)
			test.mutate(request, &verifiedBody)
			if _, err := verifier.Verify(request, verifiedBody); err == nil {
				t.Fatal("Verify() error = nil, want tampering rejection")
			}
		})
	}
}

func TestVerifierRejectsUnknownPrincipalAndExpiredTimestamp(t *testing.T) {
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	verifier, err := NewVerifier(Config{
		Audience: "notification-core",
		Keys:     map[string]string{"verevon-gateway": "gateway-test-secret-at-least-32-bytes"},
		Now:      func() time.Time { return now },
		MaxSkew:  2 * time.Minute,
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}

	body := []byte(`{}`)
	unknown := signedRequest(t, http.MethodGet, "http://notification-core:3140/notifications", body, signedFields{
		serviceID: "unknown-service",
		secret:    "unknown-test-secret-at-least-32-bytes",
		timestamp: now,
		nonce:     "nonce-unknown-123",
	})
	if _, err := verifier.Verify(unknown, body); err == nil {
		t.Fatal("unknown principal Verify() error = nil")
	}

	expired := signedRequest(t, http.MethodGet, "http://notification-core:3140/notifications", body, signedFields{
		serviceID: "verevon-gateway",
		secret:    "gateway-test-secret-at-least-32-bytes",
		timestamp: now.Add(-3 * time.Minute),
		nonce:     "nonce-expired-123",
	})
	if _, err := verifier.Verify(expired, body); err == nil {
		t.Fatal("expired Verify() error = nil")
	}
}

func TestVerifierAcceptsVerevonGatewayCrossLanguageFixture(t *testing.T) {
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	verifier, err := NewVerifier(Config{
		Audience: "notification-core",
		Keys:     map[string]string{"verevon-gateway": "0123456789abcdef0123456789abcdef"},
		Now:      func() time.Time { return now },
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	body := []byte(`{"organization_id":"org-1","recipient":{"kind":"user","id":"user-1"}}`)
	request, err := http.NewRequest(http.MethodPost, "http://notification-core:3140/api/v1/notification-requests", bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest() error = %v", err)
	}
	request.Header.Set(HeaderServiceID, "verevon-gateway")
	request.Header.Set(HeaderUserID, "user-1")
	request.Header.Set(HeaderOrganizationID, "org-1")
	request.Header.Set(HeaderRole, "admin")
	request.Header.Set(HeaderTimestamp, "2026-07-13T12:00:00+00:00")
	request.Header.Set(HeaderNonce, "fixed-nonce-1234567890")
	request.Header.Set(HeaderBodySHA256, "KSAem_coGl1Xx_rU84ulYwo1-4Joui0ynxMypC4vHSk")
	request.Header.Set(HeaderSignature, "iZbxn0GTXuuy-tZveuwQcAN-0a2bND6iR_9VB07LwIM")

	principal, err := verifier.Verify(request, body)
	if err != nil {
		t.Fatalf("Verify() error = %v", err)
	}
	if principal.ServiceID != "verevon-gateway" || principal.OrganizationID != "org-1" || principal.Role != "admin" {
		t.Fatalf("principal = %#v", principal)
	}
}

type signedFields struct {
	serviceID      string
	secret         string
	timestamp      time.Time
	nonce          string
	userID         string
	organizationID string
	role           string
}

func signedRequest(t *testing.T, method, rawURL string, body []byte, fields signedFields) *http.Request {
	t.Helper()
	request, err := http.NewRequest(method, rawURL, bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest() error = %v", err)
	}
	timestamp := fields.timestamp.UTC().Format(time.RFC3339)
	digestBytes := sha256.Sum256(body)
	digest := base64.RawURLEncoding.EncodeToString(digestBytes[:])
	canonical := Canonical(CanonicalFields{
		ServiceID:      fields.serviceID,
		Audience:       "notification-core",
		Timestamp:      timestamp,
		Nonce:          fields.nonce,
		Method:         method,
		URI:            request.URL.RequestURI(),
		UserID:         fields.userID,
		OrganizationID: fields.organizationID,
		Role:           fields.role,
		BodySHA256:     digest,
	})
	mac := hmac.New(sha256.New, []byte(fields.secret))
	_, _ = mac.Write([]byte(canonical))
	signature := base64.RawURLEncoding.EncodeToString(mac.Sum(nil))

	request.Header.Set(HeaderServiceID, fields.serviceID)
	request.Header.Set(HeaderTimestamp, timestamp)
	request.Header.Set(HeaderNonce, fields.nonce)
	request.Header.Set(HeaderBodySHA256, digest)
	request.Header.Set(HeaderSignature, signature)
	if fields.userID != "" {
		request.Header.Set(HeaderUserID, fields.userID)
	}
	if fields.organizationID != "" {
		request.Header.Set(HeaderOrganizationID, fields.organizationID)
	}
	if fields.role != "" {
		request.Header.Set(HeaderRole, fields.role)
	}
	return request
}
