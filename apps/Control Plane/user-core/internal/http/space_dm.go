package http

import (
	"errors"
	"net/http"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Stable codes of the direct-message pre-check (part of the gateway contract).
const (
	codeDMDisabled            = "dm_disabled"
	codeDMNotEntitled         = "dm_not_entitled"
	codeDMParticipantsInvalid = "dm_participants_invalid"
	codeDMExists              = "dm_exists"
)

type dmEligibilityRequest struct {
	// ParticipantIDs are the OTHER people (the caller is added from the
	// verified delegation, never from the body).
	ParticipantIDs []string `json:"participant_ids"`
}

// dmEligibility answers whether the delegated caller may open a DM with the
// listed people, and returns the canonical roster and participant hash the
// gateway then hands to Application as `expectedHash`. Typed envelope on every
// refusal: 404 dm_disabled, 403 dm_not_entitled (details.missing), 422
// dm_participants_invalid (details.reason). It writes nothing.
func (s *Server) dmEligibility(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var body dmEligibilityRequest
	if !decodeStrictBody(c, &body, false) {
		return
	}
	result, err := s.spaceRepo.DMEligibility(c.Request.Context(),
		c.GetString("org_id"), c.GetString("user_id"), body.ParticipantIDs)
	if err != nil {
		writeDMRefusal(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}

func writeDMRefusal(c *gin.Context, err error) {
	var notEntitled *spaces.DMNotEntitledError
	var invalid *spaces.DMParticipantInvalidError
	switch {
	case errors.Is(err, spaces.ErrDMDisabled):
		abortTypedError(c, http.StatusNotFound, codeDMDisabled, "direct messages are not enabled", nil)
	case errors.As(err, &notEntitled):
		abortTypedError(c, http.StatusForbidden, codeDMNotEntitled,
			"direct messages are not entitled for this organization",
			map[string]any{"missing": notEntitled.Missing})
	case errors.As(err, &invalid):
		abortTypedError(c, http.StatusUnprocessableEntity, codeDMParticipantsInvalid,
			"the participants cannot form a direct message", map[string]any{"reason": invalid.Reason})
	default:
		log.Error().Err(err).Msg("direct message eligibility failed")
		abortTypedError(c, http.StatusServiceUnavailable, spaces.CodeSpaceAuthorityUnavailable,
			"space authority is unavailable", nil)
	}
}

// dmRegistrationRefusalCode maps a Register refusal of a direct message to the
// stable code returned beside the legacy 409 body. ok is false for any other
// error, which keeps its existing mapping.
func dmRegistrationRefusalCode(err error) (string, bool) {
	switch {
	case errors.Is(err, spaces.ErrDMDisabled):
		return codeDMDisabled, true
	case errors.Is(err, spaces.ErrDMNotEntitled):
		return codeDMNotEntitled, true
	case errors.Is(err, spaces.ErrDMParticipantInvalid):
		return codeDMParticipantsInvalid, true
	case errors.Is(err, spaces.ErrDMExists):
		return codeDMExists, true
	}
	return "", false
}
