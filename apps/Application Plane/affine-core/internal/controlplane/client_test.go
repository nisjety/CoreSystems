package controlplane

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestResolveActorContext(t *testing.T) {
	authServer := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Cookie") != "session=ok" {
			writer.WriteHeader(http.StatusUnauthorized)
			return
		}

		_ = json.NewEncoder(writer).Encode(map[string]any{
			"authenticated": true,
			"data": map[string]any{
				"user": map[string]any{
					"id":    "user-123",
					"email": "test@example.com",
					"name":  "Test User",
				},
				"session": map[string]any{
					"id": "session-123",
				},
			},
		})
	}))
	defer authServer.Close()

	userServer := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Header.Get("X-User-Id") != "user-123" {
			writer.WriteHeader(http.StatusUnauthorized)
			return
		}

		_ = json.NewEncoder(writer).Encode(map[string]any{
			"userId": "user-123",
			"orgId":  "org-456",
			"role":   "admin",
		})
	}))
	defer userServer.Close()

	client := NewClient(Config{
		AuthServiceURL: authServer.URL,
		UserServiceURL: userServer.URL,
		InternalAPIKey: "internal-key",
	})

	actor, err := client.ResolveActorContext(context.Background(), "session=ok")
	if err != nil {
		t.Fatalf("expected actor context, got error: %v", err)
	}

	if actor.UserID != "user-123" || actor.OrgID != "org-456" || actor.Role != "admin" {
		t.Fatalf("unexpected actor context: %+v", actor)
	}
}

func TestResolveActorContextRequiresAuthentication(t *testing.T) {
	client := NewClient(Config{
		AuthServiceURL: "http://localhost:1",
		UserServiceURL: "http://localhost:1",
		InternalAPIKey: "internal-key",
	})

	_, err := client.ResolveActorContext(context.Background(), "")
	if !errors.Is(err, ErrUnauthenticated) {
		t.Fatalf("expected ErrUnauthenticated, got %v", err)
	}
}
