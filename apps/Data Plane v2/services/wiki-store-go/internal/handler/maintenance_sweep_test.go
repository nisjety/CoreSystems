package handler

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
)

func TestMaintenanceSweepReturnsNon2xxWhenEveryWriteFails(t *testing.T) {
	_, verifier, bearer := routeTestIdentity(t)
	var captured model.CreateMaintenanceLogInput
	h := &WikiHandler{
		createMaintenanceLog: func(_ context.Context, input model.CreateMaintenanceLogInput) (*model.MaintenanceLog, error) {
			captured = input
			return nil, errors.New("database details must not leak")
		},
	}
	router := chi.NewRouter()
	router.With(authctx.Middleware(verifier)).Post("/v1/wiki/maintenance/sweep", h.MaintenanceSweep)
	req := httptest.NewRequest(http.MethodPost, "/v1/wiki/maintenance/sweep", strings.NewReader(`{
		"items":[{"page_id":"page-1","kind":"stale_wiki","actor":"spoofed-user"}]
	}`))
	req.Header.Set("Authorization", "Bearer "+bearer)
	response := httptest.NewRecorder()

	router.ServeHTTP(response, req)

	if response.Code < 400 {
		t.Fatalf("status = %d, want non-2xx; body=%s", response.Code, response.Body.String())
	}
	if strings.Contains(response.Body.String(), "database details") {
		t.Fatalf("response leaked internal error: %s", response.Body.String())
	}
	if captured.OrgID != "org-authorized" || captured.Actor != "user-authorized" {
		t.Fatalf("audit identity = (%q, %q), want verified org/user", captured.OrgID, captured.Actor)
	}
}

func TestMaintenanceSweepValidationFailureIs400WithoutWrites(t *testing.T) {
	_, verifier, bearer := routeTestIdentity(t)
	writes := 0
	h := &WikiHandler{createMaintenanceLog: func(context.Context, model.CreateMaintenanceLogInput) (*model.MaintenanceLog, error) {
		writes++
		return &model.MaintenanceLog{}, nil
	}}
	router := chi.NewRouter()
	router.With(authctx.Middleware(verifier), authctx.RequireScope("wiki.maintenance.write")).Post("/v1/wiki/maintenance/sweep", h.MaintenanceSweep)
	req := httptest.NewRequest(http.MethodPost, "/v1/wiki/maintenance/sweep", strings.NewReader(`{
		"items":[{"kind":"stale_wiki"},{"page_id":"page-1","kind":"arbitrary"}]
	}`))
	req.Header.Set("Authorization", "Bearer "+bearer)
	response := httptest.NewRecorder()

	router.ServeHTTP(response, req)

	if response.Code != http.StatusBadRequest || writes != 0 {
		t.Fatalf("status/writes = (%d, %d), want (400, 0); body=%s", response.Code, writes, response.Body.String())
	}
}
