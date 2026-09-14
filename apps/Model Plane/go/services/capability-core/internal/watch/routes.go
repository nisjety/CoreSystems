package watch

import (
	"net/http"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// Register mounts the watch surface.
//
// `/api/v1/watches` is deliberately NOT under `/api/v1/spaces/...`: a watch
// belongs to a Space but is owned by this service, and nesting it under the
// Space path would imply Control serves it.
func (h *Handler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/watches", h.listOrCreate)
	mux.HandleFunc("/api/v1/watches/", func(w http.ResponseWriter, r *http.Request) {
		id := strings.TrimSpace(strings.TrimPrefix(r.URL.Path, "/api/v1/watches/"))
		if id == "" || strings.Contains(id, "/") {
			writeError(w, http.StatusNotFound, "no such watch")
			return
		}
		if r.Method != http.MethodDelete {
			writeError(w, http.StatusMethodNotAllowed, "only DELETE is supported for one watch")
			return
		}
		orgID, subjectID, ok := humanCaller(w, r)
		if !ok {
			return
		}
		h.Cancel(w, r, orgID, subjectID, id)
	})
}

func (h *Handler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	orgID, subjectID, ok := humanCaller(w, r)
	if !ok {
		return
	}
	switch r.Method {
	case http.MethodGet:
		h.List(w, r, orgID, r.URL.Query().Get("space_ref"), r.URL.Query().Get("include_finished") == "true")
	case http.MethodPost:
		h.Create(w, r, orgID, subjectID)
	default:
		writeError(w, http.StatusMethodNotAllowed, "only GET and POST are supported")
	}
}

// humanCaller resolves the verified identity behind a watch request.
//
// A watch is a person's standing intent, addressed to that person, and only its
// creator may cancel it — so every route here needs a real subject. A service
// principal is refused rather than given an empty one: an empty subject would
// make the creator check vacuous, and a watch nobody owns is a watch nobody can
// stop.
func humanCaller(w http.ResponseWriter, r *http.Request) (orgID, subjectID string, ok bool) {
	principal, found := authctx.PrincipalFromContext(r.Context())
	if !found {
		writeError(w, http.StatusUnauthorized, "a verified identity is required")
		return "", "", false
	}
	if principal.PrincipalType != "user" || strings.TrimSpace(principal.ActorID) == "" {
		writeError(w, http.StatusForbidden, "a watch belongs to a person; a service identity cannot hold one")
		return "", "", false
	}
	if strings.TrimSpace(principal.OrganizationID) == "" {
		writeError(w, http.StatusForbidden, "a verified organization is required")
		return "", "", false
	}
	return principal.OrganizationID, principal.ActorID, true
}
