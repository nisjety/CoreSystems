package artifacts

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/artifacts", nil)
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
	if len(got.Artifacts) == 0 {
		t.Fatal("expected non-empty Artifacts")
	}
	for _, a := range got.Artifacts {
		if a.OrgID != "triodelab" {
			t.Errorf("artifact %s: expected OrgID triodelab, got %q", a.ID, a.OrgID)
		}
		if !strings.HasPrefix(a.IdempotencyKey, idemPrefix+":") {
			t.Errorf("artifact %s: idempotency key missing prefix", a.ID)
		}
		switch a.Kind {
		case KindImage, KindAudio, KindVideo, KindDocument:
		default:
			t.Errorf("artifact %s: invalid kind %q", a.ID, a.Kind)
		}
		if a.StorageURI == "" {
			t.Errorf("artifact %s: storageUri required for offload", a.ID)
		}
		if a.SizeBytes <= 0 {
			t.Errorf("artifact %s: expected positive SizeBytes, got %d", a.ID, a.SizeBytes)
		}
	}
}

func TestHandler_POST_MethodNotAllowed(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/model-plane/artifacts", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
	if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
		t.Errorf("expected Allow: GET, got %q", allow)
	}
}
