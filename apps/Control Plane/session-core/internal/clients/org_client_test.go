package clients

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestNewOrgClient_EmptyBaseURL_ReturnsNil(t *testing.T) {
	if NewOrgClient("") != nil {
		t.Fatal("expected nil for empty baseURL")
	}
}

func TestNewOrgClient_ValidURL_ReturnsNonNil(t *testing.T) {
	if NewOrgClient("http://localhost:9999") == nil {
		t.Fatal("expected non-nil client for valid URL")
	}
}

func TestValidateMembership_ActiveMember_ReturnsRole(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasSuffix(r.URL.Path, "/members") {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"members": []map[string]any{
				{"id": "m1", "org_id": "org1", "user_id": "user1", "role": "admin", "status": "active"},
			},
			"count": 1,
		})
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	role, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if role != "admin" {
		t.Fatalf("expected role %q, got %q", "admin", role)
	}
}

func TestValidateMembership_MultipleMembersActiveMatch_ReturnsCorrectRole(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"members": []map[string]any{
				{"id": "m1", "org_id": "org1", "user_id": "other-user", "role": "admin", "status": "active"},
				{"id": "m2", "org_id": "org1", "user_id": "target-user", "role": "member", "status": "active"},
			},
			"count": 2,
		})
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	role, err := c.ValidateMembership(context.Background(), "org1", "target-user")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if role != "member" {
		t.Fatalf("expected role %q, got %q", "member", role)
	}
}

func TestValidateMembership_UserNotInList_ReturnsError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"members": []map[string]any{},
			"count":   0,
		})
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	_, err := c.ValidateMembership(context.Background(), "org1", "missing-user")
	if err == nil {
		t.Fatal("expected error for user not in list, got nil")
	}
}

func TestValidateMembership_InactiveMember_ReturnsError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"members": []map[string]any{
				{"id": "m1", "org_id": "org1", "user_id": "user1", "role": "member", "status": "inactive"},
			},
			"count": 1,
		})
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	_, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err == nil {
		t.Fatal("expected error for inactive member, got nil")
	}
}

func TestValidateMembership_HTTP500_ReturnsError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	_, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err == nil {
		t.Fatal("expected error for HTTP 500, got nil")
	}
}

func TestValidateMembership_HTTP403_ReturnsError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	_, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err == nil {
		t.Fatal("expected error for HTTP 403, got nil")
	}
}

func TestValidateMembership_MalformedJSON_ReturnsError(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{invalid json`))
	}))
	defer ts.Close()

	c := NewOrgClient(ts.URL)
	_, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err == nil {
		t.Fatal("expected error for malformed JSON, got nil")
	}
}

func TestValidateMembership_ServerUnreachable_ReturnsError(t *testing.T) {
	c := NewOrgClient("http://127.0.0.1:1") // port 1 should be unreachable
	_, err := c.ValidateMembership(context.Background(), "org1", "user1")
	if err == nil {
		t.Fatal("expected error for unreachable server, got nil")
	}
}
