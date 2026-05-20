// Package api wires the two read endpoints. Both require the caller to
// scope by `org_id`. In Phase A · A1.6 the auth check is a shared
// internal-api-key header (same pattern as the other Control Plane
// services); A1.3/A1.2 enforce mode replaces it with the auth-core JWT
// once Wave 3 lands.
package api

import (
	"crypto/subtle"
	"encoding/json"
	"net/http"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

type API struct {
	store           *store.Store
	internalAPIKey  string
}

func New(s *store.Store, internalAPIKey string) *API {
	return &API{store: s, internalAPIKey: internalAPIKey}
}

func (a *API) Mount(r chi.Router) {
	r.Get("/healthz", a.healthz)
	r.Get("/readyz", a.healthz)

	r.Route("/v1", func(r chi.Router) {
		r.Use(a.internalAuth)
		r.Get("/audit", a.listAudit)
		r.Get("/usage", a.listUsage)
		r.Get("/usage/summary", a.summariseUsage)
	})
}

func (a *API) healthz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(`{"status":"ok"}`))
}

func (a *API) internalAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if a.internalAPIKey == "" {
			next.ServeHTTP(w, r)
			return
		}
		provided := r.Header.Get("X-Internal-Api-Key")
		if provided == "" {
			provided = r.Header.Get("X-Api-Key")
		}
		if subtle.ConstantTimeCompare([]byte(provided), []byte(a.internalAPIKey)) != 1 {
			writeJSON(w, http.StatusUnauthorized, map[string]string{"error": "unauthorized"})
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (a *API) listAudit(w http.ResponseWriter, r *http.Request) {
	orgID := r.URL.Query().Get("org_id")
	if orgID == "" {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "org_id required"})
		return
	}

	filter := store.AuditFilter{
		OrgID:  orgID,
		Since:  parseTime(r.URL.Query().Get("since")),
		Until:  parseTime(r.URL.Query().Get("until")),
		Event:  r.URL.Query().Get("event"),
		UserID: r.URL.Query().Get("user_id"),
		Limit:  parseInt(r.URL.Query().Get("limit"), 100),
	}
	rows, err := a.store.ListAudit(r.Context(), filter)
	if err != nil {
		log.Error().Err(err).Str("org_id", orgID).Msg("listAudit failed")
		writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "query failed"})
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": rows, "meta": map[string]any{
		"count": len(rows),
		"limit": filter.Limit,
	}, "error": nil})
}

func (a *API) listUsage(w http.ResponseWriter, r *http.Request) {
	orgID := r.URL.Query().Get("org_id")
	if orgID == "" {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "org_id required"})
		return
	}
	filter := store.UsageFilter{
		OrgID: orgID,
		Since: parseTime(r.URL.Query().Get("since")),
		Until: parseTime(r.URL.Query().Get("until")),
		Plane: r.URL.Query().Get("plane"),
		Op:    r.URL.Query().Get("op"),
		Limit: parseInt(r.URL.Query().Get("limit"), 200),
	}
	rows, err := a.store.ListUsage(r.Context(), filter)
	if err != nil {
		log.Error().Err(err).Str("org_id", orgID).Msg("listUsage failed")
		writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "query failed"})
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": rows, "meta": map[string]any{
		"count": len(rows),
		"limit": filter.Limit,
	}, "error": nil})
}

func (a *API) summariseUsage(w http.ResponseWriter, r *http.Request) {
	orgID := r.URL.Query().Get("org_id")
	if orgID == "" {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "org_id required"})
		return
	}
	rows, err := a.store.SummariseUsage(
		r.Context(), orgID,
		parseTime(r.URL.Query().Get("since")),
		parseTime(r.URL.Query().Get("until")),
	)
	if err != nil {
		log.Error().Err(err).Str("org_id", orgID).Msg("summariseUsage failed")
		writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "query failed"})
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": rows, "meta": map[string]any{
		"count": len(rows),
	}, "error": nil})
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func parseTime(s string) time.Time {
	if s == "" {
		return time.Time{}
	}
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}
	}
	return t
}

func parseInt(s string, fallback int) int {
	if s == "" {
		return fallback
	}
	v, err := strconv.Atoi(s)
	if err != nil || v <= 0 {
		return fallback
	}
	return v
}
