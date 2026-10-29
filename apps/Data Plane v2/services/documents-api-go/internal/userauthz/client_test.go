package userauthz

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestListVisibleDocumentsSignsTenantSubjectAndRequest(t *testing.T) {
	const token = "0123456789abcdef0123456789abcdef"
	now := time.Date(2026, 7, 11, 2, 0, 0, 0, time.UTC)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Authorization"); got != "Bearer verified-user-token" {
			t.Fatalf("forwarded user proof = %q", got)
		}
		if r.Header.Get("X-Service-Id") != "documents-api" || r.Header.Get("X-Service-Token") != token {
			t.Fatal("missing service identity")
		}
		if r.Header.Get("X-User-Id") != "user-1" || r.Header.Get("X-Org-Id") != "org-1" {
			t.Fatal("delegated tenant or subject not bound")
		}
		if r.Header.Get("X-Delegation-Version") != "v2" || r.Header.Get("X-Delegation-Operation") != "authz:visible" || r.Header.Get("X-Delegation-Resource-Type") != "document" || r.Header.Get("X-Delegation-Reason") != "resolve explicit document grants" || r.Header.Get("X-Delegation-ZDR") != "true" {
			t.Fatal("bounded delegation metadata missing")
		}
		nonce := r.Header.Get("X-Delegation-Nonce")
		if len(nonce) < 16 {
			t.Fatal("delegation nonce missing")
		}
		digest := base64.RawURLEncoding.EncodeToString(sha256.New().Sum(nil))
		if r.Header.Get("X-Delegation-Body-SHA256") != digest {
			t.Fatalf("body digest = %q; want %q", r.Header.Get("X-Delegation-Body-SHA256"), digest)
		}
		canonical := strings.Join([]string{
			"v2", "documents-api", "user-core", now.Format(time.RFC3339),
			http.MethodGet, r.URL.RequestURI(), "user-1", "org-1", "authz:visible",
			"document", "", "resolve explicit document grants", "true", nonce, digest,
		}, "\n")
		mac := hmac.New(sha256.New, []byte(token))
		_, _ = mac.Write([]byte(canonical))
		wantSignature := base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
		if r.Header.Get("X-Delegation-Signature") != wantSignature {
			t.Fatal("delegation signature mismatch")
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"ids": []string{"doc-1"}, "all_org": false})
	}))
	defer server.Close()

	client := New(server.URL, token)
	client.now = func() time.Time { return now }
	ids, err := client.ListVisibleDocuments(context.Background(), "org-1", "user-1", "Bearer verified-user-token")
	if err != nil {
		t.Fatalf("ListVisibleDocuments: %v", err)
	}
	if len(ids) != 1 || ids[0] != "doc-1" {
		t.Fatalf("ids = %v", ids)
	}
}

func TestListVisibleDocumentsRequiresVerifiedUserProof(t *testing.T) {
	client := New("http://127.0.0.1:1", "test-only-service-token-0123456789")
	for _, authorization := range []string{"", "Bearer ", "Bearer token\r\nX-Forged: value"} {
		if _, err := client.ListVisibleDocuments(context.Background(), "org-1", "user-1", authorization); err == nil {
			t.Fatalf("authorization %q was accepted", authorization)
		}
	}
}
