package server

import (
	"net/http"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

func (s *Server) handleUsers(w http.ResponseWriter, r *http.Request) {
	principal, ok := s.principal(w, r)
	if !ok {
		return
	}
	// This report contains cross-user figures. The signed org-read grant is
	// required even for service principals; generic cost:read is insufficient.
	if !principal.HasScope("cost:org:read") {
		httpError(w, http.StatusForbidden, "organization cost read is not authorized")
		return
	}
	f, err := filterFromQuery(r)
	if err != nil {
		httpError(w, http.StatusBadRequest, err.Error())
		return
	}
	if !pinOrganization(w, f.OrgID, principal.OrganizationID) {
		return
	}
	f.OrgID = principal.OrganizationID
	if f.OrgID == "" || f.UserID != "" || f.RunID != "" || f.Model != "" {
		httpError(w, http.StatusBadRequest, "only the verified organization and time window are allowed")
		return
	}
	f, _, err = s.reportingFilter(r.Context(), f)
	if err != nil {
		httpError(w, http.StatusServiceUnavailable, "user usage reporting is temporarily unavailable")
		return
	}
	reader, ok := s.ledger.(ledger.UserUsageReader)
	if !ok {
		httpError(w, http.StatusServiceUnavailable, "user usage reporting is unavailable")
		return
	}
	rows, err := reader.AggregateUsers(r.Context(), f)
	if err != nil {
		httpError(w, mapHTTPStatus(err), err.Error())
		return
	}
	writeJSON(w, map[string]any{"data": rows})
}
