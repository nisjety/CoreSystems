package http

import (
	"context"
	"errors"
	"net/http"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// Room archive and restore routes (WS04-S7), under /api/v1/internal/spaces and
// answering the typed error envelope.
//
//	POST /:space_ref/archive    set an active room to archived (ACT)
//	POST /:space_ref/unarchive  restore an archived room (ACT)
//
// ACT = principal verevon-gateway + verified delegation + scope
// spaces:membership:act (the room writer's scope; the same credential switch
// keeps these dark). The acting user and organization come only from the
// delegation. The role rule (manager or owner) is applied by the repository
// inside the transaction that locks the room.

type archiveRoomBody struct {
	IdempotencyKey string `json:"idempotency_key"`
}

func (s *Server) archiveSpaceRoom(c *gin.Context)   { s.setSpaceRoomArchived(c, true) }
func (s *Server) unarchiveSpaceRoom(c *gin.Context) { s.setSpaceRoomArchived(c, false) }

func (s *Server) setSpaceRoomArchived(c *gin.Context, archive bool) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body archiveRoomBody
	if !decodeStrictBody(c, &body, true) {
		return
	}
	request := spaces.ArchiveRequest{
		SpaceRef:       c.Param("space_ref"),
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		IdempotencyKey: body.IdempotencyKey,
	}
	var result *spaces.ArchiveResult
	var err error
	if archive {
		result, err = s.spaceRepo.ArchiveSpace(c.Request.Context(), request)
	} else {
		result, err = s.spaceRepo.UnarchiveSpace(c.Request.Context(), request)
	}
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"state":              result.State,
		"changed":            result.Changed,
		"replayed":           result.Replayed,
		"archived_at":        result.ArchivedAt,
		"authority_revision": result.AuthorityRevision,
	}})
}

// refuseArchivedWrite answers a write decision (thread create or append) with
// the typed 409 room_archived when the strict membership resolution found no
// row because the room is archived and the caller is an active member of it.
// Everything else (a non-member, another organization's room, an unknown room)
// keeps the unchanged fail-closed denial, byte for byte. It reports true when
// it wrote the response.
func (s *Server) refuseArchivedWrite(c *gin.Context, resolveErr error, spaceRef string) bool {
	if !errors.Is(resolveErr, spaces.ErrNoCurrentMembership) {
		return false
	}
	archived, err := s.spaceRepo.ArchivedForMember(c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil || !archived {
		return false
	}
	abortTypedError(c, http.StatusConflict, spaces.CodeRoomArchived, "this room is archived", nil)
	return true
}

// resolveThreadWriteEvidence is resolveKindEvidence for the thread create and
// append decisions: strict active-only resolution, with a typed 409
// room_archived for an active member of an archived room.
func (s *Server) resolveThreadWriteEvidence(
	c *gin.Context,
	spaceRef string,
	personal, shared func(ctx context.Context, spaceRef, orgID, subjectID string) (spaces.PersonalThreadDecisionEvidence, error),
	audienceMsg string,
) (spaces.PersonalThreadDecisionEvidence, *spaces.CurrentMembership, bool) {
	return s.resolveKindEvidenceFor(c, spaceRef, s.spaceRepo.ResolveCurrentUserMembership, true, personal, shared, audienceMsg)
}

// resolveReadKindEvidence is resolveKindEvidence for a READ decision (shared
// retrieval): the membership step also resolves an archived room, and the
// shared resolver passed in is one of the widened read-class resolvers.
func (s *Server) resolveReadKindEvidence(
	c *gin.Context,
	spaceRef string,
	personal, shared func(ctx context.Context, spaceRef, orgID, subjectID string) (spaces.PersonalThreadDecisionEvidence, error),
	audienceMsg string,
) (spaces.PersonalThreadDecisionEvidence, *spaces.CurrentMembership, bool) {
	return s.resolveKindEvidenceFor(c, spaceRef, s.spaceRepo.ResolveReadableUserMembership, false, personal, shared, audienceMsg)
}
