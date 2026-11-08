package handler

import (
	"math"
	"net/http"
	"regexp"
	"slices"
	"strconv"
	"strings"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

// libraryOriginPattern is one origin label: lowercase words joined by hyphens.
var libraryOriginPattern = regexp.MustCompile(`^[a-z][a-z0-9-]{0,63}$`)

const (
	libraryMaxOrigins   = 4
	libraryDefaultLimit = 50
	libraryMaxLimit     = 100
)

// Library: GET /v1/documents/library?origin=&limit=&offset=
//
// The library is a filtered view of the documents the caller may read: those
// whose metadata origin is one of the origins named (repeated, or comma-separated,
// four at most). origin is required, so the route never lists every document. A
// page carries metadata only; the text is read by id through GET
// /v1/documents/{documentID}. The viewer is always the verified principal, never
// the org-wide scope: a library is personal, so an org admin sees their own
// files, the files shared with them, and org-visible files, not every member's.
// The answer is never cached.
func (h *DocumentHandler) Library(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	query := r.URL.Query()
	origins, ok := libraryOrigins(query["origin"])
	if !ok {
		writeCodedError(w, http.StatusBadRequest, "invalid_origin",
			"origin must name one to four origin labels", map[string]any{"max_origins": libraryMaxOrigins})
		return
	}
	limit, ok := libraryInt(query.Get("limit"), libraryDefaultLimit, 1, libraryMaxLimit)
	if !ok {
		writeCodedError(w, http.StatusBadRequest, "invalid_paging",
			"limit must be a whole number from 1 to 100", map[string]any{"max_limit": libraryMaxLimit})
		return
	}
	offset, ok := libraryInt(query.Get("offset"), 0, 0, math.MaxInt32)
	if !ok {
		writeCodedError(w, http.StatusBadRequest, "invalid_paging", "offset must be a whole number", nil)
		return
	}

	// An empty viewer would make the predicate match every org document, so an
	// unverified caller is refused rather than given the whole org.
	viewer := principalID(r)
	if viewer == "" {
		writeCodedError(w, http.StatusForbidden, "library_requires_principal",
			"the library needs a verified principal", nil)
		return
	}
	orgID := OrgIDFrom(r.Context())
	result, err := h.repo.ListLibrary(r.Context(), model.ListLibraryInput{
		OrgID:      orgID,
		Origins:    origins,
		Limit:      limit,
		Offset:     offset,
		ViewerID:   viewer,
		GrantedIDs: h.grantedDocs(r, orgID, viewer),
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list library documents")
		return
	}
	writeJSON(w, http.StatusOK, result)
}

// libraryOrigins reads the origin labels: each value may hold a comma-separated
// list. An empty, malformed or over-long list is refused, never trimmed to fit.
func libraryOrigins(values []string) ([]string, bool) {
	var origins []string
	for _, value := range values {
		for _, part := range strings.Split(value, ",") {
			origin := strings.TrimSpace(part)
			if origin == "" {
				continue
			}
			if !libraryOriginPattern.MatchString(origin) {
				return nil, false
			}
			if !slices.Contains(origins, origin) {
				origins = append(origins, origin)
			}
		}
	}
	if len(origins) == 0 || len(origins) > libraryMaxOrigins {
		return nil, false
	}
	return origins, true
}

// libraryInt reads one paging number: the fallback when the parameter is absent,
// otherwise a whole number in [low, high].
func libraryInt(raw string, fallback, low, high int) (int, bool) {
	if raw == "" {
		return fallback, true
	}
	value, err := strconv.Atoi(raw)
	if err != nil || value < low || value > high {
		return 0, false
	}
	return value, true
}
