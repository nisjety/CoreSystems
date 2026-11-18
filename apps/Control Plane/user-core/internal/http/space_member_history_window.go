package http

import (
	"errors"
	"net/http"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// D18: per-member room history visibility, on the membership writer seam. Both routes take the
// gateway delegation and the spaces:membership:act scope, the same guard as the add route, and
// answer the typed error envelope. The acting user and the organization come only from the
// delegation, never from the body or the path. A room manager or owner may read and change a
// member's window (CanManageRoom); anyone else is refused.
//
//	PUT /:space_ref/members/:subject_id/history-window   set the window (ACT)
//	GET /:space_ref/members/:subject_id/history-window   read the window (ACT)
//
// PUT body: {"mode": "full"|"since_join"|"since_date", "since_at": RFC 3339 instant (since_date only),
// "expected_revision": optional int64}. Set-to-state: the same window answers changed=false.

// putMemberHistoryWindowBody is the strict body of the admin change. Unknown fields are refused.
type putMemberHistoryWindowBody struct {
	Mode             string     `json:"mode"`
	SinceAt          *time.Time `json:"since_at"`
	ExpectedRevision *int64     `json:"expected_revision"`
}

func (s *Server) putSpaceMemberHistoryWindow(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body putMemberHistoryWindowBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.SetMemberHistoryWindow(c.Request.Context(), spaces.MemberHistoryWindowRequest{
		SpaceRef:         c.Param("space_ref"),
		OrgID:            c.GetString("org_id"),
		ActorID:          c.GetString("user_id"),
		SubjectID:        c.Param("subject_id"),
		Mode:             body.Mode,
		SinceAt:          body.SinceAt,
		ExpectedRevision: body.ExpectedRevision,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":      result.SpaceRef,
		"subject_id":     result.SubjectID,
		"changed":        result.Changed,
		"history_window": result.Window,
	}})
}

func (s *Server) getSpaceMemberHistoryWindow(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	result, err := s.spaceRepo.MemberHistoryWindowFor(c.Request.Context(), c.Param("space_ref"),
		c.GetString("org_id"), c.GetString("user_id"), c.Param("subject_id"))
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":      result.SpaceRef,
		"subject_id":     result.SubjectID,
		"history_window": result.Window,
	}})
}
