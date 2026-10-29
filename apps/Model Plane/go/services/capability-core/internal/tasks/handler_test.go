package tasks

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/tasks", nil)
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
	if len(got.Tasks) == 0 {
		t.Fatal("expected non-empty Tasks")
	}
	for _, task := range got.Tasks {
		if task.OrgID != "triodelab" {
			t.Errorf("task %s: expected OrgID triodelab, got %q", task.ID, task.OrgID)
		}
		if !strings.HasPrefix(task.IdempotencyKey, idemPrefix+":") {
			t.Errorf("task %s: idempotency key missing prefix", task.ID)
		}
		switch task.Status {
		case StatusCreated, StatusAssigned, StatusBlocked, StatusCompleted:
		default:
			t.Errorf("task %s: invalid status %q", task.ID, task.Status)
		}
	}
}

func TestHandler_POST_MethodNotAllowed(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/model-plane/tasks", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
	if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
		t.Errorf("expected Allow: GET, got %q", allow)
	}
}
