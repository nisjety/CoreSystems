package schedule

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/schedule", nil)
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
	if len(got.ScheduledWork) == 0 {
		t.Fatal("expected non-empty ScheduledWork")
	}
	for _, sw := range got.ScheduledWork {
		if sw.OrgID != "triodelab" {
			t.Errorf("schedule %s: expected OrgID triodelab, got %q", sw.ID, sw.OrgID)
		}
		if !strings.HasPrefix(sw.IdempotencyKey, idemPrefix+":") {
			t.Errorf("schedule %s: idempotency key missing prefix", sw.ID)
		}
		switch sw.Kind {
		case KindCron, KindRemoteTrigger, KindRecurring:
		default:
			t.Errorf("schedule %s: invalid kind %q", sw.ID, sw.Kind)
		}
	}
}

func TestHandler_POST_MethodNotAllowed(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/model-plane/schedule", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
	if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
		t.Errorf("expected Allow: GET, got %q", allow)
	}
}
