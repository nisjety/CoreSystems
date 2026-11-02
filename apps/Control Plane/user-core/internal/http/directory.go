package http

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/directory"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// People directory (WS04-S3). GET /api/v1/internal/directory/people is a read
// for the verevon-gateway workload carrying a signed delegation: the
// organization and the caller come from the delegation only, never from the
// query string. Scope directory:read:self (it ends in :self, so it requires a
// verified delegation). Removing that scope from the gateway credential is the
// hard kill switch.
const directoryReadScope = "directory:read:self"

// peopleDirectory is the narrow storage surface of the route, so tests can
// inject a fake. *directory.Store implements it.
type peopleDirectory interface {
	SearchPeople(ctx context.Context, q directory.PeopleQuery) (directory.PeopleResult, error)
}

// SetPeopleDirectory wires the people-directory store. Unwired, the route
// answers 503 directory_unavailable.
func (s *Server) SetPeopleDirectory(store peopleDirectory) {
	s.peopleDirectory = store
}

func (s *Server) requireDirectoryReader(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != verevonGatewayPrincipal ||
		!c.GetBool("delegation_verified") ||
		!hasServiceScope(c, directoryReadScope) ||
		strings.TrimSpace(c.GetString("user_id")) == "" ||
		strings.TrimSpace(c.GetString("org_id")) == "" {
		abortTypedError(c, http.StatusForbidden, "directory_delegation_required",
			"a signed gateway delegation is required", nil)
		return
	}
	c.Next()
}

func (s *Server) searchDirectoryPeople(c *gin.Context) {
	if s.peopleDirectory == nil {
		abortTypedError(c, http.StatusServiceUnavailable, "directory_unavailable", "the directory is unavailable", nil)
		return
	}
	limit := 0
	if raw := strings.TrimSpace(c.Query("limit")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil {
			abortTypedError(c, http.StatusBadRequest, "invalid_request", "limit must be a number",
				map[string]any{"field": "limit"})
			return
		}
		limit = parsed
	}
	result, err := s.peopleDirectory.SearchPeople(c.Request.Context(), directory.PeopleQuery{
		OrgID:           c.GetString("org_id"),
		CallerID:        c.GetString("user_id"),
		Q:               c.Query("q"),
		Limit:           limit,
		Cursor:          c.Query("cursor"),
		ExcludeMemberOf: c.Query("exclude_member_of"),
	})
	switch {
	case errors.Is(err, directory.ErrInvalidRequest):
		abortTypedError(c, http.StatusBadRequest, "invalid_request", "the directory query is invalid", nil)
		return
	case errors.Is(err, directory.ErrCallerNotInOrg):
		abortTypedError(c, http.StatusForbidden, "member_not_in_organization",
			"organization membership is required", nil)
		return
	case err != nil:
		log.Error().Err(err).Msg("people directory search failed")
		abortTypedError(c, http.StatusServiceUnavailable, "directory_unavailable", "the directory is unavailable", nil)
		return
	}
	var next any
	if result.NextCursor != "" {
		next = result.NextCursor
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"people": result.People, "next_cursor": next}})
}
