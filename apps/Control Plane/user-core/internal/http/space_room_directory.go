package http

import (
	"errors"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Room visibility, self-join and the room directory (WS04-S8), under
// /api/v1/internal and answering the typed error envelope.
//
//	POST  /spaces/:space_ref/join          self-join an open room (ACT)
//	PATCH /spaces/:space_ref/settings      visibility and join role (ACT)
//	GET   /directory/rooms                 browsable open rooms (DIR)
//	PUT   /spaces/directory-policy         per-organization switch (POL)
//
// ACT = verevon-gateway + verified delegation + spaces:membership:act (the
// writer credential is the kill switch). DIR = verevon-gateway + verified
// delegation + directory:read:self. POL = the control-space-policy principal
// with spaces:policy:write (operator path; a room owner can never enable the
// directory for their organization). The acting user and the organization come
// only from the delegation.

type joinSpaceRoomBody struct {
	IdempotencyKey string `json:"idempotency_key"`
}

type updateSpaceRoomSettingsBody struct {
	Visibility     *string `json:"visibility"`
	JoinRole       *string `json:"join_role"`
	HistoryMode    *string `json:"history_mode"`
	IdempotencyKey string  `json:"idempotency_key"`
}

type directoryPolicyBody struct {
	OrgID                 string `json:"org_id"`
	OrgPublicRoomsEnabled *bool  `json:"org_public_rooms_enabled"`
}

func (s *Server) joinSpaceRoom(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body joinSpaceRoomBody
	if !decodeStrictBody(c, &body, true) {
		return
	}
	result, err := s.spaceRepo.JoinSpace(c.Request.Context(), spaces.JoinRequest{
		SpaceRef:       c.Param("space_ref"),
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	s.publishMembershipOpEvent(c, result)
	c.Header("Cache-Control", "no-store")
	// recipient_ready is false here by contract: the recipient audience is
	// reconciled by Application after this write, and the client polls the
	// membership until it turns true.
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":           result.SpaceRef,
		"role":                result.Role,
		"changed":             result.Changed,
		"replayed":            result.Replayed,
		"already_member":      !result.Changed && !result.Replayed,
		"recipient_ready":     false,
		"membership_revision": result.MembershipRevision,
		"authority_revision":  result.AuthorityRevision,
		"member_count":        result.MemberCount,
	}})
}

func (s *Server) updateSpaceRoomSettings(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body updateSpaceRoomSettingsBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.UpdateRoomSettings(c.Request.Context(), spaces.SettingsRequest{
		SpaceRef:       c.Param("space_ref"),
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		Visibility:     body.Visibility,
		JoinRole:       body.JoinRole,
		HistoryMode:    body.HistoryMode,
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":          result.SpaceRef,
		"visibility":         result.Visibility,
		"join_role":          result.JoinRole,
		"history_mode":       result.HistoryMode,
		"changed":            result.Changed,
		"replayed":           result.Replayed,
		"authority_revision": result.AuthorityRevision,
		"privacy_revision":   result.PrivacyRevision,
	}})
}

func (s *Server) listDirectoryRooms(c *gin.Context) {
	if s.spaceRepo == nil {
		abortTypedError(c, http.StatusServiceUnavailable, "directory_unavailable", "the directory is unavailable", nil)
		return
	}
	rooms, err := s.spaceRepo.ListDirectoryRooms(c.Request.Context(), c.GetString("org_id"), c.GetString("user_id"))
	switch {
	case errors.Is(err, spaces.ErrDirectoryCallerNotInOrg):
		abortTypedError(c, http.StatusForbidden, "member_not_in_organization",
			"organization membership is required", nil)
		return
	case err != nil:
		log.Error().Err(err).Msg("room directory read failed")
		abortTypedError(c, http.StatusServiceUnavailable, "directory_unavailable", "the directory is unavailable", nil)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": rooms})
}

func (s *Server) upsertSpaceDirectoryPolicy(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body directoryPolicyBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	if strings.TrimSpace(body.OrgID) == "" {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "org_id is required",
			map[string]any{"field": "org_id"})
		return
	}
	if body.OrgPublicRoomsEnabled == nil {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "org_public_rooms_enabled is required",
			map[string]any{"field": "org_public_rooms_enabled"})
		return
	}
	updated, err := s.spaceRepo.UpsertDirectoryPolicy(c.Request.Context(), spaces.DirectoryPolicy{
		OrgID:                 body.OrgID,
		OrgPublicRoomsEnabled: *body.OrgPublicRoomsEnabled,
	}, c.GetString("service_id"))
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"updated": updated}})
}
