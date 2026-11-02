package http

import (
	"errors"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// WS15-S11: the room membership feed, a read-only view of the membership
// ledger for the timeline's system rows. It is registered through the P-UC1
// seam and shares the membership resolver's guard (signed gateway delegation)
// and scope (spaces:resolve): the caller needs a live membership of the room,
// and the organization comes from the delegation, never from the request.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.GET("/:space_ref/membership-events", s.requireVerifiedSpaceResolver, s.spaceMembershipFeed)
	})
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok || method != http.MethodGet {
			return "", false
		}
		parts := strings.Split(rest, "/")
		if len(parts) == 2 && parts[1] == "membership-events" {
			return "spaces:resolve", true
		}
		return "", false
	})
}

// spaceMembershipFeed answers GET /spaces/:space_ref/membership-events?after&before&limit.
func (s *Server) spaceMembershipFeed(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	params, err := spaces.NormalizeMembershipFeedParams(c.Query("after"), c.Query("before"), c.Query("limit"))
	if err != nil {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest,
			"after and before must be RFC 3339 times with after earlier than before, and limit from 1 to 100", nil)
		return
	}
	feed, err := s.spaceRepo.MembershipFeed(
		c.Request.Context(), c.Param("space_ref"), c.GetString("org_id"), c.GetString("user_id"), params,
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		s.writeSpaceDenial(c, err, c.Param("space_ref"), spaces.ExpectMembershipOnly, "current Space membership required", http.StatusInternalServerError, "Space membership feed failed")
		return
	}
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Space membership feed failed"})
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": feed})
}
