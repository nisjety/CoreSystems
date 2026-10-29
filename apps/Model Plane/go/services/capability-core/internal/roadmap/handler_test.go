package roadmap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestHandler_GET_ReturnsCatalog(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/api/v1/model-plane/implementation-status", nil)
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
	if len(got.ServiceChecklist) == 0 {
		t.Error("response ServiceChecklist empty")
	}
	if len(got.BackendRuntime) == 0 {
		t.Error("response BackendRuntime empty")
	}
	if len(got.ProductShell) == 0 {
		t.Error("response ProductShell empty")
	}
	if len(got.ClaudeDonorRoadmap) == 0 {
		t.Error("response ClaudeDonorRoadmap empty")
	}
	if len(got.ModelPlaneV2Parity) == 0 {
		t.Error("response ModelPlaneV2Parity empty")
	}
}

func TestHandler_NonGET_Returns405(t *testing.T) {
	for _, method := range []string{http.MethodPost, http.MethodPut, http.MethodDelete, http.MethodPatch} {
		t.Run(method, func(t *testing.T) {
			req := httptest.NewRequest(method, "/api/v1/model-plane/implementation-status", nil)
			rec := httptest.NewRecorder()

			NewHandler().ServeHTTP(rec, req)

			if rec.Code != http.StatusMethodNotAllowed {
				t.Errorf("expected 405, got %d", rec.Code)
			}
			if allow := rec.Header().Get("Allow"); allow != http.MethodGet {
				t.Errorf("expected Allow: GET, got %q", allow)
			}
		})
	}
}
