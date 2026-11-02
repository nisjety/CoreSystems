package http

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Control-native room membership writer routes (WS04-S4), all under
// /api/v1/internal/spaces and all answering the typed error envelope
// {"error":{"code","message","details"}}.
//
//	POST   /:space_ref/members                add (ACT)
//	PATCH  /:space_ref/members/:subject_id    change role (ACT)
//	DELETE /:space_ref/members/:subject_id    remove, or leave when the subject is the actor (ACT)
//	GET    /:space_ref/audience-candidates    audience read (APP)
//
// ACT = principal verevon-gateway + verified delegation + scope
// spaces:membership:act. The acting user and the organization come only from
// the signed delegation, never from the body or the path. The credential is the
// kill switch: a deployment that does not grant spaces:membership:act to the
// gateway answers 403 on every ACT route, so the writer ships dark.
// APP = principal application-space-lifecycle + scope spaces:membership:read
// (a service read, no end user).
const (
	spaceMembershipActScope  = "spaces:membership:act"
	spaceMembershipReadScope = "spaces:membership:read"
)

// requireSpaceMembershipActor admits only the gateway workload carrying a
// verified delegation and the dedicated writer scope. It is modelled on
// requireVerifiedSpaceResolver but is its own scope, so a deployment can
// enable resolution without enabling writes.
func (s *Server) requireSpaceMembershipActor(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != verevonGatewayPrincipal ||
		!c.GetBool("delegation_verified") ||
		!hasServiceScope(c, spaceMembershipActScope) ||
		strings.TrimSpace(c.GetString("user_id")) == "" ||
		strings.TrimSpace(c.GetString("org_id")) == "" {
		abortTypedError(c, http.StatusForbidden, "room_delegation_required",
			"a signed gateway delegation with the membership writer scope is required", nil)
		return
	}
	c.Next()
}

// requireSpaceAudienceReader admits only the Application lifecycle workload
// with the audience-read scope. It is separate from the membership writer
// scope (spaces:membership:write) on purpose: reading candidates is not
// writing a roster.
func (s *Server) requireSpaceAudienceReader(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != applicationSpaceLifecyclePrincipal ||
		!hasServiceScope(c, spaceMembershipReadScope) {
		abortTypedError(c, http.StatusForbidden, "audience_reader_required",
			"the Application Space lifecycle principal with the membership read scope is required", nil)
		return
	}
	c.Next()
}

// writeRoomError answers a repository error. Typed room errors keep their
// status and code; anything else is an authority outage (503), logged without
// request content.
func writeRoomError(c *gin.Context, err error) {
	var roomErr *spaces.RoomError
	if errors.As(err, &roomErr) {
		abortTypedError(c, roomErr.Status, roomErr.Code, roomErr.Message, roomErr.Details)
		return
	}
	log.Error().Err(err).Msg("space membership operation failed")
	abortTypedError(c, http.StatusServiceUnavailable, spaces.CodeSpaceAuthorityUnavailable,
		"space authority is unavailable", nil)
}

// decodeStrictBody decodes a JSON body rejecting unknown fields. An empty body
// is allowed when optional is true. It writes a typed 400 and returns false on
// failure.
func decodeStrictBody(c *gin.Context, dst any, optional bool) bool {
	raw, err := io.ReadAll(io.LimitReader(c.Request.Body, 1<<16))
	if err != nil {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "the request body could not be read", nil)
		return false
	}
	if len(bytes.TrimSpace(raw)) == 0 {
		if optional {
			return true
		}
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "a JSON body is required", nil)
		return false
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(dst); err != nil || decoder.More() {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "the request body is invalid", nil)
		return false
	}
	return true
}

// publishMembershipOpEvent announces a user row that became active or
// inactive. A role-only change is ledger-only and is never published.
func (s *Server) publishMembershipOpEvent(c *gin.Context, result *spaces.MemberOpResult) {
	if result == nil || result.Event == nil || result.Event.Action == spaces.ActionRoleChanged {
		return
	}
	event := result.Event
	s.publisher.PublishSpaceMembershipEvent(c.Request.Context(), event.SpaceRef, event.OrgID, event.SubjectID,
		event.Active, event.EventID, event.SpaceSeq, event.Action, event.Role, event.ActorKind)
}

type addSpaceMemberBody struct {
	SubjectID      string `json:"subject_id"`
	Role           string `json:"role"`
	IdempotencyKey string `json:"idempotency_key"`
}

type changeSpaceMemberRoleBody struct {
	Role             string `json:"role"`
	ExpectedRevision *int64 `json:"expected_revision"`
	IdempotencyKey   string `json:"idempotency_key"`
}

type removeSpaceMemberBody struct {
	IdempotencyKey string `json:"idempotency_key"`
}

func (s *Server) addSpaceMember(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body addSpaceMemberBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.AddMember(c.Request.Context(), spaces.MemberOpRequest{
		SpaceRef:       c.Param("space_ref"),
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		SubjectID:      body.SubjectID,
		Role:           body.Role,
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	s.publishMembershipOpEvent(c, result)
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":           result.SpaceRef,
		"subject_id":          result.SubjectID,
		"role":                result.Role,
		"changed":             result.Changed,
		"replayed":            result.Replayed,
		"membership_revision": result.MembershipRevision,
		"authority_revision":  result.AuthorityRevision,
		"member_count":        result.MemberCount,
	}})
}

func (s *Server) changeSpaceMemberRole(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body changeSpaceMemberRoleBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.ChangeMemberRole(c.Request.Context(), spaces.MemberOpRequest{
		SpaceRef:         c.Param("space_ref"),
		OrgID:            c.GetString("org_id"),
		ActorID:          c.GetString("user_id"),
		SubjectID:        c.Param("subject_id"),
		Role:             body.Role,
		IdempotencyKey:   body.IdempotencyKey,
		ExpectedRevision: body.ExpectedRevision,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	s.publishMembershipOpEvent(c, result)
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref":           result.SpaceRef,
		"subject_id":          result.SubjectID,
		"role":                result.Role,
		"from_role":           result.FromRole,
		"changed":             result.Changed,
		"replayed":            result.Replayed,
		"membership_revision": result.MembershipRevision,
		"authority_revision":  result.AuthorityRevision,
		"member_count":        result.MemberCount,
	}})
}

func (s *Server) removeSpaceMember(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	var body removeSpaceMemberBody
	if !decodeStrictBody(c, &body, true) {
		return
	}
	result, err := s.spaceRepo.RemoveMember(c.Request.Context(), spaces.MemberOpRequest{
		SpaceRef:       c.Param("space_ref"),
		OrgID:          c.GetString("org_id"),
		ActorID:        c.GetString("user_id"),
		SubjectID:      c.Param("subject_id"),
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeRoomError(c, err)
		return
	}
	s.publishMembershipOpEvent(c, result)
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"changed":             result.Changed,
		"left":                result.Left,
		"member_count":        result.MemberCount,
		"membership_revision": result.MembershipRevision,
	}})
}

func (s *Server) spaceAudienceCandidates(c *gin.Context) {
	if s.spaceRepo == nil {
		writeRoomError(c, errors.New("space authority repository unavailable"))
		return
	}
	candidates, err := s.spaceRepo.AudienceCandidates(c.Request.Context(), c.Param("space_ref"))
	if err != nil {
		writeRoomError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": candidates})
}
