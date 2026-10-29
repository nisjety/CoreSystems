package modalities

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/modalities", nil)
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
	if len(got.Modalities) != 8 {
		t.Fatalf("expected 8 modalities (chat/completions/images/speech/translation/documents/video/realtime), got %d", len(got.Modalities))
	}

	want := map[Kind]bool{
		KindChat: true, KindCompletions: true, KindImages: true, KindSpeech: true,
		KindTranslation: true, KindDocuments: true, KindVideo: true, KindRealtime: true,
	}
	seen := map[Kind]bool{}
	for _, m := range got.Modalities {
		if m.OrgID != "triodelab" {
			t.Errorf("modality %s: expected OrgID triodelab, got %q", m.ID, m.OrgID)
		}
		if !strings.HasPrefix(m.IdempotencyKey, idemPrefix+":") {
			t.Errorf("modality %s: idempotency key missing prefix", m.ID)
		}
		if !want[m.Kind] {
			t.Errorf("modality %s: unexpected kind %q", m.ID, m.Kind)
		}
		if m.PrimaryRoute == "" {
			t.Errorf("modality %s: primaryRoute required for provider routing", m.ID)
		}
		seen[m.Kind] = true
	}
	for k := range want {
		if !seen[k] {
			t.Errorf("missing modality kind %q", k)
		}
	}
}

func TestHandler_POST_MethodNotAllowed(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/model-plane/modalities", nil)
	rec := httptest.NewRecorder()

	NewHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", rec.Code)
	}
	if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
		t.Errorf("expected Allow: GET, got %q", allow)
	}
}
