package coordination

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/coordination", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	if ct := rec.Header().Get("Content-Type"); ct != "application/json" {
		t.Errorf("expected Content-Type application/json, got %q", ct)
	}

	var got Catalog
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("response not valid JSON catalog: %v", err)
	}
	if len(got.Teams) == 0 {
		t.Fatal("expected non-empty Teams")
	}
	for _, team := range got.Teams {
		if team.OrgID != "triodelab" {
			t.Errorf("team %s: expected OrgID triodelab, got %q", team.ID, team.OrgID)
		}
		if !strings.HasPrefix(team.IdempotencyKey, idemPrefix+":") {
			t.Errorf("team %s: idempotency key missing prefix", team.ID)
		}
		if team.ParentRunID == "" {
			t.Errorf("team %s: expected non-empty ParentRunID", team.ID)
		}
	}
}

func TestHandler_POST_MethodNotAllowed(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/model-plane/coordination", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
	if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
		t.Errorf("expected Allow: GET, got %q", allow)
	}
}
