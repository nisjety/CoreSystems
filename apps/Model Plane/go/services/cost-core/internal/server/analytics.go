package server

import (
	"errors"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"net/http"
	"time"
)

func (s *Server) handleAnalytics(w http.ResponseWriter, r *http.Request) {
	p, ok := s.principal(w, r)
	if !ok {
		return
	}
	scope := r.URL.Query().Get("scope")
	if scope == "" {
		scope = "me"
	}
	if scope != "me" && scope != "organization" {
		httpError(w, 400, "invalid usage scope")
		return
	}
	f, err := filterFromQuery(r)
	if err != nil {
		httpError(w, 400, err.Error())
		return
	}
	if !pinOrganization(w, f.OrgID, p.OrganizationID) {
		return
	}
	f.OrgID = p.OrganizationID
	if f.OrgID == "" {
		httpError(w, 403, "organization is required")
		return
	}
	if scope == "organization" {
		if !p.HasScope("cost:org:read") {
			httpError(w, 403, "organization cost read is not authorized")
			return
		}
		if f.UserID != "" {
			httpError(w, 400, "organization report cannot select a user")
			return
		}
	} else {
		if p.PrincipalType != "user" || p.ActorID == "" || (f.UserID != "" && f.UserID != p.ActorID) {
			httpError(w, 403, "user scope does not match verified token")
			return
		}
		f.UserID = p.ActorID
	}
	if f.Model != "" || f.RunID != "" {
		httpError(w, 400, "only scope and time window are allowed")
		return
	}
	f, start, err := s.reportingFilter(r.Context(), f)
	if err != nil {
		httpError(w, 503, "usage reporting is temporarily unavailable")
		return
	}
	reader, ok := s.ledger.(ledger.AnalyticsReader)
	if !ok {
		httpError(w, 503, "usage reporting is unavailable")
		return
	}
	// Freeze an upper bound so the export documents exactly what was read.
	now := time.Now().UTC()
	if f.Until.IsZero() || f.Until.After(now) {
		f.Until = now
	}
	rows, err := reader.UsageAnalytics(r.Context(), f)
	if errors.Is(err, ledger.ErrAnalyticsTooLarge) {
		httpError(w, 422, err.Error())
		return
	}
	if err != nil {
		httpError(w, 503, "usage reporting is temporarily unavailable")
		return
	}
	var from *time.Time
	if !f.Since.IsZero() {
		from = &f.Since
	}
	writeJSON(w, map[string]any{"data": map[string]any{
		"buckets": rows, "scope": scope, "reporting_started_at": start, "from": from, "to": f.Until, "timezone": "UTC",
	}})
}
